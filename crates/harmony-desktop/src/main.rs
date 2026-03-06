#![allow(mismatched_lifetime_syntaxes)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod api;
pub mod errors;
pub mod icons;
pub mod media;
pub mod preferences;
pub mod theme;
pub mod views;
pub mod widgets;

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::Duration,
};

use iced::{
    Color, Element, Length, Task, Theme,
    widget::{container, mouse_area, stack, text},
};
use tokio::sync::mpsc::UnboundedReceiver;

#[cfg(target_os = "windows")]
mod platform {
    use iced::window;

    const GWL_HWNDPARENT: i32 = -8;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetWindowLongPtrW(hwnd: isize, index: i32, new_long: isize) -> isize;
        fn EnableWindow(hwnd: isize, enable: i32) -> i32;
        fn SetForegroundWindow(hwnd: isize) -> i32;
    }

    /// Extract the Win32 HWND from an iced window via `window::run`.
    pub fn extract_hwnd(id: window::Id) -> iced::Task<Option<isize>> {
        window::run(id, |w| {
            w.window_handle().ok().and_then(|h| match h.as_raw() {
                iced::window::raw_window_handle::RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
                _ => None,
            })
        })
    }

    /// Set the settings window as an owned window of the main window,
    /// and disable the main window to get true modal behavior.
    pub unsafe fn set_modal_owner(primary_hwnd: isize, secondary_hwnd: isize) {
        unsafe {
            SetWindowLongPtrW(secondary_hwnd, GWL_HWNDPARENT, primary_hwnd);
        }
        unsafe {
            EnableWindow(primary_hwnd, 0);
        }
    }

    /// Re-enable the main window and bring it to foreground.
    pub unsafe fn unset_modal_owner(primary_hwnd: isize) {
        unsafe {
            EnableWindow(primary_hwnd, 1);
        }
        unsafe {
            SetForegroundWindow(primary_hwnd);
        }
    }
}

use iced::window;

use crate::{
    api::{ApiClient, Channel, CurrentUser, account},
    views::{
        login::{LoginMessage, LoginView},
        main::MainView,
        splash::{SplashMessage, SplashView},
    },
};
use crate::{
    theme::TEXT_PRIMARY,
    views::{
        dialogs::{
            backend::{BackendMessage, BackendView},
            external_link::{ExternalLinkMessage, ExternalLinkView},
            mfa::{MfaMessage, MfaView},
            settings::{SettingsMessage, SettingsView},
        },
        main::MainMessage,
    },
};

#[derive(Debug, Clone)]
pub struct ChatUser {
    pub name: String,
    pub avatar_color_start: Color,
    #[allow(dead_code)]
    pub avatar_color_end: Color,
}

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    CallCard { channel: String, duration: String },
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub user: MessageAuthor,
    pub time: i64,
    pub content: MessageContent,
}

#[derive(Debug, Clone)]
pub enum MessageAuthor {
    User {
        id: String,
        name: String,
        avatar_color_start: Color,
        avatar_color_end: Color,
    },
    System,
}

impl MessageAuthor {
    pub fn id(&self) -> String {
        match self {
            MessageAuthor::User { id, .. } => id.clone(),
            MessageAuthor::System => "SYSTEM".into(),
            // this will never collide with actual ids since they are ulids
        }
    }
    pub fn name(&self) -> String {
        match self {
            MessageAuthor::User { name, .. } => name.clone(),
            MessageAuthor::System => "System".into(),
        }
    }
    pub fn avatar_color(&self) -> Color {
        match self {
            MessageAuthor::User {
                avatar_color_start, ..
            } => *avatar_color_start,
            MessageAuthor::System => TEXT_PRIMARY,
        }
    }
}

rust_i18n::i18n!("i18n");

pub enum AppWindowView {
    Splash(SplashView),
    Login(LoginView),
    Main(MainView),
    Mfa(MfaView),
    Settings(SettingsView),
    ExternalLink(ExternalLinkView),
    Backend(BackendView),
}

pub struct AppWindow {
    view: AppWindowView,
    /// for dialog windows, the parent window they are modal to
    parent: Option<window::Id>,
    #[cfg(target_os = "windows")]
    hwnd: Option<isize>,
}

impl AppWindow {
    fn new(view: AppWindowView) -> Self {
        Self {
            view,
            parent: None,
            #[cfg(target_os = "windows")]
            hwnd: None,
        }
    }

    fn dialog(view: AppWindowView, parent: window::Id) -> Self {
        Self {
            view,
            parent: Some(parent),
            #[cfg(target_os = "windows")]
            hwnd: None,
        }
    }
}

struct App {
    windows: BTreeMap<window::Id, AppWindow>,
    api: Option<Arc<dyn ApiClient>>,
    backend_account: String,
    backend_harmony: String,
}

pub type EventReceiver = UnboundedReceiver<harmony_api::Event>;

pub type LoginResult = (Arc<dyn ApiClient>, CurrentUser, HashMap<String, Channel>);

#[derive(Clone)]
pub enum Message {
    Splash(SplashMessage),
    SplashFinished(Option<LoginResult>),
    Login(LoginMessage),
    LoginFinished(LoginResult),
    OpenMfa(account::LoginMfa),
    Mfa(MfaMessage),
    Main(MainMessage),
    ExternalLink(ExternalLinkMessage),
    OpenExternalLink(window::Id, String),
    CloseExternalLink,
    OpenSettings,
    FocusSettings,
    Settings(SettingsMessage),
    OpenBackend,
    Backend(BackendMessage),
    BackendChanged(String, String),
    CloseBackend,
    Logout,
    WindowClosed(window::Id),
    #[cfg(target_os = "windows")]
    HwndCaptured(window::Id, Option<isize>),
}

pub static DEFAULT_BASE_URL_AS: &'static str = "https://account.nextania.com";
// pub static DEFAULT_BASE_URL_HARMONY: &'static str = "wss://chat.nextania.com";
pub static DEFAULT_BASE_URL_HARMONY: &'static str = "ws://localhost:9005";

impl App {
    fn new() -> (Self, Task<Message>) {
        let (splash_id, open_splash) = window::open(window::Settings {
            size: iced::Size::new(400.0, 200.0),
            position: window::Position::Centered,
            resizable: false,
            decorations: false,
            ..Default::default()
        });
        // TODO: in the splash screen:
        // - check for updates
        // - check authentication
        let delay_task = Task::perform(tokio::time::sleep(Duration::from_millis(1000)), |_| {
            Message::SplashFinished(None)
        });
        (
            Self {
                windows: BTreeMap::from([(
                    splash_id,
                    AppWindow::new(AppWindowView::Splash(views::splash::SplashView::new())),
                )]),
                api: None,
                backend_account: DEFAULT_BASE_URL_AS.to_string(),
                backend_harmony: DEFAULT_BASE_URL_HARMONY.to_string(),
            },
            Task::batch([open_splash.discard(), delay_task]),
        )
    }

    fn open_dialog(
        &mut self,
        parent: window::Id,
        view: AppWindowView,
        size: iced::Size,
    ) -> Task<Message> {
        let (dialog_id, open_task) = window::open(window::Settings {
            size,
            position: window::Position::Centered,
            resizable: false,
            minimizable: false,
            #[cfg(target_os = "windows")]
            platform_specific: window::settings::PlatformSpecific {
                skip_taskbar: true,
                ..Default::default()
            },
            ..Default::default()
        });
        self.windows
            .insert(dialog_id, AppWindow::dialog(view, parent));

        #[cfg(target_os = "windows")]
        return open_task
            .then(move |_| platform::extract_hwnd(dialog_id))
            .map(move |hwnd| Message::HwndCaptured(dialog_id, hwnd));
        #[cfg(not(target_os = "windows"))]
        return open_task.discard();
    }

    fn close_dialog(&mut self, predicate: impl Fn(&AppWindowView) -> bool) -> Task<Message> {
        let Some((id, window)) = self
            .windows
            .extract_if(.., |_, w| predicate(&w.view))
            .next()
        else {
            return Task::none();
        };

        let _parent = window.parent;

        #[cfg(target_os = "windows")]
        if let Some(parent_id) = _parent {
            if let Some(parent_hwnd) = self.windows.get(&parent_id).and_then(|w| w.hwnd) {
                unsafe {
                    platform::unset_modal_owner(parent_hwnd);
                }
            }
        }

        window::close(id)
    }

    fn find_and_focus(&self, predicate: impl Fn(&AppWindowView) -> bool) -> Option<Task<Message>> {
        self.windows
            .iter()
            .find(|(_, w)| predicate(&w.view))
            .map(|(id, _)| window::gain_focus(*id))
    }

    fn find_window_id(&self, predicate: impl Fn(&AppWindowView) -> bool) -> Option<window::Id> {
        self.windows
            .iter()
            .find(|(_, w)| predicate(&w.view))
            .map(|(id, _)| *id)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SplashFinished(login_result) => {
                let mut tasks = vec![];
                if login_result.is_none() {
                    let (login_id, open_task) = window::open(window::Settings {
                        size: iced::Size::new(800.0, 550.0),
                        position: window::Position::Centered,
                        resizable: false,
                        ..Default::default()
                    });
                    self.windows.insert(
                        login_id,
                        AppWindow::new(AppWindowView::Login(views::login::LoginView::new(
                            login_id,
                            self.backend_account.clone(),
                            self.backend_harmony.clone(),
                        ))),
                    );
                    #[cfg(target_os = "windows")]
                    let open_done = open_task
                        .then(move |_| platform::extract_hwnd(login_id))
                        .map(move |hwnd| Message::HwndCaptured(login_id, hwnd));
                    #[cfg(not(target_os = "windows"))]
                    let open_done = open_task.discard();
                    tasks.push(open_done);
                } else {
                    // TODO: If we already have valid credentials (e.g. from a previous session), skip the login screen and go straight to the main window
                }
                let close_splash = self
                    .windows
                    .extract_if(.., |_, w| matches!(w.view, AppWindowView::Splash(_)))
                    .next()
                    .map(|(id, _)| window::close(id))
                    .unwrap_or_else(Task::none);
                tasks.push(close_splash);
                return Task::batch(tasks);
            }
            Message::LoginFinished((api, current_user, conversations)) => {
                let (main_id, open_task) = window::open(window::Settings {
                    size: iced::Size::new(1100.0, 700.0),
                    position: window::Position::Centered,
                    resizable: true,
                    ..Default::default()
                });
                let close_login = self
                    .windows
                    .extract_if(.., |_, w| matches!(w.view, AppWindowView::Login(_)))
                    .next()
                    .map(|(id, _)| window::close(id))
                    .unwrap_or_else(Task::none);
                let close_mfa = self
                    .windows
                    .extract_if(.., |_, w| matches!(w.view, AppWindowView::Mfa(_)))
                    .next()
                    .map(|(id, _)| window::close(id))
                    .unwrap_or_else(Task::none);
                let close_backend = self
                    .windows
                    .extract_if(.., |_, w| matches!(w.view, AppWindowView::Backend(_)))
                    .next()
                    .map(|(id, _)| window::close(id))
                    .unwrap_or_else(Task::none);
                self.api = Some(api.clone());
                self.windows.insert(
                    main_id,
                    AppWindow::new(AppWindowView::Main(MainView::new(
                        api,
                        current_user,
                        conversations,
                    ))),
                );

                #[cfg(target_os = "windows")]
                let open_done = open_task
                    .then(move |_| platform::extract_hwnd(main_id))
                    .map(move |hwnd| Message::HwndCaptured(main_id, hwnd));
                #[cfg(not(target_os = "windows"))]
                let open_done = open_task.discard();

                return Task::batch([
                    open_done,
                    close_login,
                    close_mfa,
                    close_backend,
                ]);
            }
            Message::OpenMfa(mfa) => {
                let parent = self
                    .find_window_id(|v| matches!(v, AppWindowView::Login(_)))
                    .expect("Login window should exist when opening MFA");
                return self.open_dialog(
                    parent,
                    AppWindowView::Mfa(MfaView::new(mfa, self.backend_harmony.clone())),
                    iced::Size::new(400.0, 200.0),
                );
            }
            Message::Mfa(mfa_message) => {
                let (
                    _,
                    AppWindow {
                        view: AppWindowView::Mfa(v),
                        ..
                    },
                ) = self
                    .windows
                    .iter_mut()
                    .find(|(_, w)| matches!(w.view, AppWindowView::Mfa(_)))
                    .expect("MFA window should exist")
                else {
                    unreachable!()
                };
                return v.update(mfa_message);
            }
            Message::ExternalLink(external_link_message) => {
                let (
                    _,
                    AppWindow {
                        view: AppWindowView::ExternalLink(v),
                        ..
                    },
                ) = self
                    .windows
                    .iter_mut()
                    .find(|(_, w)| matches!(w.view, AppWindowView::ExternalLink(_)))
                    .expect("External link window should exist")
                else {
                    unreachable!()
                };
                return v.update(external_link_message);
            }
            Message::OpenExternalLink(parent, link) => {
                if let Some(task) =
                    self.find_and_focus(|v| matches!(v, AppWindowView::ExternalLink(_)))
                {
                    return task;
                }
                return self.open_dialog(
                    parent,
                    AppWindowView::ExternalLink(ExternalLinkView::new(link)),
                    iced::Size::new(400.0, 200.0),
                );
            }
            Message::CloseExternalLink => {
                return self.close_dialog(|v| matches!(v, AppWindowView::ExternalLink(_)));
            }
            Message::Login(login_message) => {
                let (
                    _,
                    AppWindow {
                        view: AppWindowView::Login(v),
                        ..
                    },
                ) = self
                    .windows
                    .iter_mut()
                    .find(|(_, w)| matches!(w.view, AppWindowView::Login(_)))
                    .expect("Login window should exist")
                else {
                    unreachable!()
                };
                return v.update(login_message);
            }
            Message::Main(main_message) => {
                let (
                    _,
                    AppWindow {
                        view: AppWindowView::Main(v),
                        ..
                    },
                ) = self
                    .windows
                    .iter_mut()
                    .find(|(_, w)| matches!(w.view, AppWindowView::Main(_)))
                    .expect("Main window should exist")
                else {
                    unreachable!()
                };
                return v.update(main_message);
            }
            Message::Logout => {
                // TODO: clear tokens
                self.api = None;
                let close_settings = self.close_dialog(|v| matches!(v, AppWindowView::Settings(_)));
                let close_main = self
                    .windows
                    .extract_if(.., |_, w| matches!(w.view, AppWindowView::Main(_)))
                    .next()
                    .map(|(id, _)| window::close(id))
                    .unwrap_or_else(Task::none);
                let (login_id, open_login) = window::open(window::Settings {
                    size: iced::Size::new(800.0, 550.0),
                    position: window::Position::Centered,
                    resizable: false,
                    ..Default::default()
                });
                self.windows.insert(
                    login_id,
                    AppWindow::new(AppWindowView::Login(views::login::LoginView::new(
                        login_id,
                        self.backend_account.clone(),
                        self.backend_harmony.clone(),
                    ))),
                );
                #[cfg(target_os = "windows")]
                let open_login = open_login
                    .then(move |_| platform::extract_hwnd(login_id))
                    .map(move |hwnd| Message::HwndCaptured(login_id, hwnd));
                #[cfg(not(target_os = "windows"))]
                let open_login = open_login.discard();
                return Task::batch([close_main, close_settings, open_login]);
            }
            Message::OpenSettings => {
                if let Some(task) = self.find_and_focus(|v| matches!(v, AppWindowView::Settings(_)))
                {
                    return task;
                }
                let parent = self
                    .find_window_id(|v| matches!(v, AppWindowView::Main(_)))
                    .expect("Main window should exist when opening Settings");
                return self.open_dialog(
                    parent,
                    AppWindowView::Settings(SettingsView::new()),
                    iced::Size::new(800.0, 520.0),
                );
            }
            Message::FocusSettings => {
                if let Some(task) = self.find_and_focus(|v| matches!(v, AppWindowView::Settings(_)))
                {
                    return task;
                }
            }
            Message::Settings(msg) => {
                let (
                    _,
                    AppWindow {
                        view: AppWindowView::Settings(v),
                        ..
                    },
                ) = self
                    .windows
                    .iter_mut()
                    .find(|(_, w)| matches!(w.view, AppWindowView::Settings(_)))
                    .expect("Settings window should exist")
                else {
                    unreachable!()
                };
                return v.update(msg);
            }
            Message::OpenBackend => {
                if let Some(task) = self.find_and_focus(|v| matches!(v, AppWindowView::Backend(_)))
                {
                    return task;
                }
                let parent = self
                    .find_window_id(|v| matches!(v, AppWindowView::Login(_)))
                    .expect("Login window should exist when opening backend settings");
                return self.open_dialog(
                    parent,
                    AppWindowView::Backend(BackendView::new(
                        self.backend_account.clone(),
                        self.backend_harmony.clone(),
                    )),
                    iced::Size::new(420.0, 240.0),
                );
            }
            Message::Backend(msg) => {
                let (
                    _,
                    AppWindow {
                        view: AppWindowView::Backend(v),
                        ..
                    },
                ) = self
                    .windows
                    .iter_mut()
                    .find(|(_, w)| matches!(w.view, AppWindowView::Backend(_)))
                    .expect("Backend window should exist")
                else {
                    unreachable!()
                };
                return v.update(msg);
            }
            Message::BackendChanged(account, harmony) => {
                self.backend_account = account.clone();
                self.backend_harmony = harmony.clone();
                let close = self.close_dialog(|v| matches!(v, AppWindowView::Backend(_)));
                let notify = if self
                    .windows
                    .values()
                    .any(|w| matches!(w.view, AppWindowView::Login(_)))
                {
                    Task::done(Message::Login(LoginMessage::BackendUpdated(
                        account, harmony,
                    )))
                } else {
                    Task::none()
                };
                return Task::batch([close, notify]);
            }
            Message::CloseBackend => {
                return self.close_dialog(|v| matches!(v, AppWindowView::Backend(_)));
            }
            #[cfg(target_os = "windows")]
            Message::HwndCaptured(id, hwnd) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.hwnd = hwnd;
                }
                if let Some(window) = self.windows.get(&id) {
                    if let Some(parent_id) = window.parent {
                        let child_hwnd = window.hwnd;
                        let parent_hwnd = self.windows.get(&parent_id).and_then(|w| w.hwnd);
                        if let (Some(child_hwnd), Some(parent_hwnd)) = (child_hwnd, parent_hwnd) {
                            unsafe {
                                platform::set_modal_owner(parent_hwnd, child_hwnd);
                            }
                        }
                    }
                }
            }
            Message::WindowClosed(id) => {
                let Some(window) = self.windows.remove(&id) else {
                    // window was closed by the app
                    return Task::none();
                };
                // enable parent window if this was a dialog
                #[cfg(target_os = "windows")]
                if let Some(parent_id) = window.parent {
                    if let Some(parent_hwnd) = self.windows.get(&parent_id).and_then(|w| w.hwnd) {
                        unsafe {
                            platform::unset_modal_owner(parent_hwnd);
                        }
                    }
                }
                match window.view {
                    AppWindowView::Splash(_) | AppWindowView::Login(_) => {
                        return iced::exit();
                    }
                    AppWindowView::Main(_) => {
                        if let Some(settings_window) = self
                            .windows
                            .extract_if(.., |_, w| matches!(w.view, AppWindowView::Settings(_)))
                            .map(|(id, _)| id)
                            .next()
                        {
                            let _ = window::close::<Message>(settings_window);
                            self.windows.remove(&settings_window);
                        }
                        // TODO: if stay_running is enabled, we should keep running in the background
                        if self.windows.is_empty() {
                            return iced::exit();
                        }
                    }
                    _ => return Task::none(),
                }
            }
        }
        Task::none()
    }

    fn view(&self, id: window::Id) -> Element<Message> {
        match match self.windows.get(&id) {
            Some(w) => Some(&w.view),
            None => None,
        } {
            Some(AppWindowView::ExternalLink(v)) => v.view().map(Message::ExternalLink),
            Some(AppWindowView::Splash(v)) => v.view().map(Message::Splash),
            Some(AppWindowView::Login(v)) => v.view().map(Message::Login),
            Some(AppWindowView::Mfa(v)) => v.view().map(Message::Mfa),
            Some(AppWindowView::Main(v)) => {
                let main = v.view().map(Message::Main);
                if self
                    .windows
                    .iter()
                    .any(|(_, w)| matches!(w.view, AppWindowView::Settings(_)))
                {
                    let overlay = mouse_area(
                        container(iced::widget::text(""))
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .style(|_theme| container::Style {
                                background: Some(iced::Background::Color(Color {
                                    r: 0.0,
                                    g: 0.0,
                                    b: 0.0,
                                    a: 0.4,
                                })),
                                ..Default::default()
                            }),
                    )
                    .on_press(Message::FocusSettings);
                    stack![main, overlay].into()
                } else {
                    main
                }
            }
            Some(AppWindowView::Settings(v)) => v.view().map(Message::Settings),
            Some(AppWindowView::Backend(v)) => v.view().map(Message::Backend),
            None => text("").into(),
        }
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        window::close_events().map(Message::WindowClosed)
    }

    fn theme(&self, _id: window::Id) -> Theme {
        Theme::Dark
    }

    fn title(&self, id: window::Id) -> String {
        match self.windows.get(&id).map(|w| &w.view) {
            Some(AppWindowView::ExternalLink(_)) => "External link".into(),
            Some(AppWindowView::Splash(_)) => "Loading...".into(),
            Some(AppWindowView::Login(_)) => "Log in".into(),
            Some(AppWindowView::Mfa(_)) => "Two-factor authentication".into(),
            Some(AppWindowView::Main(_)) => "Harmony".into(),
            Some(AppWindowView::Settings(_)) => "Settings".into(),
            Some(AppWindowView::Backend(_)) => "Custom server URLs".into(),
            None => "Harmony".into(),
        }
    }
}

fn main() -> iced::Result {
    rust_i18n::set_locale("en");
    iced::daemon(App::new, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .font(include_bytes!("../assets/fonts/dm-sans-variable.ttf"))
        .font(include_bytes!(
            "../assets/fonts/dm-sans-italic-variable.ttf"
        ))
        .font(include_bytes!("../assets/fonts/fluentsystemicons-resizable.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/noto-sans-sc-variable.ttf").as_slice())
        .default_font(iced::Font::with_name("Noto Sans SC"))
        .run()
}

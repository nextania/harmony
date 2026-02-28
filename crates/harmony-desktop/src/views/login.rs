use async_stream::stream;
use harmony_api::Event;
use iced::{
    Border, Color, Element, Font, Length, Padding, Shadow, Task, Theme, Vector, alignment, color,
    widget::{
        Space, Svg, button, column, container, image, row, stack, svg::Handle, text, text_input,
    },
    window,
};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    Message, api::{account, live::LiveApiClient}, errors::RenderableError, icons::{FLUENT_ICONS, Icon}, theme::{
        ACCENT_PURPLE, BG_LOGIN_CARD, BG_LOGIN_INPUT, BORDER_CARD, DM_SANS, LINK_COLOR, LOGIN_BG,
        LOGO_SVG, SUBTLE_GREY, TEXT_MUTED, TEXT_WHITE,
    }, views::main::MainMessage, widgets::button::ButtonExt
};

use crate::api::{ApiClient, Channel, CurrentUser};
use std::{collections::HashMap, sync::Arc};

enum LoginFlow {
    Done(
        Arc<dyn ApiClient>,
        CurrentUser,
        HashMap<String, Channel>,
        UnboundedReceiver<Event>
    ),
    NeedsMfa(account::LoginMfa),
}

#[derive(Clone)]
pub enum LoginMessage {
    EmailChanged(String),
    PasswordChanged(String),
    Submit,
    Failed(RenderableError),
    OpenExternalLink(String),
    OpenBackend,
    OpenToken,
    BackendUpdated(String, String),
}

pub struct LoginView {
    email: String,
    password: String,
    logo_handle: Handle,
    bg_handle: image::Handle,
    login_error: Option<String>,
    id: window::Id,
    backend_account: String,
    backend_harmony: String,
}

impl LoginView {
    pub fn new(id: window::Id, backend_account: String, backend_harmony: String) -> Self {
        Self {
            email: String::new(),
            password: String::new(),
            logo_handle: Handle::from_memory(LOGO_SVG),
            bg_handle: image::Handle::from_bytes(LOGIN_BG),
            login_error: None,
            id,
            backend_account,
            backend_harmony,
        }
    }
    pub fn update(&mut self, message: LoginMessage) -> Task<Message> {
        match message {
            LoginMessage::EmailChanged(s) => {
                self.login_error = None;
                self.email = s;
            }
            LoginMessage::PasswordChanged(s) => {
                self.login_error = None;
                self.password = s;
            }
            LoginMessage::OpenExternalLink(s) => {
                return Task::done(Message::OpenExternalLink(self.id, s));
            }
            LoginMessage::OpenBackend => {
                return Task::done(Message::OpenBackend);
            }
            LoginMessage::OpenToken => {
                return Task::done(Message::OpenToken);
            }
            LoginMessage::BackendUpdated(account, harmony) => {
                self.backend_account = account;
                self.backend_harmony = harmony;
            }
            LoginMessage::Submit => {
                let email = self.email.clone();
                let password = self.password.clone();
                let backend_account = self.backend_account.clone();
                let backend_harmony = self.backend_harmony.clone();
                return Task::stream(
                    stream! {
                        let result = async {
                            match account::login(&backend_account, &email, &password).await? {
                                account::LoginResult::Success(token) => {
                                    let (client, stream) = LiveApiClient::connect(&backend_harmony, &token).await?;
                                    let current_user = client.get_current_user().await?;
                                    let conversations = client.get_conversations().await?
                                        .into_iter().map(|c| (c.id(), c)).collect();
                                    Ok::<_, RenderableError>(LoginFlow::Done(client, current_user, conversations, stream))
                                }
                                account::LoginResult::RequiresContinuation(mfa) => {
                                    Ok::<_, RenderableError>(LoginFlow::NeedsMfa(mfa))
                                }
                            }
                        }.await;
                        match result {
                            Ok(LoginFlow::Done(client, user, convs, mut stream)) => {
                                yield Message::LoginFinished((client, user, convs));
                                while let Some(event) = stream.recv().await {
                                    yield Message::Main(MainMessage::ServerEvent(event));
                                }
                            }
                            Ok(LoginFlow::NeedsMfa(mfa)) => yield Message::OpenMfa(mfa),
                            Err(e) => yield Message::Login(LoginMessage::Failed(e)),
                        }
                    }
                );
            }
            LoginMessage::Failed(e) => {
                self.login_error = Some(e.to_string());
            }
        }
        Task::none()
    }
    pub fn view(&self) -> Element<LoginMessage> {
        let logo_mark = Svg::new(self.logo_handle.clone()).width(41).height(41);

        let logo_text = column![
            text("Harmony").size(24).color(TEXT_WHITE).font(Font {
                weight: iced::font::Weight::Medium,
                ..DM_SANS
            }),
            text("by Nextania").size(16).color(TEXT_WHITE).font(Font {
                weight: iced::font::Weight::ExtraLight,
                ..DM_SANS
            }),
        ];

        let logo = row![logo_mark, logo_text]
            .spacing(12)
            .align_y(alignment::Vertical::Center);

        let globe = container(
            text(Icon::GlobeRegular.unicode())
                .size(20)
                .color(TEXT_MUTED)
                .font(FLUENT_ICONS),
        )
        .padding(Padding::from([2, 2]));

        let card_header = row![logo, Space::new().width(Length::Fill), globe,]
            .align_y(alignment::Vertical::Center);

        let title_block = column![
            text("Sign in").size(24).color(TEXT_WHITE).font(Font {
                weight: iced::font::Weight::Bold,
                ..DM_SANS
            }),
            text("Authenticate with your Nextania account.")
                .size(14)
                .color(TEXT_WHITE)
                .font(Font {
                    weight: iced::font::Weight::Medium,
                    ..DM_SANS
                }),
        ]
        .spacing(4);

        let input_style = |_theme: &Theme, _status: text_input::Status| text_input::Style {
            background: iced::Background::Color(BG_LOGIN_INPUT),
            border: Border {
                color: SUBTLE_GREY,
                width: 1.0,
                radius: 4.into(),
            },
            icon: SUBTLE_GREY,
            placeholder: SUBTLE_GREY,
            value: TEXT_WHITE,
            selection: ACCENT_PURPLE,
        };

        let email_input = text_input("Email", &self.email)
            .on_input(LoginMessage::EmailChanged)
            .size(12)
            .font(DM_SANS)
            .style(input_style)
            .width(Length::Fill);

        let password_input = text_input("Password", &self.password)
            .on_input(LoginMessage::PasswordChanged)
            .on_submit(LoginMessage::Submit)
            .secure(true)
            .size(12)
            .font(DM_SANS)
            .style(input_style)
            .width(Length::Fill);

        let sign_in_btn = button(
            container(
                text("Sign in")
                    .size(12)
                    .color(TEXT_WHITE)
                    .font(DM_SANS)
                    .align_x(alignment::Horizontal::Center),
            )
            .center_x(Length::Fill),
        )
        .on_press(LoginMessage::Submit)
        .width(Length::Fill)
        .padding(Padding::from([4, 8]))
        .style(|_theme, status| button::Style {
            background: Some(iced::Background::Color(match status {
                button::Status::Hovered => color!(0xa000cc),
                button::Status::Pressed => color!(0x6e008a),
                _ => ACCENT_PURPLE,
            })),
            border: Border::default().rounded(4),
            text_color: TEXT_WHITE,
            ..Default::default()
        })
        .cursor_default();

        let use_passkey = text("Use a passkey")
            .size(12)
            .color(LINK_COLOR)
            .font(DM_SANS);

        let fields_col = {
            let mut col = column![email_input, password_input, sign_in_btn, use_passkey]
                .spacing(8)
                .width(Length::Fill);
            if let Some(err) = &self.login_error {
                col = col.push(
                    text(err.as_str())
                        .size(12)
                        .color(iced::color!(0xff4444))
                        .font(DM_SANS),
                );
            }
            col
        };

        let more_options = column![
            text("More options").size(12).color(SUBTLE_GREY).font(Font {
                weight: iced::font::Weight::Light,
                ..DM_SANS
            }),
            button(
                text("Sign in with token")
                    .size(12)
                    .color(LINK_COLOR)
                    .font(DM_SANS)
            )
            .padding(Padding::ZERO)
            .style(|_theme, _status| button::Style {
                background: None,
                border: Border::default(),
                text_color: LINK_COLOR,
                ..Default::default()
            })
            .on_press(LoginMessage::OpenToken),
            button(
                text("Create an account")
                    .size(12)
                    .color(LINK_COLOR)
                    .font(DM_SANS)
            )
            .padding(Padding::ZERO)
            .style(|_theme, _status| button::Style {
                background: None,
                border: Border::default(),
                text_color: LINK_COLOR,
                ..Default::default()
            })
            .on_press(LoginMessage::OpenExternalLink(
                "https://account.nextania.com/register".to_string()
            )),
            button(
                text("Configure custom server URL")
                    .size(12)
                    .color(LINK_COLOR)
                    .font(DM_SANS)
            )
            .padding(Padding::ZERO)
            .style(|_theme, _status| button::Style {
                background: None,
                border: Border::default(),
                text_color: LINK_COLOR,
                ..Default::default()
            })
            .on_press(LoginMessage::OpenBackend),
            button(text("Get help").size(12).color(LINK_COLOR).font(DM_SANS))
                .padding(Padding::ZERO)
                .style(|_theme, _status| button::Style {
                    background: None,
                    border: Border::default(),
                    text_color: LINK_COLOR,
                    ..Default::default()
                })
                .on_press(LoginMessage::OpenExternalLink(
                    "https://nextania.com".to_string()
                )),
        ]
        .spacing(2);

        let card = container(
            column![card_header, title_block, fields_col, more_options]
                .spacing(16)
                .width(Length::Fill),
        )
        .padding(16)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_LOGIN_CARD)),
            border: Border {
                color: BORDER_CARD,
                width: 1.0,
                radius: 10.into(),
            },
            shadow: Shadow {
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.25,
                },
                offset: Vector::new(0.0, 6.0),
                blur_radius: 4.0,
            },
            ..Default::default()
        });

        let bg_image = image(self.bg_handle.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(iced::ContentFit::Cover);

        let foreground = container(
            row![container(card).width(310), Space::new().width(Length::Fill),]
                .height(Length::Fill)
                .align_y(alignment::Vertical::Center)
                .padding(Padding::from([10, 16])),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        stack![bg_image, foreground]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

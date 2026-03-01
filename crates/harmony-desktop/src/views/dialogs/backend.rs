use iced::{
    Border, Element, Length, Padding, Task, Theme, alignment, color,
    widget::{Space, button, column, container, row, text, text_input},
};

use crate::{
    Message,
    theme::{
        ACCENT_PURPLE, BG_APP, BG_LOGIN_INPUT, BG_SELECTED, BORDER, DM_SANS, SUBTLE_GREY,
        TEXT_MUTED, TEXT_WHITE,
    },
    widgets::button::ButtonExt,
};

#[derive(Clone)]
pub enum BackendMessage {
    AccountServerChanged(String),
    HarmonyServerChanged(String),
    Save,
    Close,
}

pub struct BackendView {
    account_server: String,
    harmony_server: String,
}

impl BackendView {
    pub fn new(account_server: String, harmony_server: String) -> Self {
        Self {
            account_server,
            harmony_server,
        }
    }

    pub fn update(&mut self, message: BackendMessage) -> Task<Message> {
        match message {
            BackendMessage::AccountServerChanged(s) => {
                self.account_server = s;
            }
            BackendMessage::HarmonyServerChanged(s) => {
                self.harmony_server = s;
            }
            BackendMessage::Save => {
                return Task::done(Message::BackendChanged(
                    self.account_server.clone(),
                    self.harmony_server.clone(),
                ));
            }
            BackendMessage::Close => {
                return Task::done(Message::CloseBackend);
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<BackendMessage> {
        let title = text("Custom server URLs")
            .size(18)
            .color(TEXT_WHITE)
            .font(iced::Font {
                weight: iced::font::Weight::Bold,
                ..DM_SANS
            });

        let subtitle = text("Configure the servers that Harmony connects to.")
            .size(13)
            .color(TEXT_MUTED)
            .font(DM_SANS);

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

        let account_label = text("Account server")
            .size(12)
            .color(TEXT_MUTED)
            .font(DM_SANS);

        let account_input = text_input("https://account.nextania.com", &self.account_server)
            .on_input(BackendMessage::AccountServerChanged)
            .size(12)
            .font(DM_SANS)
            .style(input_style)
            .width(Length::Fill);

        let harmony_label = text("Harmony server")
            .size(12)
            .color(TEXT_MUTED)
            .font(DM_SANS);

        let harmony_input = text_input("https://chat.nextania.com", &self.harmony_server)
            .on_input(BackendMessage::HarmonyServerChanged)
            .on_submit(BackendMessage::Save)
            .size(12)
            .font(DM_SANS)
            .style(input_style)
            .width(Length::Fill);

        let save_btn = button(
            container(
                text("Save")
                    .size(12)
                    .color(TEXT_WHITE)
                    .font(DM_SANS)
                    .align_x(alignment::Horizontal::Center),
            )
            .center_x(Length::Fill),
        )
        .on_press(BackendMessage::Save)
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

        let cancel_btn = button(
            container(
                text("Cancel")
                    .size(12)
                    .color(TEXT_WHITE)
                    .font(DM_SANS)
                    .align_x(alignment::Horizontal::Center),
            )
            .center_x(Length::Fill),
        )
        .on_press(BackendMessage::Close)
        .padding(Padding::from([4, 8]))
        .style(|_theme, status| button::Style {
            background: Some(iced::Background::Color(match status {
                button::Status::Hovered => color!(0x3d2448),
                button::Status::Pressed => color!(0x231528),
                _ => BG_SELECTED,
            })),
            border: Border {
                color: BORDER,
                width: 1.0,
                ..Border::default().rounded(4)
            },
            text_color: TEXT_WHITE,
            ..Default::default()
        })
        .cursor_default();

        let content = column![
            column![title, subtitle].spacing(4),
            column![account_label, account_input].spacing(4),
            column![harmony_label, harmony_input].spacing(4),
            Space::new().height(Length::Fill),
            row![Space::new().width(Length::Fill), save_btn, cancel_btn].spacing(8),
        ]
        .spacing(12)
        .width(Length::Fill)
        .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::new(16.0))
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_APP)),
                ..Default::default()
            })
            .into()
    }
}

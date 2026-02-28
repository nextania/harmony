use iced::{
    Border, Color, Element, Length, Padding, Task,
    widget::{column, container, row, text},
};

use crate::{
    Message,
    theme::{BG_APP, BG_SIDEBAR, BORDER, DM_SANS, TEXT_MUTED, TEXT_PRIMARY},
};

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    Close,
}

pub struct SettingsView;

impl SettingsView {
    pub fn new() -> Self {
        Self
    }

    pub fn update(&mut self, message: SettingsMessage) -> Task<Message> {
        match message {
            SettingsMessage::Close => {}
        }
        Task::none()
    }

    pub fn view(&self) -> Element<SettingsMessage> {
        let sidebar = container(
            column![
                section_item("Account"),
                section_item("Appearance"),
                section_item("Notifications"),
                section_item("Privacy"),
                section_item("Advanced"),
            ]
            .spacing(2)
            .padding(Padding::from([8, 0])),
        )
        .width(200)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_SIDEBAR)),
            border: Border {
                color: BORDER,
                width: 0.0,
                radius: 0.into(),
            },
            ..Default::default()
        });

        let content = container(
            column![
                text("Settings").size(22).color(TEXT_PRIMARY).font(DM_SANS),
                text("Select a category on the left to configure your preferences.")
                    .size(14)
                    .color(TEXT_MUTED)
                    .font(DM_SANS),
            ]
            .spacing(12)
            .padding(Padding::from([32, 32])),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_APP)),
            ..Default::default()
        });

        row![sidebar, content]
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    }
}

fn section_item(label: &str) -> Element<'static, SettingsMessage> {
    use iced::widget::button;

    let label = label.to_string();
    button(text(label).size(14).color(TEXT_PRIMARY).font(DM_SANS))
        .width(Length::Fill)
        .padding(Padding::from([8, 16]))
        .style(|_theme, status| button::Style {
            background: Some(iced::Background::Color(match status {
                button::Status::Hovered => Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.05,
                },
                button::Status::Pressed => Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.1,
                },
                _ => Color::TRANSPARENT,
            })),
            border: Border::default().rounded(5),
            text_color: TEXT_PRIMARY,
            ..Default::default()
        })
        .into()
}

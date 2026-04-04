use iced::{
    Border, Element, Length, Padding, Task,
    border::Radius,
    widget::{Space, button, column, container, row, text},
};

use crate::{Message, widgets::styles};
use crate::{
    theme::{BG_APP, BG_SUNKEN, BORDER, DM_SANS, TEXT_MUTED},
    widgets::button::ButtonExt,
};

#[derive(Clone)]
pub enum ExternalLinkMessage {
    Open,
    Close,
}

pub struct ExternalLinkView {
    url: String,
}

impl ExternalLinkView {
    pub fn new(url: String) -> Self {
        Self { url }
    }

    pub fn update(&mut self, message: ExternalLinkMessage) -> Task<Message> {
        match message {
            ExternalLinkMessage::Close => Task::done(Message::CloseExternalLink),
            ExternalLinkMessage::Open => {
                let url = self.url.clone();
                Task::perform(
                    async move {
                        let _ = open::that(&url);
                    },
                    |_| Message::CloseExternalLink,
                )
            }
        }
    }

    pub fn view(&self) -> Element<ExternalLinkMessage> {
        container(
            column![
                column![
                    text("You are about to open the following URL in your default browser:")
                        .size(14)
                        .font(DM_SANS),
                    container(text(&self.url).size(14).font(DM_SANS))
                        .style(|_theme| container::Style {
                            background: Some(BG_SUNKEN.into()),
                            border: Border {
                                color: BORDER,
                                width: 1.0,
                                radius: Radius::new(4),
                            },
                            text_color: Some(TEXT_MUTED),
                            ..Default::default()
                        })
                        .padding(Padding::new(6.0))
                        .width(Length::Fill),
                    text(
                        "If you would like to continue, please click the button below to proceed."
                    )
                    .size(14)
                    .font(DM_SANS),
                ]
                .width(Length::Fill)
                .spacing(8),
                Space::new().height(Length::Fill),
                row![
                    Space::new().width(Length::Fill),
                    button(text("Open link").font(DM_SANS).size(12))
                        .on_press(ExternalLinkMessage::Open)
                        .padding(Padding::from([4, 8]))
                        .style(styles::primary)
                        .cursor_default(),
                    button(text("Cancel").font(DM_SANS).size(12))
                        .on_press(ExternalLinkMessage::Close)
                        .padding(Padding::from([4, 8]))
                        .style(styles::secondary)
                        .cursor_default(),
                ]
                .spacing(8)
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding::new(10.0))
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_APP)),
            ..Default::default()
        })
        .into()
    }
}

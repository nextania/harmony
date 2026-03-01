use async_stream::stream;
use iced::{
    Border, Element, Font, Length, Padding, Task, Theme, alignment, color,
    widget::{Space, button, column, container, text, text_input},
};

use crate::{
    Message,
    api::live::LiveApiClient,
    errors::RenderableError,
    theme::{ACCENT_PURPLE, BG_APP, BG_LOGIN_INPUT, DM_SANS, SUBTLE_GREY, TEXT_WHITE},
    views::main::MainMessage,
    widgets::button::ButtonExt,
};

#[derive(Clone)]
pub enum TokenMessage {
    TokenChanged(String),
    Submit,
    Failed(RenderableError),
}

pub struct TokenView {
    token: String,
    error: Option<String>,
    backend_harmony: String,
}

impl TokenView {
    pub fn new(backend_harmony: String) -> Self {
        Self {
            token: String::new(),
            error: None,
            backend_harmony,
        }
    }

    pub fn update(&mut self, message: TokenMessage) -> Task<Message> {
        match message {
            TokenMessage::TokenChanged(s) => {
                self.error = None;
                self.token = s;
            }
            TokenMessage::Submit => {
                if self.token.is_empty() {
                    return Task::none();
                }
                let token = self.token.clone();
                let backend_harmony = self.backend_harmony.clone();
                return Task::stream(stream! {
                    let result = async {
                        let (client, stream) = LiveApiClient::connect(&backend_harmony, &token).await?;
                        let current_user = client.get_current_user().await?;
                        let conversations = client.get_conversations().await?
                            .into_iter().map(|c| (c.id(), c)).collect();
                        Ok::<_, RenderableError>((client, current_user, conversations, stream))
                    }.await;
                    match result {
                        Ok((client, user, convs, mut stream)) => {
                            yield Message::LoginFinished((client, user, convs));
                            while let Some(event) = stream.recv().await {
                                yield Message::Main(MainMessage::ServerEvent(event));
                            }
                        }
                        Err(e) => yield Message::Token(TokenMessage::Failed(e)),
                    }
                });
            }
            TokenMessage::Failed(e) => {
                self.error = Some(e.to_string());
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<TokenMessage> {
        let title = text("Sign in with token")
            .size(20)
            .color(TEXT_WHITE)
            .font(Font {
                weight: iced::font::Weight::Bold,
                ..DM_SANS
            });

        let subtitle = text("Paste your authentication token below to sign in.")
            .size(13)
            .color(SUBTLE_GREY)
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

        let token_input = text_input("Token", &self.token)
            .on_input(TokenMessage::TokenChanged)
            .on_submit(TokenMessage::Submit)
            .secure(true)
            .size(14)
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
        .on_press(TokenMessage::Submit)
        .width(Length::Fill)
        .padding(Padding::from([6, 8]))
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

        let mut content = column![
            column![title, subtitle, token_input].spacing(12),
            Space::new().height(Length::Fill),
            sign_in_btn,
        ]
        .width(Length::Fill);

        if let Some(err) = &self.error {
            content = content.push(
                text(err.as_str())
                    .size(12)
                    .color(iced::color!(0xff4444))
                    .font(DM_SANS),
            );
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(24)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_APP)),
                ..Default::default()
            })
            .into()
    }
}

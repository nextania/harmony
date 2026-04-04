use async_stream::stream;
use iced::{
    Border, Element, Font, Length, Padding, Task, Theme, alignment, color,
    widget::{Space, button, column, container, text, text_input},
};

use crate::{
    Message,
    api::{account, ApiClient},
    errors::RenderableError,
    theme::{ACCENT_PURPLE, BG_APP, BG_LOGIN_INPUT, DM_SANS, SUBTLE_GREY, TEXT_WHITE},
    views::main::MainMessage,
    widgets::{button::ButtonExt, styles},
};

#[derive(Clone)]
pub enum MfaMessage {
    CodeChanged(String),
    Submit,
    Failed(RenderableError),
}

pub struct MfaView {
    mfa: Option<account::LoginMfa>,
    code: String,
    error: Option<String>,
    backend_account: String,
    backend_harmony: String,
    password: String,
}

impl MfaView {
    pub fn new(mfa: account::LoginMfa, backend_account: String, backend_harmony: String, password: String) -> Self {
        Self {
            mfa: Some(mfa),
            code: String::new(),
            error: None,
            backend_account,
            backend_harmony,
            password,
        }
    }

    pub fn update(&mut self, message: MfaMessage) -> Task<Message> {
        match message {
            MfaMessage::CodeChanged(s) => {
                // accept digits only, max 8 characters.
                let filtered: String = s.chars().filter(|c| c.is_ascii_digit()).take(8).collect();
                self.error = None;
                self.code = filtered;
            }
            MfaMessage::Submit => {
                if self.code.len() != 8 {
                    return Task::none();
                }
                if let Some(mfa) = self.mfa.as_ref() {
                    let mfa = mfa.clone();
                    let code = self.code.clone();
                    let backend_account = self.backend_account.clone();
                    let backend_harmony = self.backend_harmony.clone();
                    let password = self.password.clone();
                    return Task::stream(stream! {
                        let result = async {
                            let (token, encrypted_key) = mfa.code(&code).await?;
                            let (client, stream) = ApiClient::connect(&backend_account, &backend_harmony, &token, &encrypted_key, &password).await?;
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
                            Err(e) => yield Message::Mfa(MfaMessage::Failed(e)),
                        }
                    });
                }
            }
            MfaMessage::Failed(e) => {
                self.error = Some(e.to_string());
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<MfaMessage> {
        let title = text("Two-factor authentication")
            .size(20)
            .color(TEXT_WHITE)
            .font(Font {
                weight: iced::font::Weight::Bold,
                ..DM_SANS
            });

        let subtitle = text("Enter the 8-digit code from your authenticator app.")
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

        let code_input = text_input("00000000", &self.code)
            .on_input(MfaMessage::CodeChanged)
            .on_submit(MfaMessage::Submit)
            .size(20)
            .font(DM_SANS)
            .style(input_style)
            .width(Length::Fill);

        let verify_btn = button(
            container(
                text("Verify")
                    .size(12)
                    .color(TEXT_WHITE)
                    .font(DM_SANS)
                    .align_x(alignment::Horizontal::Center),
            )
            .center_x(Length::Fill),
        )
        .on_press(MfaMessage::Submit)
        .width(Length::Fill)
        .padding(Padding::from([6, 8]))
        .style(styles::primary)
        .cursor_default();

        let mut content = column![
            column![title, subtitle, code_input].spacing(12),
            Space::new().height(Length::Fill),
            verify_btn
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

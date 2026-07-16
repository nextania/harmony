use iced::{
    Border, Color, Element, Font, Length, Padding, alignment,
    widget::{button, container, image, row, text, text_input},
};

use crate::{
    icons::{FLUENT_ICONS, Icon},
    theme::{ACCENT_PURPLE, BG_PANEL, BG_SUNKEN, BORDER, DM_SANS, TEXT_PLACEHOLDER, TEXT_PRIMARY},
    views::main::{ChatMode, MainMessage, MainView},
    widgets::{button::ButtonExt, styles},
};

pub fn top_bar(state: &MainView) -> Element<MainMessage> {
    let (avatar, name) = match state
        .current_channels
        .get(
            state
                .current_conversation
                .as_ref()
                .expect("This should be defined"),
        )
        .expect("This should be defined")
        .data()
    {
        harmony_api::ChannelData::PrivateChannel {
            initiator_id,
            target_id,
            ..
        } => {
            let other_id = if *initiator_id == state.current_user_id {
                target_id
            } else {
                initiator_id
            };
            state
                .api
                .users()
                .get(other_id)
                .map_or((None, "Unknown".to_string()), |x| {
                    (x.avatar().cloned(), x.display_name().to_string())
                })
        }
        harmony_api::ChannelData::GroupChannel { .. } => (None, "Unnamed Group".to_string()),
    };

    let avatar = container(image(state.default_avatar.clone()))
        .width(24)
        .height(24)
        .style(move |_theme| container::Style {
            border: Border::default().rounded(6),
            ..Default::default()
        });
    let user_name = text(name).size(16).color(TEXT_PRIMARY).font(Font {
        weight: iced::font::Weight::Bold,
        ..DM_SANS
    });
    let channel_desc = row![avatar, user_name]
        .spacing(12)
        .align_y(alignment::Vertical::Center);

    let text_active = matches!(state.chat_mode, ChatMode::Text);
    let voice_active = matches!(state.chat_mode, ChatMode::Voice);

    let text_btn = button(
        row![
            text(if text_active {
                Icon::ChatFilled.unicode()
            } else {
                Icon::ChatRegular.unicode()
            })
            .size(20)
            .font(FLUENT_ICONS),
            text("Text")
                .size(14)
                .color(TEXT_PRIMARY)
                .font(if text_active {
                    Font {
                        weight: iced::font::Weight::Bold,
                        ..DM_SANS
                    }
                } else {
                    DM_SANS
                })
        ]
        .spacing(4)
        .align_y(alignment::Vertical::Center),
    )
    .padding(Padding::from([2, 8]))
    .on_press(MainMessage::ChatModeSelected(ChatMode::Text))
    .style(styles::tab_mode(text_active))
    .cursor_default();

    let voice_btn = button(
        row![
            text(if voice_active {
                Icon::Speaker2Filled.unicode()
            } else {
                Icon::Speaker2Regular.unicode()
            })
            .size(20)
            .font(FLUENT_ICONS),
            text("Voice")
                .size(14)
                .color(TEXT_PRIMARY)
                .font(if voice_active {
                    Font {
                        weight: iced::font::Weight::Bold,
                        ..DM_SANS
                    }
                } else {
                    DM_SANS
                })
        ]
        .spacing(4)
        .align_y(alignment::Vertical::Center),
    )
    .padding(Padding::from([2, 8]))
    .on_press(MainMessage::ChatModeSelected(ChatMode::Voice))
    .style(styles::tab_mode(voice_active))
    .cursor_default();

    let mode_selector = container(row![text_btn, voice_btn].spacing(4))
        .padding(4)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_SUNKEN)),
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: 4.into(),
            },
            ..Default::default()
        });

    let search = container(
        text_input("Search this chat...", &state.search_input)
            .on_input(MainMessage::SearchInputChanged)
            .size(14)
            .font(DM_SANS)
            .style(|_theme, _status| text_input::Style {
                background: iced::Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                icon: TEXT_PLACEHOLDER,
                placeholder: TEXT_PLACEHOLDER,
                value: TEXT_PRIMARY,
                selection: ACCENT_PURPLE,
            }),
    )
    .width(230)
    .padding(Padding::from([2, 4]))
    .style(|_theme| container::Style {
        background: Some(iced::Background::Color(BG_SUNKEN)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 5.into(),
        },
        ..Default::default()
    });

    container(
        row![
            container(channel_desc).width(Length::Fill),
            mode_selector,
            container(search)
                .padding(Padding {
                    left: 8.0,
                    ..Default::default()
                })
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Right),
        ]
        .align_y(alignment::Vertical::Center)
        .padding(Padding::from([8, 16]))
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(iced::Background::Color(BG_PANEL)),
        ..Default::default()
    })
    .into()
}

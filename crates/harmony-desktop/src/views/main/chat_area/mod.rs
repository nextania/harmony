use iced::{
    Border, Element, Length, Padding,
    widget::{button, column, container, row, text},
};

use crate::{
    icons::{FLUENT_ICONS, Icon},
    theme::{ACCENT_PURPLE, BG_SCREENSHARE_PANEL, DM_SANS, TEXT_PRIMARY},
    views::main::{
        ChatMode, MainMessage, MainView,
        chat_area::{input::chat_frame, messages::main_chat, voice::voice_area},
    },
    widgets::{button::ButtonExt, styles},
};

pub mod input;
pub mod messages;
pub mod top_bar;
pub mod voice;

fn screenshare_banner(state: &MainView) -> Option<Element<MainMessage>> {
    if state.is_local_screensharing() {
        Some(
            container(
                row![
                    text(Icon::ShareScreenPersonFilled.unicode())
                        .size(16)
                        .color(TEXT_PRIMARY)
                        .font(FLUENT_ICONS),
                    text("You're sharing your screen")
                        .size(13)
                        .color(TEXT_PRIMARY)
                        .font(DM_SANS),
                    iced::widget::Space::new().width(Length::Fill),
                    button(
                        container(
                            text("Stop sharing")
                                .size(12)
                                .color(TEXT_PRIMARY)
                                .font(DM_SANS),
                        )
                        .padding(Padding::from([2, 8])),
                    )
                    .on_press(MainMessage::ToggleScreenShare)
                    .style(styles::accent_dim)
                    .cursor_default(),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .padding(Padding::from([6, 12]))
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_SCREENSHARE_PANEL)),
                border: Border::default().rounded(0),
                ..Default::default()
            })
            .into(),
        )
    } else if let Some(participant) = state.remote_screenshare_available() {
        let is_consuming = state.is_consuming_remote_screenshare();
        let name = &participant.profile.display_name;
        let label = if is_consuming {
            format!("Viewing {name}'s screen")
        } else {
            format!("{name} is sharing their screen")
        };

        let view_btn: Option<Element<MainMessage>> = if !is_consuming {
            state.pending_screen_track_id().map(|track_id| {
                button(
                    container(text("View").size(12).color(TEXT_PRIMARY).font(DM_SANS))
                        .padding(Padding::from([2, 8])),
                )
                .on_press(MainMessage::ConsumeScreenTrack(track_id.to_string()))
                .style(styles::accent_dim)
                .cursor_default()
                .into()
            })
        } else {
            Some(
                button(
                    container(
                        text("Stop viewing")
                            .size(12)
                            .color(TEXT_PRIMARY)
                            .font(DM_SANS),
                    )
                    .padding(Padding::from([2, 8])),
                )
                .on_press(MainMessage::StopViewingScreenTrack)
                .style(styles::accent_dim)
                .cursor_default()
                .into(),
            )
        };

        let mut banner_row = row![
            text(Icon::ShareScreenPersonFilled.unicode())
                .size(16)
                .color(TEXT_PRIMARY)
                .font(FLUENT_ICONS),
            text(label).size(13).color(TEXT_PRIMARY).font(DM_SANS),
            iced::widget::Space::new().width(Length::Fill),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        if let Some(btn) = view_btn {
            banner_row = banner_row.push(btn);
        }

        Some(
            container(banner_row)
                .width(Length::Fill)
                .padding(Padding::from([6, 12]))
                .style(|_theme| container::Style {
                    background: Some(iced::Background::Color(ACCENT_PURPLE)),
                    border: Border::default().rounded(0),
                    ..Default::default()
                })
                .into(),
        )
    } else {
        None
    }
}

pub fn chat_area(state: &MainView) -> Element<MainMessage> {
    let top_bar = top_bar::top_bar(state);
    let banner = screenshare_banner(state);
    let body: Element<MainMessage> = match state.chat_mode {
        ChatMode::Voice => voice_area(state),
        ChatMode::Text => {
            let main_chat = main_chat(state);
            let chat_frame = chat_frame(state);
            column![main_chat, chat_frame]
                .width(Length::Fill)
                .height(Length::Fill)
                .spacing(0)
                .into()
        }
    };
    let mut content = column![top_bar];
    if let Some(b) = banner {
        content = content.push(b);
    }
    content = content.push(body);
    content
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(0)
        .into()
}

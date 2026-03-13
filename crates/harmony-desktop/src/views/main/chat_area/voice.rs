use iced::{
    Border, Color, Element, Length, Padding, Shadow, Vector, alignment, color,
    widget::{Column, Space, button, column, container, row, text},
};

use crate::{
    api::CallState,
    icons::{FLUENT_ICONS, Icon},
    theme::{
        ACCENT_PURPLE, BG_APP, BG_CTRL_INACTIVE, BG_PARTICIPANT_CARD, BG_PARTICIPANT_LABEL,
        BG_SCREENSHARE_PANEL, DANGER_RED, DM_SANS, OVERLAY, TEXT_MUTED, TEXT_PRIMARY,
    },
    views::main::{MainMessage, MainView},
    widgets::{button::ButtonExt, styles},
};

pub fn voice_area(state: &MainView) -> Element<MainMessage> {
    let current_user_id = &state.current_user.profile.id;

    match &state.current_call_state {
        None => {
            let start_btn = button(
                container(
                    text("Start call")
                        .size(14)
                        .color(TEXT_PRIMARY)
                        .font(DM_SANS)
                        .align_x(alignment::Horizontal::Center),
                )
                .center_x(Length::Fill),
            )
            .width(Length::Shrink)
            .on_press(MainMessage::StartCall)
            .padding(Padding::from([6, 12]))
            .style(styles::accent_dim)
            .cursor_default();

            let label = text("No active call in this channel")
                .size(16)
                .color(TEXT_MUTED)
                .font(DM_SANS);

            container(
                column![label, start_btn]
                    .spacing(16)
                    .align_x(alignment::Horizontal::Center),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_APP)),
                ..Default::default()
            })
            .into()
        }
        Some(call) => {
            let user_in_call = call
                .participants
                .iter()
                .any(|p| p.profile.id == *current_user_id);

            if !user_in_call {
                let names: Vec<String> = call
                    .participants
                    .iter()
                    .map(|p| p.profile.display_name.clone())
                    .collect();
                let in_call_label = text(format!(
                    "In call: {}",
                    if names.is_empty() {
                        "nobody".to_string()
                    } else {
                        names.join(", ")
                    }
                ))
                .size(16)
                .color(TEXT_MUTED)
                .font(DM_SANS);

                let join_btn = button(
                    container(
                        text("Join call")
                            .size(14)
                            .color(TEXT_PRIMARY)
                            .font(DM_SANS)
                            .align_x(alignment::Horizontal::Center),
                    )
                    .center_x(Length::Fill),
                )
                .width(Length::Shrink)
                .on_press(MainMessage::JoinCall)
                .padding(Padding::from([6, 12]))
                .style(styles::accent_dim)
                .cursor_default();

                container(
                    column![in_call_label, join_btn]
                        .spacing(16)
                        .align_x(alignment::Horizontal::Center),
                )
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(|_theme| container::Style {
                    background: Some(iced::Background::Color(BG_APP)),
                    ..Default::default()
                })
                .into()
            } else {
                voice_in_call(state, call)
            }
        }
    }
}

fn voice_in_call<'a>(state: &'a MainView, call: &'a CallState) -> Element<'a, MainMessage> {
    let current_user_id = &state.current_user.profile.id;

    let my_tracks = call
        .participants
        .iter()
        .find(|p| p.profile.id == *current_user_id)
        .map(|p| &p.tracks);

    let mic_active = my_tracks.map_or(false, |t| t.audio);
    let cam_active = my_tracks.map_or(false, |t| t.video);
    let screen_active = my_tracks.map_or(false, |t| t.screen);

    let mut content = vec![];
    let screen_sharer = call
        .participants
        .iter()
        .find(|p| p.tracks.screen)
        .map(|p| p.profile.display_name.as_str());

    if let Some(sharer_name) = screen_sharer {
        let screen_label = container(
            row![
                text(Icon::ShareScreenPersonFilled.unicode())
                    .size(18)
                    .color(TEXT_PRIMARY)
                    .font(FLUENT_ICONS),
                text(sharer_name.to_string())
                    .size(14)
                    .color(TEXT_PRIMARY)
                    .font(DM_SANS)
            ]
            .spacing(8)
            .align_y(alignment::Vertical::Center),
        )
        .padding(Padding::from([4, 10]))
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(OVERLAY)),
            border: Border {
                color: color!(0x9d9d9d),
                width: 1.0,
                radius: 5.into(),
            },
            ..Default::default()
        });

        let fullscreen_btn = container(
            text(Icon::FullScreenMaximizeRegular.unicode())
                .size(18)
                .color(TEXT_PRIMARY)
                .font(FLUENT_ICONS),
        )
        .center(32)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(OVERLAY)),
            border: Border {
                color: color!(0x9d9d9d),
                width: 1.0,
                radius: 5.into(),
            },
            ..Default::default()
        });

        let panel_bottom = row![
            screen_label,
            Space::new().width(Length::Fill),
            fullscreen_btn,
        ]
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill);

        content.push(
            container(
                column![Space::new().height(Length::Fill), panel_bottom]
                    .width(Length::Fill)
                    .padding(10),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_SCREENSHARE_PANEL)),
                border: Border::default().rounded(10),
                ..Default::default()
            })
            .into(),
        );
    } else {
        content.push(Space::new().height(Length::Fill).into());
    };

    let participant_card = |name: &str, avatar_color: Color, audio: bool| -> Element<MainMessage> {
        let avatar = container(text("").size(1))
            .width(64)
            .height(64)
            .style(move |_theme| container::Style {
                background: Some(iced::Background::Color(avatar_color)),
                border: Border::default().rounded(10),
                ..Default::default()
            });

        let mic_indicator = text(if audio {
            Icon::MicFilled.unicode()
        } else {
            Icon::MicOffRegular.unicode()
        })
        .size(14)
        .color(if audio { TEXT_PRIMARY } else { TEXT_MUTED })
        .font(FLUENT_ICONS);

        let name_label = container(
            row![
                text(name.to_string())
                    .size(16)
                    .color(TEXT_PRIMARY)
                    .font(DM_SANS)
                    .align_x(alignment::Horizontal::Center),
                mic_indicator,
            ]
            .spacing(4)
            .align_y(alignment::Vertical::Center),
        )
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .padding(Padding::from([5, 15]))
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_PARTICIPANT_LABEL)),
            border: Border::default().rounded(5),
            ..Default::default()
        });

        container(
            column![avatar, name_label]
                .spacing(16)
                .align_x(alignment::Horizontal::Center)
                .width(Length::Fill),
        )
        .width(200)
        .padding(Padding::from([10, 13]))
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_PARTICIPANT_CARD)),
            border: Border::default().rounded(5),
            shadow: Shadow {
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.25,
                },
                offset: Vector::new(0.0, 4.0),
                blur_radius: 4.0,
            },
            ..Default::default()
        })
        .into()
    };

    let mut participants_row_content = row![].spacing(10);
    for participant in call.participants.iter() {
        participants_row_content = participants_row_content.push(participant_card(
            &participant.profile.display_name,
            participant.profile.avatar_color_start,
            participant.tracks.audio,
        ));
    }
    let participants_row = container(participants_row_content)
        .center_x(Length::Fill)
        .into();
    content.push(participants_row);
    if screen_sharer.is_none() {
        content.push(Space::new().height(Length::Fill).into());
    }

    let ctrl_btn = |icon: char, bg: Color, msg: MainMessage| -> Element<MainMessage> {
        button(
            container(text(icon).size(24).color(TEXT_PRIMARY).font(FLUENT_ICONS))
                .center_x(48)
                .center_y(48),
        )
        .on_press(msg)
        .padding(0)
        .style(styles::call_ctrl(bg))
        .cursor_default()
        .into()
    };

    let mic_icon = if mic_active {
        Icon::MicFilled.unicode()
    } else {
        Icon::MicOffRegular.unicode()
    };
    let mic_bg = if mic_active {
        ACCENT_PURPLE
    } else {
        BG_CTRL_INACTIVE
    };

    let cam_icon = if cam_active {
        Icon::CameraFilled.unicode()
    } else {
        Icon::CameraOffRegular.unicode()
    };
    let cam_bg = if cam_active {
        ACCENT_PURPLE
    } else {
        BG_CTRL_INACTIVE
    };

    let screen_icon = if screen_active {
        Icon::ShareScreenStopFilled.unicode()
    } else {
        Icon::ShareScreenStartRegular.unicode()
    };
    let screen_bg = if screen_active {
        ACCENT_PURPLE
    } else {
        BG_CTRL_INACTIVE
    };

    let controls = container(
        row![
            ctrl_btn(cam_icon, cam_bg, MainMessage::ToggleCamera),
            ctrl_btn(mic_icon, mic_bg, MainMessage::ToggleMic),
            ctrl_btn(screen_icon, screen_bg, MainMessage::ToggleScreenShare),
            ctrl_btn(
                Icon::CallEndFilled.unicode(),
                DANGER_RED,
                MainMessage::LeaveCall
            ),
        ]
        .spacing(8),
    )
    .center_x(Length::Fill)
    .into();
    content.push(controls);

    let content = Column::from_vec(content)
        .spacing(16)
        .padding(Padding::from([16, 12]))
        .width(Length::Fill)
        .height(Length::Fill);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_APP)),
            ..Default::default()
        })
        .into()
}

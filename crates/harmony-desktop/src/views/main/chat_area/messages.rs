use chrono::{DateTime, Utc};
use iced::{
    Border, Color, Element, Font, Length, Padding, Shadow, Vector, alignment,
    widget::{Column, Space, button, column, container, row, scrollable, text},
};

use crate::{
    MessageContent,
    theme::{ACCENT_PURPLE_DIM, BG_APP, BG_CALL_CARD, BORDER, DM_SANS, TEXT_MUTED, TEXT_PRIMARY},
    views::main::{MainMessage, MainView},
    widgets::button::ButtonExt,
};

const GROUP_TIME_LIMIT_MINUTES: u32 = 5;

pub fn main_chat(state: &MainView) -> Element<MainMessage> {
    let mut messages_col = Column::new().spacing(4).width(Length::Fill);
    // TODO: if this is the first message in the channel, show a beginning text

    let mut group_user: Option<String> = None;
    let mut group_start_minutes: Option<DateTime<Utc>> = None;
    let mut first_group = true;

    for msg in state.current_conversation_messages.iter() {
        let msg_ts =
            DateTime::<Utc>::from_timestamp_millis(msg.time).expect("Invalid timestamp in message");

        // continue current group when the author is the same
        // and timestamp is within GROUP_TIME_LIMIT_MINUTES of
        // first message; call cards always start a new group
        let is_continuation = !matches!(&msg.content, MessageContent::CallCard { .. })
            && match (group_user.as_ref(), group_start_minutes) {
                (Some(gu), Some(gs)) => {
                    *gu == msg.user.id()
                        && msg_ts.signed_duration_since(gs).num_minutes()
                            <= GROUP_TIME_LIMIT_MINUTES as i64
                }
                _ => false,
            };

        if !is_continuation {
            if !first_group {
                messages_col = messages_col.push(Space::new().height(11));
            }
            first_group = false;
            group_user = Some(msg.user.id());
            group_start_minutes = Some(msg_ts);
        }

        let build_call_card = |channel: String, duration: String| -> Element<MainMessage> {
            let call_title = text("Call started")
                .size(18)
                .color(TEXT_PRIMARY)
                .font(Font {
                    weight: iced::font::Weight::Bold,
                    ..DM_SANS
                });
            let call_channel = text(channel).size(14).color(TEXT_PRIMARY).font(DM_SANS);
            let call_duration = text(duration).size(14).color(TEXT_PRIMARY).font(DM_SANS);
            let call_info = column![call_title, column![call_channel, call_duration]].spacing(16);

            let p1 = container(text("").size(1))
                .width(32)
                .height(32)
                .style(|_theme| container::Style {
                    background: Some(iced::Background::Color(Color {
                        r: 0xaa as f32 / 255.0,
                        g: 0xaa as f32 / 255.0,
                        b: 0xaa as f32 / 255.0,
                        a: 1.0,
                    })),
                    border: Border::default().rounded(4),
                    ..Default::default()
                });
            let p2 = container(text("").size(1))
                .width(32)
                .height(32)
                .style(|_theme| container::Style {
                    background: Some(iced::Background::Color(Color {
                        r: 0xc4 as f32 / 255.0,
                        g: 0xc4 as f32 / 255.0,
                        b: 0xc4 as f32 / 255.0,
                        a: 1.0,
                    })),
                    border: Border::default().rounded(4),
                    ..Default::default()
                });
            let participants = row![p1, p2].spacing(2);

            let join_btn = button(
                container(
                    text("Join")
                        .size(14)
                        .color(TEXT_PRIMARY)
                        .font(DM_SANS)
                        .align_x(alignment::Horizontal::Center),
                )
                .center_x(Length::Fill),
            )
            .width(Length::Shrink)
            .on_press(MainMessage::JoinCall)
            .padding(Padding::from([4, 24]))
            .style(|_theme, _status| button::Style {
                background: Some(iced::Background::Color(ACCENT_PURPLE_DIM)),
                border: Border::default().rounded(5),
                text_color: TEXT_PRIMARY,
                shadow: Shadow {
                    color: Color {
                        r: 106.0 / 255.0,
                        g: 0.0,
                        b: 155.0 / 255.0,
                        a: 0.25,
                    },
                    offset: Vector::new(0.0, 4.0),
                    blur_radius: 4.0,
                },
                ..Default::default()
            })
            .cursor_default();

            let right_side = column![participants, join_btn]
                .spacing(16)
                .align_x(alignment::Horizontal::Right);

            container(
                row![
                    call_info,
                    iced::widget::Space::new().width(Length::Fill),
                    right_side,
                ]
                .align_y(alignment::Vertical::Center)
                .width(Length::Fill),
            )
            .width(350)
            .padding(Padding::from([16, 12]))
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(BG_CALL_CARD)),
                border: Border {
                    color: BORDER,
                    width: 1.0,
                    radius: 10.into(),
                },
                ..Default::default()
            })
            .into()
        };

        let msg_row: Element<MainMessage> = if is_continuation {
            let body: Element<MainMessage> = match &msg.content {
                MessageContent::Text(t) => {
                    text(t).size(16).color(TEXT_PRIMARY).font(DM_SANS).into()
                }
                MessageContent::CallCard { channel, duration } => {
                    build_call_card(channel.clone(), duration.clone())
                }
            };
            // 40 (avatar) + 12 (spacing) left offset to align with content
            row![Space::new().width(52), body]
                .align_y(alignment::Vertical::Top)
                .into()
        } else {
            let avatar_color = msg.user.avatar_color();
            let avatar = container(text("").size(1))
                .width(40)
                .height(40)
                .style(move |_theme| container::Style {
                    background: Some(iced::Background::Color(avatar_color)),
                    border: Border::default().rounded(10),
                    ..Default::default()
                });

            let username = text(msg.user.name())
                .size(16)
                .color(TEXT_PRIMARY)
                .font(Font {
                    weight: iced::font::Weight::Bold,
                    ..DM_SANS
                });
            // HH:MM format for message timestamps
            // if timestamp was before today, show the date as well
            let time_label = text(&msg.formatted_time)
                .size(12)
                .color(TEXT_MUTED)
                .font(DM_SANS);
            let header = row![username, time_label]
                .spacing(8)
                .align_y(alignment::Vertical::Center);

            let content_widget: Element<MainMessage> = match &msg.content {
                MessageContent::Text(t) => {
                    let msg_text = text(t).size(16).color(TEXT_PRIMARY).font(DM_SANS);
                    column![header, msg_text].spacing(4).into()
                }
                MessageContent::CallCard { channel, duration } => {
                    let card = build_call_card(channel.clone(), duration.clone());
                    column![header, card].spacing(8).into()
                }
            };

            row![avatar, content_widget]
                .spacing(12)
                .align_y(alignment::Vertical::Top)
                .into()
        };

        messages_col = messages_col.push(msg_row);
    }

    let chat_content = container(messages_col)
        .width(Length::Fill)
        .padding(Padding::from([0, 12]));

    container(
        column![
            Space::new().height(Length::Fill),
            scrollable(chat_content).anchor_bottom(),
        ]
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(Padding::ZERO.vertical(12))
    .style(|_theme| container::Style {
        background: Some(iced::Background::Color(BG_APP)),
        ..Default::default()
    })
    .into()
}

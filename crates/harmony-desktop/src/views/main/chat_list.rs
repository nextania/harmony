use iced::{
    Border, Element, Font, Length, Padding, alignment,
    widget::{Column, Row, Space, button, column, container, image, text},
};

use crate::{
    icons::{FLUENT_ICONS, Icon},
    theme::{ACCENT_PURPLE, BG_PANEL, DM_SANS, TEXT_PRIMARY, TEXT_WHITE},
    views::main::{MainMessage, MainView},
    widgets::{button::ButtonExt, styles},
};

pub fn chat_list(state: &MainView) -> Element<MainMessage> {
    let title = text("Messages").size(20).color(TEXT_WHITE).font(Font {
        weight: iced::font::Weight::Bold,
        ..DM_SANS
    });

    let mut chat_items = Column::new().width(Length::Fill);

    for (id, conv) in state.current_channels.iter() {
        let is_selected = state
            .current_conversation
            .as_ref()
            .is_some_and(|selected| selected == id);
        let (channel_name, channel_icon) = match conv.data() {
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
                let (avatar_url, display_name) = state
                    .api
                    .users()
                    .get(other_id)
                    .map_or((None, "Unknown".to_string()), |x| {
                        (x.avatar().cloned(), x.display_name().to_string())
                    });
                let avatar = container(image(state.default_avatar.clone()))
                    .width(30)
                    .height(30)
                    .style(move |_theme| container::Style {
                        border: Border::default().rounded(8),
                        ..Default::default()
                    });
                (display_name, avatar)
            }
            harmony_api::ChannelData::GroupChannel { .. } => {
                todo!("Group chat icons not implemented yet");
            }
        };

        let is_call_with_screenshare =
            state.call.channel_id.as_ref() == Some(id) && state.has_active_screenshare();
        let screenshare_indicator: Option<Element<MainMessage>> = if is_call_with_screenshare {
            Some(
                text(Icon::ShareScreenPersonFilled.unicode())
                    .size(12)
                    .color(ACCENT_PURPLE)
                    .font(FLUENT_ICONS)
                    .into(),
            )
        } else {
            None
        };

        let name = text(channel_name).size(16).color(TEXT_PRIMARY).font(Font {
            weight: if is_selected {
                iced::font::Weight::Bold
            } else {
                iced::font::Weight::Medium
            },
            ..DM_SANS
        });

        let mut user_row_items = vec![channel_icon.into(), name.into()];
        if let Some(indicator) = screenshare_indicator {
            user_row_items.push(Space::new().width(Length::Fill).into());
            user_row_items.push(indicator);
        }

        let user_row = Row::from_vec(user_row_items)
            .spacing(12)
            .align_y(alignment::Vertical::Center);

        let chat_btn = button(container(user_row).padding(Padding::from([4, 0])))
            .on_press(MainMessage::ChatSelected(id.clone()))
            .width(Length::Fill)
            .style(styles::chat_item(is_selected))
            .cursor_default();

        chat_items = chat_items.push(chat_btn);
    }

    let content = column![title, chat_items].spacing(24).width(Length::Fill);

    container(content)
        .width(280)
        .height(Length::Fill)
        .padding(Padding {
            top: 16.0,
            right: 16.0,
            bottom: 16.0,
            left: 16.0,
        })
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_PANEL)),
            ..Default::default()
        })
        .into()
}

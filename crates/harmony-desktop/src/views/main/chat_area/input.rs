use iced::{
    Border, Color, Element, Font, Length, Padding, alignment,
    widget::{Column, button, column, container, row, scrollable, text, text_input},
};
use iced_aw::{DropDown, drop_down::Alignment};

use crate::{
    icons::{FLUENT_ICONS, Icon},
    theme::{
        ACCENT_PURPLE, BG_APP, BG_CHAT_BOX, BG_HOVER, BG_SELECTED, BG_SUNKEN, BORDER, DM_SANS,
        TEXT_MUTED, TEXT_PLACEHOLDER, TEXT_PRIMARY,
    },
    views::main::{MainMessage, MainView},
    widgets::{button::ButtonExt, styles},
};

pub fn chat_frame(state: &MainView) -> Element<MainMessage> {
    let input = text_input("Type a message...", &state.chat_input)
        .on_input(MainMessage::ChatInputChanged)
        .on_submit(MainMessage::SendMessage)
        .size(16)
        .font(DM_SANS)
        .style(|_theme, _status| text_input::Style {
            background: iced::Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            icon: TEXT_PLACEHOLDER,
            placeholder: TEXT_PLACEHOLDER,
            value: TEXT_PRIMARY,
            selection: ACCENT_PURPLE,
        });

    let attach_icon = button(
        text(Icon::AttachRegular.unicode())
            .size(24)
            .font(FLUENT_ICONS),
    )
    .style(styles::icon_accent)
    .padding(0)
    .cursor_default();

    let emoji_btn = button(
        text(if state.emoji_picker_open {
            Icon::EmojiFilled.unicode()
        } else {
            Icon::EmojiRegular.unicode()
        })
        .size(24)
        .font(FLUENT_ICONS),
    )
    .on_press(MainMessage::ToggleEmojiPicker)
    .style(styles::icon_accent)
    .padding(0)
    .cursor_default();

    let emoji_dropdown = DropDown::new(emoji_btn, emoji_picker(state), state.emoji_picker_open)
        .width(Length::Fill)
        .alignment(Alignment::TopStart)
        .on_dismiss(MainMessage::EmojiPickerDismiss);

    let controls = row![attach_icon, emoji_dropdown].spacing(12);

    let chat_box = container(
        row![input, controls]
            .align_y(alignment::Vertical::Center)
            .width(Length::Fill),
    )
    .padding(Padding::from([8, 12]))
    .width(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(iced::Background::Color(BG_CHAT_BOX)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 5.into(),
        },
        ..Default::default()
    });

    container(chat_box)
        .width(Length::Fill)
        .padding(Padding {
            top: 12.0,
            right: 12.0,
            bottom: 12.0,
            left: 12.0,
        })
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_APP)),
            ..Default::default()
        })
        .into()
}

fn emoji_picker(state: &MainView) -> Element<MainMessage> {
    let search = container(
        text_input("Search emojis...", &state.emoji_search)
            .on_input(MainMessage::EmojiSearchChanged)
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
    .padding(Padding::from([2, 4]))
    .width(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(iced::Background::Color(BG_SUNKEN)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 5.into(),
        },
        ..Default::default()
    });

    let categories: &[(emojis::Group, &str)] = &[
        (emojis::Group::SmileysAndEmotion, "😊"),
        (emojis::Group::PeopleAndBody, "👋"),
        (emojis::Group::AnimalsAndNature, "🌿"),
        (emojis::Group::FoodAndDrink, "🍔"),
        (emojis::Group::TravelAndPlaces, "🏠"),
        (emojis::Group::Activities, "⚽"),
        (emojis::Group::Objects, "💡"),
        (emojis::Group::Symbols, "❤\u{fe0f}"),
        (emojis::Group::Flags, "🏁"),
    ];

    let mut cat_row = row![].spacing(2);
    for &(group, icon) in categories {
        let is_active = state.emoji_picker_category == group && state.emoji_search.is_empty();
        cat_row = cat_row.push(
            button(container(text(icon).size(18)).center_x(32).center_y(32))
                .on_press(MainMessage::EmojiCategorySelected(group))
                .padding(0)
                .style(move |_theme, status| {
                    let bg = if is_active {
                        BG_SELECTED
                    } else {
                        match status {
                            button::Status::Hovered => BG_HOVER,
                            _ => Color::TRANSPARENT,
                        }
                    };
                    button::Style {
                        background: Some(iced::Background::Color(bg)),
                        border: Border::default().rounded(4),
                        text_color: TEXT_PRIMARY,
                        ..Default::default()
                    }
                })
                .cursor_default(),
        );
    }

    let category_label = text(if state.emoji_search.is_empty() {
        emoji_category_label(state.emoji_picker_category)
    } else {
        "Search results"
    })
    .size(12)
    .color(TEXT_MUTED)
    .font(Font {
        weight: iced::font::Weight::Bold,
        ..DM_SANS
    });

    let emojis_list: Vec<&emojis::Emoji> = if state.emoji_search.is_empty() {
        emojis::iter()
            .filter(|e| e.group() == state.emoji_picker_category)
            .collect()
    } else {
        let search_lower = state.emoji_search.to_lowercase();
        emojis::iter()
            .filter(|e| e.name().to_lowercase().contains(&search_lower))
            .collect()
    };

    let emojis_per_row = 8;
    let mut grid = Column::new().spacing(2);
    for chunk in emojis_list.chunks(emojis_per_row) {
        let mut emoji_row = row![].spacing(2);
        for emoji in chunk {
            let emoji_str = emoji.as_str().to_string();
            emoji_row = emoji_row.push(
                button(
                    container(text(emoji.as_str()).size(22))
                        .center_x(36)
                        .center_y(36),
                )
                .on_press(MainMessage::EmojiSelected(emoji_str))
                .padding(0)
                .style(|_theme, status| {
                    let bg = match status {
                        button::Status::Hovered => BG_HOVER,
                        button::Status::Pressed => BG_SELECTED,
                        _ => Color::TRANSPARENT,
                    };
                    button::Style {
                        background: Some(iced::Background::Color(bg)),
                        border: Border::default().rounded(4),
                        text_color: TEXT_PRIMARY,
                        ..Default::default()
                    }
                })
                .cursor_default(),
            );
        }
        grid = grid.push(emoji_row);
    }

    let scrollable_grid = scrollable(container(grid).padding(Padding::from([0, 4]))).height(280);

    container(
        column![search, cat_row, category_label, scrollable_grid]
            .spacing(8)
            .width(Length::Shrink),
    )
    .padding(8)
    .style(|_theme| container::Style {
        background: Some(iced::Background::Color(BG_CHAT_BOX)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 8.into(),
        },
        ..Default::default()
    })
    .into()
}

fn emoji_category_label(group: emojis::Group) -> &'static str {
    match group {
        emojis::Group::SmileysAndEmotion => "Smileys and emotion",
        emojis::Group::PeopleAndBody => "People and body",
        emojis::Group::AnimalsAndNature => "Animals and nature",
        emojis::Group::FoodAndDrink => "Food and drink",
        emojis::Group::TravelAndPlaces => "Travel and places",
        emojis::Group::Activities => "Activities",
        emojis::Group::Objects => "Objects",
        emojis::Group::Symbols => "Symbols",
        emojis::Group::Flags => "Flags",
    }
}

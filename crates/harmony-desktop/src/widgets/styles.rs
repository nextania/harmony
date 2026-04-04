use iced::{Border, Color, Shadow, Vector, color, widget::button};

use crate::theme::{
    ACCENT_PURPLE, ACCENT_PURPLE_DIM, BG_HOVER, BG_SELECTED, BG_SELECTED_CHAT, BORDER, LINK_COLOR,
    TEXT_MUTED, TEXT_PRIMARY, TEXT_WHITE,
};

/// For transparent buttons with hover and press feedback
pub fn ghost(_theme: &iced::Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(match status {
            button::Status::Hovered => BG_HOVER,
            button::Status::Pressed => BG_SELECTED,
            _ => Color::TRANSPARENT,
        })),
        border: Border::default().rounded(5),
        text_color: TEXT_PRIMARY,
        ..Default::default()
    }
}

/// For tab selection items
pub fn selectable(is_active: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| button::Style {
        background: Some(iced::Background::Color(if is_active {
            BG_SELECTED
        } else {
            match status {
                button::Status::Hovered => BG_HOVER,
                button::Status::Pressed => BG_SELECTED,
                _ => Color::TRANSPARENT,
            }
        })),
        border: Border::default().rounded(5),
        text_color: TEXT_PRIMARY,
        ..Default::default()
    }
}

/// Like [`selectable`] but with the `BG_SELECTED_CHAT` background
pub fn chat_item(is_selected: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| button::Style {
        background: Some(iced::Background::Color(if is_selected {
            BG_SELECTED_CHAT
        } else {
            match status {
                button::Status::Hovered => BG_HOVER,
                button::Status::Pressed => BG_SELECTED_CHAT,
                _ => Color::TRANSPARENT,
            }
        })),
        border: Border::default().rounded(5),
        text_color: TEXT_PRIMARY,
        ..Default::default()
    }
}

/// Tab-mode selector button
pub fn tab_mode(is_active: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    // TODO: figure out if we can do only a bottom border when active
    move |_theme, status| {
        let border = if is_active {
            Border {
                color: ACCENT_PURPLE,
                width: 2.0,
                radius: 4.into(),
            }
        } else {
            Border::default().rounded(4)
        };
        let bg = if is_active {
            BG_SELECTED
        } else {
            match status {
                button::Status::Hovered => BG_HOVER,
                button::Status::Pressed => BG_SELECTED,
                _ => Color::TRANSPARENT,
            }
        };
        button::Style {
            background: Some(iced::Background::Color(bg)),
            border,
            text_color: TEXT_PRIMARY,
            ..Default::default()
        }
    }
}

/// For the chat box icon buttons
pub fn icon_accent(_theme: &iced::Theme, status: button::Status) -> button::Style {
    match status {
        button::Status::Hovered => button::Style {
            border: Border::default().rounded(5),
            text_color: ACCENT_PURPLE,
            ..Default::default()
        },
        button::Status::Pressed => button::Style {
            border: Border::default().rounded(5),
            text_color: TEXT_MUTED,
            ..Default::default()
        },
        _ => button::Style {
            background: Some(iced::Background::Color(Color::TRANSPARENT)),
            border: Border::default().rounded(5),
            text_color: TEXT_PRIMARY,
            ..Default::default()
        },
    }
}

/// For call control buttons
pub fn call_ctrl(bg: Color) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, _status| button::Style {
        background: Some(iced::Background::Color(bg)),
        border: Border::default().rounded(5),
        text_color: TEXT_PRIMARY,
        ..Default::default()
    }
}

pub fn primary(_theme: &iced::Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(match status {
            button::Status::Hovered => color!(0xa000cc),
            button::Status::Pressed => color!(0x6e008a),
            _ => ACCENT_PURPLE,
        })),
        border: Border::default().rounded(4),
        text_color: TEXT_WHITE,
        ..Default::default()
    }
}

pub fn secondary(_theme: &iced::Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(match status {
            button::Status::Hovered => color!(0x3d2448),
            button::Status::Pressed => color!(0x231528),
            _ => BG_SELECTED,
        })),
        border: Border {
            color: BORDER,
            width: 1.0,
            ..Border::default().rounded(4)
        },
        text_color: TEXT_WHITE,
        ..Default::default()
    }
}

// TODO:
pub fn accent_dim(_theme: &iced::Theme, _status: button::Status) -> button::Style {
    button::Style {
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
    }
}

pub fn link(_theme: &iced::Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        border: Border::default(),
        text_color: LINK_COLOR,
        ..Default::default()
    }
}

/// For the avatar button
pub fn invisible(_theme: &iced::Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        ..Default::default()
    }
}

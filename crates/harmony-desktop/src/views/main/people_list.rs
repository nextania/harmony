use iced::{
    Border, Color, Element, Font, Length, Padding, alignment,
    widget::{Column, button, column, container, image, row, text, text_input},
};

use crate::{
    icons::{FLUENT_ICONS, Icon},
    theme::{
        ACCENT_PURPLE, BG_HOVER, BG_PANEL, BG_SELECTED, BG_SUNKEN, BORDER, DANGER_RED, DM_SANS,
        TEXT_MUTED, TEXT_PRIMARY, TEXT_WHITE,
    },
    views::main::{
        MainMessage, MainView,
        contacts::{Contact, ContactStatus, ContactsMessage},
    },
    widgets::{button::ButtonExt, styles},
};

pub fn people_list(state: &MainView) -> Element<MainMessage> {
    let title = text("People").size(20).color(TEXT_WHITE).font(Font {
        weight: iced::font::Weight::Bold,
        ..DM_SANS
    });

    let add_input = text_input("Add by username...", &state.contacts.add_input)
        .on_input(|s| MainMessage::Contacts(ContactsMessage::AddInputChanged(s)))
        .on_submit(MainMessage::Contacts(ContactsMessage::AddSubmit))
        .size(14)
        .padding(Padding::from([6, 10]))
        .width(Length::Fill)
        .font(DM_SANS)
        .style(|_theme, _status| text_input::Style {
            background: iced::Background::Color(BG_SUNKEN),
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: 6.into(),
            },
            icon: TEXT_MUTED,
            placeholder: TEXT_MUTED,
            value: TEXT_PRIMARY,
            selection: ACCENT_PURPLE,
        });

    let add_btn = button(
        text(Icon::PersonAddFilled.unicode())
            .size(18)
            .color(TEXT_PRIMARY)
            .font(FLUENT_ICONS),
    )
    .on_press(MainMessage::Contacts(ContactsMessage::AddSubmit))
    .padding(Padding::from([6, 8]))
    .style(styles::ghost)
    .cursor_default();

    let add_row = row![add_input, add_btn]
        .spacing(6)
        .align_y(alignment::Vertical::Center);

    let mut contact_items = Column::new().spacing(2).width(Length::Fill);

    if !state.contacts.loaded {
        contact_items = contact_items.push(
            text("Loading contacts...")
                .size(14)
                .color(TEXT_MUTED)
                .font(DM_SANS),
        );
    } else if state.contacts.list.is_empty() {
        contact_items = contact_items.push(
            text("No contacts yet")
                .size(14)
                .color(TEXT_MUTED)
                .font(DM_SANS),
        );
    } else {
        for contact in &state.contacts.list {
            if contact.status != ContactStatus::None {
                contact_items = contact_items.push(contact_row(state, contact, &contact.user_id));
            }
        }
    }

    let content = column![title, add_row, contact_items]
        .spacing(16)
        .width(Length::Fill);

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

fn contact_row(
    state: &MainView,
    contact: &Contact,
    user_id: &str,
) -> Element<'static, MainMessage> {
    let (avatar_url, display_name) = state
        .api
        .users()
        .get(&contact.user_id)
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

    let name = text(display_name).size(14).color(TEXT_PRIMARY).font(Font {
        weight: iced::font::Weight::Medium,
        ..DM_SANS
    });

    let status_label = contact_status_badge(contact.status);

    let name_col = column![name, status_label].spacing(2);

    let actions: Element<MainMessage> = match contact.status {
        ContactStatus::Established => row![
            icon_action_btn(
                Icon::ChatRegular,
                TEXT_PRIMARY,
                MainMessage::OpenPrivateChannel(user_id.to_string()),
            ),
            icon_action_btn(
                Icon::DeleteRegular,
                DANGER_RED,
                MainMessage::Contacts(ContactsMessage::Remove(user_id.to_string())),
            ),
        ]
        .spacing(2)
        .into(),
        ContactStatus::PendingRemote => icon_action_btn(
            Icon::DismissRegular,
            TEXT_MUTED,
            MainMessage::Contacts(ContactsMessage::Remove(user_id.to_string())),
        ),
        ContactStatus::None => icon_action_btn(
            Icon::DismissRegular,
            TEXT_MUTED,
            MainMessage::Contacts(ContactsMessage::Remove(user_id.to_string())),
        ),
        ContactStatus::PendingLocal => row![
            icon_action_btn(
                Icon::CheckmarkRegular,
                ACCENT_PURPLE,
                MainMessage::Contacts(ContactsMessage::Accept(user_id.to_string())),
            ),
            icon_action_btn(
                Icon::DismissRegular,
                DANGER_RED,
                MainMessage::Contacts(ContactsMessage::Remove(user_id.to_string())),
            ),
        ]
        .spacing(2)
        .into(),
        ContactStatus::Blocked => icon_action_btn(
            Icon::PersonProhibitedRegular,
            TEXT_MUTED,
            MainMessage::Contacts(ContactsMessage::Unblock(user_id.to_string())),
        ),
    };

    let left = row![avatar, name_col]
        .spacing(10)
        .align_y(alignment::Vertical::Center)
        .width(Length::Fill);

    let full_row = row![left, actions]
        .align_y(alignment::Vertical::Center)
        .spacing(4)
        .padding(Padding::from([4, 4]));

    container(full_row)
        .width(Length::Fill)
        .style(|_theme| container::Style {
            ..Default::default()
        })
        .into()
}

fn contact_status_badge(status: ContactStatus) -> Element<'static, MainMessage> {
    let (label, color) = match status {
        ContactStatus::Established => ("Friend", Color::from_rgb(0.13, 0.65, 0.35)),
        ContactStatus::PendingLocal => ("Incoming request", Color::from_rgb(0.85, 0.45, 0.13)),
        ContactStatus::PendingRemote => ("Request", Color::from_rgb(0.55, 0.30, 0.90)),
        ContactStatus::Blocked => ("Blocked", Color::from_rgb(0.75, 0.15, 0.15)),
        ContactStatus::None => unreachable!(),
    };
    text(label).size(11).color(color).font(DM_SANS).into()
}

fn icon_action_btn(icon: Icon, color: Color, msg: MainMessage) -> Element<'static, MainMessage> {
    button(
        text(icon.unicode())
            .size(16)
            .color(color)
            .font(FLUENT_ICONS),
    )
    .on_press(msg)
    .padding(Padding::from([4, 6]))
    .style(move |_theme, status| button::Style {
        background: Some(iced::Background::Color(match status {
            button::Status::Hovered => BG_HOVER,
            button::Status::Pressed => BG_SELECTED,
            _ => Color::TRANSPARENT,
        })),
        border: Border::default().rounded(4),
        text_color: color,
        ..Default::default()
    })
    .cursor_default()
}

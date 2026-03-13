use iced::{
    Border, Color, Element, Font, Length, Padding, alignment, color,
    widget::{Column, Space, button, column, container, row, stack, text},
};
use iced_aw::{DropDown, drop_down::Alignment};

use crate::{
    icons::{FLUENT_ICONS, Icon},
    theme::{BG_HOVER, BG_SIDEBAR, BORDER, DM_SANS, LINK_COLOR, TEXT_MUTED, TEXT_PRIMARY},
    views::main::{AvatarAction, MainMessage, MainView, SidebarTab},
    widgets::{button::ButtonExt, styles},
};

pub fn sidebar(state: &MainView) -> Element<MainMessage> {
    let nav_icon = button(
        text(Icon::NavigationRegular.unicode())
            .size(20)
            .color(TEXT_PRIMARY)
            .font(FLUENT_ICONS),
    )
    .on_press(MainMessage::ToggleChatList)
    .style(styles::ghost)
    .cursor_default();

    let tab_button = |label: &str,
                      icon_char: Icon,
                      filled_icon_char: Icon,
                      tab: SidebarTab,
                      is_active: bool|
     -> Element<MainMessage> {
        let label = label.to_string();
        let icon_char = if is_active {
            filled_icon_char.unicode()
        } else {
            icon_char.unicode()
        };
        let icon: Element<MainMessage> = text(icon_char)
            .size(24)
            .color(TEXT_PRIMARY)
            .font(FLUENT_ICONS)
            .align_x(alignment::Horizontal::Center)
            .into();
        let lbl: Element<MainMessage> = text(label)
            .size(12)
            .color(TEXT_PRIMARY)
            .font(if is_active {
                Font {
                    weight: iced::font::Weight::Bold,
                    ..DM_SANS
                }
            } else {
                DM_SANS
            })
            .align_x(alignment::Horizontal::Center)
            .into();
        let content = column![icon, lbl]
            .spacing(4)
            .align_x(alignment::Horizontal::Center)
            .width(Length::Fill);

        button(container(content).center_x(Length::Fill))
            .on_press(MainMessage::TabSelected(tab))
            .style(styles::selectable(is_active))
            .cursor_default()
    };

    let messages_active = matches!(state.active_tab, SidebarTab::Messages);
    let spaces_active = matches!(state.active_tab, SidebarTab::Spaces);
    let people_active = matches!(state.active_tab, SidebarTab::People);

    let tabs: Column<MainMessage> = column![
        tab_button(
            "Messages",
            Icon::ChatRegular,
            Icon::ChatFilled,
            SidebarTab::Messages,
            messages_active
        ),
        tab_button(
            "Spaces",
            Icon::PeopleCommunityRegular,
            Icon::PeopleCommunityFilled,
            SidebarTab::Spaces,
            spaces_active
        ),
        tab_button(
            "People",
            Icon::ContactCardRegular,
            Icon::ContactCardFilled,
            SidebarTab::People,
            people_active
        ),
    ]
    .spacing(6)
    .align_x(alignment::Horizontal::Center);

    let top = column![nav_icon, tabs]
        .spacing(16)
        .align_x(alignment::Horizontal::Center)
        .padding(Padding::from([0, 6]));

    let avatar_inner = container(text("").size(18).color(color!(0x555555)).font(DM_SANS))
        .width(40)
        .height(40)
        .center_x(40)
        .center_y(40)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(TEXT_PRIMARY)),
            border: Border::default().rounded(10),
            ..Default::default()
        });

    let status_dot = container(Space::new())
        .width(12)
        .height(12)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(color!(0x23a55a))),
            border: Border {
                color: BG_SIDEBAR,
                width: 2.0,
                radius: 6.into(),
            },
            ..Default::default()
        });

    let status_overlay = container(status_dot)
        .width(40)
        .height(40)
        .align_x(alignment::Horizontal::Right)
        .align_y(alignment::Vertical::Bottom);

    let avatar_with_status = stack![avatar_inner, status_overlay];

    let avatar_btn = button(avatar_with_status)
        .on_press(MainMessage::ToggleAvatarMenu)
        .padding(0)
        .style(styles::invisible)
        .cursor_default();

    let menu_item =
        |label: &'static str, icon: Icon, action: AvatarAction| -> Element<MainMessage> {
            button(
                row![
                    text(icon.unicode())
                        .size(16)
                        .color(TEXT_PRIMARY)
                        .font(FLUENT_ICONS),
                    text(label).size(14).color(TEXT_PRIMARY).font(DM_SANS)
                ]
                .spacing(8)
                .align_y(alignment::Vertical::Center),
            )
            .on_press(MainMessage::AvatarMenuAction(action))
            .width(Length::Fill)
            .style(styles::ghost)
            .cursor_default()
        };

    let profile_avatar = container(text("").size(14).color(color!(0x555555)).font(DM_SANS))
        .width(36)
        .height(36)
        .center_x(36)
        .center_y(36)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(TEXT_PRIMARY)),
            border: Border::default().rounded(8),
            ..Default::default()
        });

    let set_status_btn = button(text("Set status").size(12).color(LINK_COLOR).font(DM_SANS))
        .on_press(MainMessage::AvatarMenuAction(AvatarAction::Profile))
        .padding(Padding::ZERO)
        .style(|_theme, status| button::Style {
            background: Some(iced::Background::Color(match status {
                button::Status::Hovered => BG_HOVER,
                _ => Color::TRANSPARENT,
            })),
            border: Border::default().rounded(3),
            text_color: LINK_COLOR,
            ..Default::default()
        })
        .cursor_default();

    let profile_info = column![
        text(state.current_user.profile.display_name.clone())
            .size(14)
            .color(TEXT_PRIMARY)
            .font(Font {
                weight: iced::font::Weight::Bold,
                ..DM_SANS
            }),
        text(state.current_user.profile.username.clone())
            .size(12)
            .color(TEXT_MUTED)
            .font(DM_SANS),
        set_status_btn,
    ]
    .spacing(2);

    let profile_card = row![profile_avatar, profile_info]
        .spacing(10)
        .padding(4)
        .align_y(alignment::Vertical::Center);

    let divider =
        container(Space::new().width(Length::Fill).height(1.0)).style(|_theme| container::Style {
            background: Some(iced::Background::Color(BORDER)),
            ..Default::default()
        });

    let avatar_menu = container(
        column![
            profile_card,
            divider,
            menu_item("Settings", Icon::SettingsRegular, AvatarAction::Settings),
            menu_item("Log out", Icon::SignOutRegular, AvatarAction::Logout),
        ]
        .spacing(6)
        .width(Length::Fill),
    )
    .width(160)
    .padding(6)
    .style(|_theme| container::Style {
        background: Some(iced::Background::Color(BG_SIDEBAR)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 8.into(),
        },
        ..Default::default()
    });

    let avatar = DropDown::new(avatar_btn, avatar_menu, state.avatar_menu_open)
        .width(Length::Fill)
        .alignment(Alignment::TopEnd)
        .on_dismiss(MainMessage::AvatarMenuDismiss);

    let sidebar_content = column![top, Space::new().height(Length::Fill), avatar]
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .padding(Padding::from([12, 0]));

    container(sidebar_content)
        .width(80)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_SIDEBAR)),
            ..Default::default()
        })
        .into()
}

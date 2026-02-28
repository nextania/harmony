use iced::{Color, Font, color};

pub const BG_APP: Color = color!(0x12001e); // base app layer
pub const BG_SUNKEN: Color = color!(0x14001d); // recessed inputs / containers
pub const BG_PANEL: Color = color!(0x170021); // panel columns (chat list, top bar)
pub const BG_CHAT_BOX: Color = color!(0x1b0030); // chat input area
pub const BG_SIDEBAR: Color = color!(0x1d002a); // sidebar
pub const BG_HOVER: Color = color!(0x1f1028); // generic hover state
pub const BG_SELECTED: Color = color!(0x2f1b36); // active / selected item
pub const BG_SELECTED_CHAT: Color = Color {
    r: 76.0 / 255.0,
    g: 54.0 / 255.0,
    b: 85.0 / 255.0,
    a: 0.5,
};
pub const BG_CTRL_INACTIVE: Color = Color {
    r: 117.0 / 255.0,
    g: 117.0 / 255.0,
    b: 117.0 / 255.0,
    a: 0.5,
};
pub const BG_PARTICIPANT_CARD: Color = Color {
    r: 148.0 / 255.0,
    g: 148.0 / 255.0,
    b: 148.0 / 255.0,
    a: 0.2,
};
pub const BG_PARTICIPANT_LABEL: Color = Color {
    r: 7.0 / 255.0,
    g: 7.0 / 255.0,
    b: 7.0 / 255.0,
    a: 0.5,
};
pub const OVERLAY: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.6,
};
pub const BG_SCREENSHARE_PANEL: Color = Color {
    r: 10.0 / 255.0,
    g: 6.0 / 255.0,
    b: 30.0 / 255.0,
    a: 1.0,
};
pub const BG_CALL_CARD: Color = Color {
    r: 22.0 / 255.0,
    g: 22.0 / 255.0,
    b: 22.0 / 255.0,
    a: 0.5,
};
pub const BORDER: Color = color!(0x333333); // general borders
pub const BORDER_CARD: Color = color!(0x454545); // card / panel borders
pub const ACCENT_PURPLE: Color = color!(0x8b00ae); // primary accent (selection, focus)
pub const ACCENT_PURPLE_DIM: Color = color!(0x6a009b); // dimmer purple for action buttons
pub const DANGER_RED: Color = color!(0x950000); // destructive / end-call actions
pub const LINK_COLOR: Color = color!(0xc05ff1); // bright purple for clickable links
pub const TEXT_PRIMARY: Color = color!(0xd9d9d9);
pub const TEXT_MUTED: Color = color!(0x979797); // secondary / helper text
pub const TEXT_PLACEHOLDER: Color = color!(0x707070); // input placeholder
pub const SUBTLE_GREY: Color = color!(0x797979); // subtle borders, muted icons / labels
pub const TEXT_WHITE: Color = Color::WHITE;

pub const BG_SPLASH: Color = color!(0x1f0021);
pub const BG_LOGIN_CARD: Color = color!(0x15001e);
pub const BG_LOGIN_INPUT: Color = color!(0x291e2f);

pub const DM_SANS: Font = Font {
    family: iced::font::Family::Name("DM Sans"),
    weight: iced::font::Weight::Normal,
    // NOTE: iced is somehow bugged and falls back to the default font if stretch is set to Normal
    // setting it to SemiCondensed is a workaround, but it works fine and doesn't affect the appearance of the font at all
    #[cfg(target_os = "windows")]
    stretch: iced::font::Stretch::SemiCondensed,
    // UPDATE: on Linux, even the SemiCondensed stretch causes the fallback, so
    // we have to use ExtraCondensed which somehow also fixes the issue
    #[cfg(not(target_os = "windows"))]
    stretch: iced::font::Stretch::ExtraCondensed,
    style: iced::font::Style::Normal,
};

pub const LOGO_SVG: &[u8] = include_bytes!("../assets/logo.svg");
pub const LOGIN_BG: &[u8] = include_bytes!("../assets/login-bg.png");

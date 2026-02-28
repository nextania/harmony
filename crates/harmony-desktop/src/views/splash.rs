use iced::{
    Element, Font, Length, Padding, alignment,
    widget::{Space, Svg, column, container, row, svg::Handle, text},
};

use crate::theme::{BG_SPLASH, DM_SANS, LOGO_SVG, TEXT_MUTED, TEXT_WHITE};

#[derive(Debug, Clone)]
pub enum SplashMessage {}

pub struct SplashView;
impl SplashView {
    pub fn new() -> Self {
        Self
    }
    pub fn view(&self) -> Element<SplashMessage> {
        let logo_mark = Svg::new(Handle::from_memory(LOGO_SVG)).width(48).height(48);
        let logo_text = column![
            text("Harmony").size(24).color(TEXT_WHITE).font(Font {
                weight: iced::font::Weight::Medium,
                ..DM_SANS
            }),
            text("by Nextania").size(16).color(TEXT_WHITE).font(Font {
                weight: iced::font::Weight::ExtraLight,
                ..DM_SANS
            }),
        ];

        let logo = row![logo_mark, logo_text]
            .spacing(12)
            .align_y(alignment::Vertical::Center);

        let bottom = row![
            text("Authenticating...")
                .size(12)
                .color(TEXT_MUTED)
                .font(Font {
                    weight: iced::font::Weight::Light,
                    ..DM_SANS
                }),
            Space::new().width(Length::Fill),
            text(concat!("version ", env!("CARGO_PKG_VERSION")))
                .size(12)
                .color(TEXT_MUTED)
                .font(Font {
                    weight: iced::font::Weight::Light,
                    ..DM_SANS
                }),
        ]
        .width(376);

        container(
            column![
                Space::new().height(Length::Fill),
                container(logo).center_x(Length::Fill),
                Space::new().height(Length::Fill),
                container(bottom).center_x(Length::Fill),
            ]
            .height(Length::Fill)
            .padding(Padding::from([10, 0])),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_SPLASH)),
            ..Default::default()
        })
        .into()
    }
}

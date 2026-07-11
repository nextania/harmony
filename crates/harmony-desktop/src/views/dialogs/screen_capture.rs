use iced::{
    Border, Element, Length, Padding, Task,
    advanced::image::Handle as ImageHandle,
    alignment,
    widget::{Column, Space, button, column, container, image, row, scrollable, text},
};

use crate::{
    Message,
    media::screen_capture::{
        CaptureTargetInfo, CaptureTargetList, ScreenCaptureConfig, ScreenQuality,
    },
    theme::{
        ACCENT_PURPLE, BG_APP, BG_HOVER, BG_SELECTED, BG_SUNKEN, BORDER, DM_SANS, TEXT_MUTED,
        TEXT_PRIMARY,
    },
    widgets::{button::ButtonExt, styles},
};

#[derive(Clone)]
pub enum ScreenCaptureMessage {
    TargetsLoaded(CaptureTargetList),
    SelectTarget(usize),
    QualitySelected(ScreenQuality),
    Confirm,
    Cancel,
}

pub struct ScreenCaptureView {
    targets: Vec<CaptureTargetInfo>,
    thumbnail_handles: Vec<Option<ImageHandle>>,
    selected: Option<usize>,
    quality: ScreenQuality,
    loading: bool,
}

impl ScreenCaptureView {
    pub fn new() -> (Self, Task<Message>) {
        let load = Task::perform(
            crate::media::screen_capture::list_targets_with_thumbnails(),
            |list| Message::ScreenCapture(ScreenCaptureMessage::TargetsLoaded(list)),
        );
        (
            Self {
                targets: Vec::new(),
                thumbnail_handles: Vec::new(),
                selected: None,
                quality: ScreenQuality::P1080,
                loading: true,
            },
            load,
        )
    }

    pub fn update(&mut self, message: ScreenCaptureMessage) -> Task<Message> {
        match message {
            ScreenCaptureMessage::TargetsLoaded(list) => {
                let targets = match list {
                    CaptureTargetList::Portal(info) => vec![info],
                    CaptureTargetList::Targets(targets) => targets,
                };
                self.thumbnail_handles = targets
                    .iter()
                    .map(|info| {
                        info.thumbnail
                            .as_ref()
                            .map(|f| ImageHandle::from_rgba(f.width, f.height, f.rgba.clone()))
                    })
                    .collect();
                self.targets = targets;
                self.loading = false;
                if !self.targets.is_empty() {
                    self.selected = Some(0);
                }
            }
            ScreenCaptureMessage::SelectTarget(idx) => {
                if idx < self.targets.len() {
                    self.selected = Some(idx);
                }
            }
            ScreenCaptureMessage::QualitySelected(q) => {
                self.quality = q;
            }
            ScreenCaptureMessage::Confirm => {
                if let Some(idx) = self.selected {
                    if let Some(info) = self.targets.get(idx) {
                        let target = info.target.clone();
                        let config = ScreenCaptureConfig {
                            quality: self.quality,
                            source_width: info.source_width,
                            source_height: info.source_height,
                            ..Default::default()
                        };
                        return Task::done(Message::ScreenCaptureSelected(target, config));
                    }
                }
            }
            ScreenCaptureMessage::Cancel => {
                return Task::done(Message::CloseScreenCapture);
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<ScreenCaptureMessage> {
        let title = text("Select a screen or window to share")
            .size(16)
            .color(TEXT_PRIMARY)
            .font(DM_SANS);

        let body: Element<ScreenCaptureMessage> = if self.loading {
            container(
                text("Loading available screens...")
                    .size(14)
                    .color(TEXT_MUTED)
                    .font(DM_SANS),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
        } else if self.targets.is_empty() {
            container(
                text("No capturable screens or windows found.")
                    .size(14)
                    .color(TEXT_MUTED)
                    .font(DM_SANS),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
        } else {
            let mut grid = Column::new().spacing(8);

            // Build rows of 3 thumbnails
            let mut current_row = row![].spacing(8);
            for (i, info) in self.targets.iter().enumerate() {
                let is_selected = self.selected == Some(i);
                let border_color = if is_selected { ACCENT_PURPLE } else { BORDER };
                let bg = if is_selected { BG_SELECTED } else { BG_SUNKEN };

                let thumb: Element<ScreenCaptureMessage> =
                    match self.thumbnail_handles.get(i).and_then(|h| h.as_ref()) {
                        Some(handle) => container(
                            image(handle.clone())
                                .width(Length::Fill)
                                .height(Length::Fixed(120.0))
                                .content_fit(iced::ContentFit::Contain),
                        )
                        .width(Length::Fill)
                        .height(Length::Fixed(120.0))
                        .into(),
                        None => {
                            container(text("No preview").size(12).color(TEXT_MUTED).font(DM_SANS))
                                .center_x(Length::Fill)
                                .center_y(120)
                                .into()
                        }
                    };

                let label = text(&info.title).size(11).color(TEXT_PRIMARY).font(DM_SANS);

                let card = button(
                    container(column![thumb, label].spacing(4).width(Length::Fill))
                        .padding(Padding::new(6.0))
                        .style(move |_theme| container::Style {
                            background: Some(iced::Background::Color(bg)),
                            border: Border {
                                color: border_color,
                                width: if is_selected { 2.0 } else { 1.0 },
                                radius: 6.into(),
                            },
                            ..Default::default()
                        }),
                )
                .on_press(ScreenCaptureMessage::SelectTarget(i))
                .padding(0)
                .width(Length::FillPortion(1))
                .style(|_theme, _status| button::Style {
                    background: None,
                    ..Default::default()
                })
                .cursor_default();

                current_row = current_row.push(card);

                if (i + 1) % 3 == 0 {
                    grid = grid.push(current_row);
                    current_row = row![].spacing(8);
                }
            }
            // push remaining partial row
            let remaining = self.targets.len() % 3;
            if remaining > 0 {
                // pad with empty space
                for _ in remaining..3 {
                    current_row = current_row.push(Space::new().width(Length::FillPortion(1)));
                }
                grid = grid.push(current_row);
            }

            scrollable(grid.width(Length::Fill))
                .height(Length::Fill)
                .into()
        };

        let quality_label = text("Quality:").size(13).color(TEXT_MUTED).font(DM_SANS);
        let quality_btn = |q: ScreenQuality, label_text: &str| -> Element<ScreenCaptureMessage> {
            let is_active = self.quality == q;
            let label_text = label_text.to_string();
            button(
                text(label_text)
                    .size(12)
                    .color(if is_active { TEXT_PRIMARY } else { TEXT_MUTED })
                    .font(DM_SANS),
            )
            .on_press(ScreenCaptureMessage::QualitySelected(q))
            .padding(Padding::from([4, 10]))
            .style(move |_theme, status| {
                let bg = if is_active {
                    ACCENT_PURPLE
                } else {
                    match status {
                        button::Status::Hovered => BG_HOVER,
                        _ => BG_SUNKEN,
                    }
                };
                button::Style {
                    background: Some(iced::Background::Color(bg)),
                    border: Border::default().rounded(4),
                    text_color: if is_active { TEXT_PRIMARY } else { TEXT_MUTED },
                    ..Default::default()
                }
            })
            .cursor_default()
            .into()
        };

        let quality_row = row![
            quality_label,
            quality_btn(ScreenQuality::P720, "720p"),
            quality_btn(ScreenQuality::P1080, "1080p"),
            quality_btn(ScreenQuality::P1440, "1440p"),
        ]
        .spacing(6)
        .align_y(alignment::Vertical::Center);

        let can_confirm = self.selected.is_some() && !self.loading;
        let confirm_btn = button(text("Share").font(DM_SANS).size(12))
            .padding(Padding::from([4, 12]))
            .style(styles::primary)
            .on_press_maybe(if can_confirm {
                Some(ScreenCaptureMessage::Confirm)
            } else {
                None
            })
            .cursor_default();

        let cancel_btn = button(text("Cancel").font(DM_SANS).size(12))
            .on_press(ScreenCaptureMessage::Cancel)
            .padding(Padding::from([4, 12]))
            .style(styles::secondary)
            .cursor_default();

        let bottom_row = row![
            quality_row,
            Space::new().width(Length::Fill),
            confirm_btn,
            cancel_btn,
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center);

        container(
            column![title, body, bottom_row]
                .spacing(10)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding::new(14.0))
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(BG_APP)),
            ..Default::default()
        })
        .into()
    }
}

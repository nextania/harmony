use iced::advanced::image as iced_image;
use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::tree;
use iced::advanced::{Layout, Widget};
use iced::{Element, Length, Rectangle, Size, Theme};

use crate::media::video::Frame;

const DEFAULT_WIDTH: f32 = 640.0;
const DEFAULT_HEIGHT: f32 = 360.0;

pub struct VideoPlayer<'a> {
    frame: Option<&'a Frame>,
    width: Length,
    height: Length,
}

impl<'a> VideoPlayer<'a> {
    pub fn new(frame: Option<&'a Frame>) -> Self {
        Self {
            frame,
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for VideoPlayer<'_>
where
    Renderer:
        iced::advanced::Renderer + iced_image::Renderer<Handle = iced::advanced::image::Handle>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(
        &mut self,
        _tree: &mut iced::advanced::widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let (intrinsic_w, intrinsic_h) = match self.frame {
            Some(f) => (f.width as f32, f.height as f32),
            None => (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        };

        let aspect = intrinsic_w / intrinsic_h;

        let limits = limits.width(self.width).height(self.height);
        let max = limits.max();

        // preserve aspect ratio
        let (w, h) = if max.width / max.height > aspect {
            (max.height * aspect, max.height)
        } else {
            (max.width, max.width / aspect)
        };

        layout::Node::new(Size::new(w, h))
    }

    fn draw(
        &self,
        _tree: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        match self.frame {
            Some(frame) => {
                let handle =
                    iced_image::Handle::from_rgba(frame.width, frame.height, frame.rgba.clone());
                let image =
                    iced_image::Image::new(handle).filter_method(iced_image::FilterMethod::Linear);
                iced_image::Renderer::draw_image(renderer, image, bounds, bounds);
            }
            None => {
                // placeholder
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        border: iced::Border {
                            radius: 4.0.into(),
                            width: 1.0,
                            color: iced::Color::from_rgb(0.3, 0.3, 0.3),
                        },
                        shadow: iced::Shadow::default(),
                        snap: true,
                    },
                    iced::Color::from_rgb(0.1, 0.1, 0.1),
                );
            }
        }
    }
}

impl<'a, Message> From<VideoPlayer<'a>> for Element<'a, Message>
where
    Message: 'a,
{
    fn from(player: VideoPlayer<'a>) -> Self {
        Element::new(player)
    }
}

pub fn video_player(frame: Option<&Frame>) -> VideoPlayer<'_> {
    VideoPlayer::new(frame)
}

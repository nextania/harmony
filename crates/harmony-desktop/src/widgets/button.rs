use iced::{
    Element,
    advanced::Widget,
    widget::{Button, button::Catalog},
};

pub trait ButtonExt<'a, Message> {
    fn cursor_default(self) -> Element<'a, Message>;
}

impl<'a, Message> ButtonExt<'a, Message> for Button<'a, Message>
where
    Message: 'a + Clone,
{
    fn cursor_default(self) -> Element<'a, Message> {
        ButtonOverride::new(self)
    }
}

pub struct ButtonOverride<'a, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: iced::advanced::Renderer,
{
    button: Button<'a, Message, Theme, Renderer>,
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for ButtonOverride<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: Catalog,
    Renderer: 'a + iced::advanced::Renderer,
{
    fn size_hint(&self) -> iced::Size<iced::Length> {
        self.button.size_hint()
    }

    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        self.button.tag()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        self.button.state()
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        self.button.children()
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        self.button.diff(tree);
    }

    fn operate(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.button.operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) {
        self.button.update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        _tree: &iced::advanced::widget::Tree,
        _layout: iced::advanced::Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
        _renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        iced::advanced::mouse::Interaction::Idle
    }

    fn size(&self) -> iced::Size<iced::Length> {
        self.button.size()
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        self.button.layout(tree, renderer, limits)
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        self.button
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }
}

impl<'a, Message, Theme, Renderer> ButtonOverride<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a + Catalog,
    Renderer: 'a + iced::advanced::Renderer,
{
    pub fn new(
        button: Button<'a, Message, Theme, Renderer>,
    ) -> Element<'a, Message, Theme, Renderer> {
        Element::new(ButtonOverride { button })
    }
}

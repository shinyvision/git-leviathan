//! Tiny wrapper widget that intercepts wheel events *before* the child sees
//! them, optionally translating the scroll into a custom message and
//! capturing the event. Used for shift+wheel-to-horizontal in the diff view:
//! `iced::widget::mouse_area` defers to its child first, so a scrollable
//! child swallows the wheel before mouse_area can react. This widget runs
//! its own update first.
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{mouse, Clipboard, Shell};
use iced::event::Event;
use iced::{Element, Length, Rectangle, Size, Vector};

pub struct WheelIntercept<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    on_scroll: Option<Box<dyn Fn(mouse::ScrollDelta) -> Option<Message> + 'a>>,
    enabled: bool,
}

impl<'a, Message, Theme, Renderer> WheelIntercept<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            on_scroll: None,
            enabled: true,
        }
    }

    /// The handler returns `Some(msg)` to consume the wheel event (published +
    /// captured so the child scrollable never sees it) or `None` to let it
    /// fall through — e.g. a horizontal-only trackpad pan the intercept
    /// shouldn't swallow.
    pub fn on_scroll(
        mut self,
        on_scroll: impl Fn(mouse::ScrollDelta) -> Option<Message> + 'a,
    ) -> Self {
        self.on_scroll = Some(Box::new(on_scroll));
        self
    }

    /// When false, wheel events pass straight through to the child without
    /// being captured. Always wrapping the child (regardless of `enabled`)
    /// keeps the widget tree stable so child state — like a scrollable's
    /// scroll offset — survives toggling.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for WheelIntercept<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::stateless()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::None
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // Intercept wheel events *before* child handles them. If we have a
        // scroll handler and the cursor is over our bounds, publish + capture
        // so the inner scrollable never sees the event.
        if self.enabled {
            if let (Event::Mouse(mouse::Event::WheelScrolled { delta }), Some(handler)) =
                (event, self.on_scroll.as_ref())
            {
                if cursor.is_over(layout.bounds()) {
                    if let Some(message) = handler(*delta) {
                        shell.publish(message);
                        shell.capture_event();
                        return;
                    }
                }
            }
        }

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        renderer_style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            renderer_style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<WheelIntercept<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(w: WheelIntercept<'a, Message, Theme, Renderer>) -> Self {
        Element::new(w)
    }
}

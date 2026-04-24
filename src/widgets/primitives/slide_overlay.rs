use iced::advanced::overlay;
use iced::advanced::widget::tree::Tree;
use iced::advanced::widget::{Operation, Widget};
use iced::advanced::{layout, renderer, Clipboard, Layout, Shell};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

pub struct SlideOverlay<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    slide_offset: f32,
    top_offset: f32,
    left_offset: f32,
    width: Length,
    bottom_inset: f32,
}

impl<'a, Message, Theme, Renderer> SlideOverlay<'a, Message, Theme, Renderer>
where
    Theme: iced::widget::container::Catalog + 'a,
    Renderer: renderer::Renderer + 'a,
    Message: Clone + 'a,
{
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        slide_offset: f32,
        top_offset: f32,
        left_offset: f32,
        width: impl Into<Length>,
        bottom_inset: f32,
    ) -> Self {
        Self {
            content: content.into(),
            slide_offset,
            top_offset,
            left_offset,
            width: width.into(),
            bottom_inset,
        }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for SlideOverlay<'a, Message, Theme, Renderer>
where
    Theme: iced::widget::container::Catalog + 'a,
    Renderer: renderer::Renderer + 'a,
    Message: Clone + 'a,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<()>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::None
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.content]);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn size_hint(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let full_height = limits.max().height;
        let panel_height = (full_height - self.top_offset - self.bottom_inset).max(0.0);
        let available_width = (limits.max().width - self.left_offset).max(0.0);
        let child_limits =
            layout::Limits::new(Size::ZERO, Size::new(available_width, panel_height))
                .width(self.width)
                .height(Length::Fixed(panel_height));
        let child_node =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &child_limits);

        layout::Node::with_children(
            Size::new(limits.max().width, full_height),
            vec![child_node.move_to(Point::new(self.left_offset, self.top_offset))],
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let children: Vec<_> = layout.children().collect();
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            children[0],
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: iced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let children: Vec<_> = layout.children().collect();
        let clip_rect = children[0].bounds();

        let translated_viewport = Rectangle {
            x: viewport.x + self.slide_offset,
            ..*viewport
        };

        if clip_rect.intersects(viewport) {
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                children[0],
                cursor,
                renderer,
                clipboard,
                shell,
                &translated_viewport,
            )
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> iced::mouse::Interaction {
        let child: Vec<_> = layout.children().collect();
        let child_bounds = child[0].bounds();
        let clip_rect = Rectangle {
            x: child_bounds.x - self.slide_offset,
            y: child_bounds.y,
            width: child_bounds.width,
            height: child_bounds.height,
        };

        if cursor.is_over(clip_rect) {
            let child_interaction = self.content.as_widget().mouse_interaction(
                &tree.children[0],
                child[0],
                cursor,
                viewport,
                renderer,
            );
            if child_interaction == iced::mouse::Interaction::None {
                iced::mouse::Interaction::Idle
            } else {
                child_interaction
            }
        } else {
            iced::mouse::Interaction::None
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let child_layout = layout.children().next()?;
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            child_layout,
            renderer,
            viewport,
            translation - Vector::new(self.slide_offset, 0.0),
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let children: Vec<_> = layout.children().collect();
        let clip_rect = children[0].bounds();

        renderer.with_layer(clip_rect, |renderer| {
            renderer.with_translation(Vector::new(-self.slide_offset, 0.0), |renderer| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    children[0],
                    cursor,
                    viewport,
                );
            });
        });
    }
}

impl<'a, Message, Theme, Renderer> From<SlideOverlay<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Theme: iced::widget::container::Catalog + 'a,
    Renderer: renderer::Renderer + 'a,
    Message: Clone + 'a,
{
    fn from(widget: SlideOverlay<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}

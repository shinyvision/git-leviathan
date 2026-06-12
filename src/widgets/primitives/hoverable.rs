use iced::advanced::widget::tree::Tree;
use iced::advanced::widget::{Operation, Widget};
use iced::advanced::{layout, mouse, renderer, Clipboard, Layout, Shell};
use iced::{Element, Event, Length, Point, Rectangle, Size};

/// Hover status for styling, similar to button::Status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverStatus {
    Idle,
    Hovered,
}

impl HoverStatus {
    pub fn is_hovered(self) -> bool {
        matches!(self, HoverStatus::Hovered)
    }
}

type StyleFn<'a, Theme> = Box<dyn Fn(&Theme, HoverStatus) -> iced::widget::container::Style + 'a>;

/// A widget that provides hover styling for any content.
/// Computes hover state fresh each frame like Button does.
/// For visual-only hovers (background/border changes).
pub struct Hoverable<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    style: StyleFn<'a, Theme>,
}

impl<'a, Message, Theme, Renderer> Hoverable<'a, Message, Theme, Renderer> {
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        style: impl Fn(&Theme, HoverStatus) -> iced::widget::container::Style + 'a,
    ) -> Self {
        Self {
            content: content.into(),
            style: Box::new(style),
        }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Hoverable<'a, Message, Theme, Renderer>
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
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
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

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
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
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // Compute hover fresh each frame, exactly like Button does
        let is_hovered = cursor.is_over(layout.bounds());
        let status = if is_hovered {
            HoverStatus::Hovered
        } else {
            HoverStatus::Idle
        };

        let style = (self.style)(theme, status);

        // Draw container background
        if let Some(background) = style.background {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    border: style.border,
                    ..Default::default()
                },
                background,
            );
        }

        // Draw content
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            &renderer::Style::default(),
            layout,
            cursor,
            viewport,
        );
    }
}

impl<'a, Message, Theme, Renderer> From<Hoverable<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Theme: iced::widget::container::Catalog + 'a,
    Renderer: renderer::Renderer + 'a,
    Message: Clone + 'a,
{
    fn from(hoverable: Hoverable<'a, Message, Theme, Renderer>) -> Self {
        Element::new(hoverable)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// HoverableSwap - For content-swapping hovers (revealing buttons on hover)
// ─────────────────────────────────────────────────────────────────────────────

/// Widget that swaps content based on hover state.
/// Stores hover state internally, renders `hover_content` when hovered,
/// `idle_content` otherwise.
pub struct HoverableSwap<'a, Message, Theme, Renderer> {
    idle_content: Element<'a, Message, Theme, Renderer>,
    hover_content: Element<'a, Message, Theme, Renderer>,
    style_idle: Box<dyn Fn(&Theme) -> iced::widget::container::Style + 'a>,
    style_hover: Box<dyn Fn(&Theme) -> iced::widget::container::Style + 'a>,
}

impl<'a, Message, Theme, Renderer> HoverableSwap<'a, Message, Theme, Renderer> {
    pub fn new(
        idle_content: impl Into<Element<'a, Message, Theme, Renderer>>,
        hover_content: impl Into<Element<'a, Message, Theme, Renderer>>,
        style_idle: impl Fn(&Theme) -> iced::widget::container::Style + 'a,
        style_hover: impl Fn(&Theme) -> iced::widget::container::Style + 'a,
    ) -> Self {
        Self {
            idle_content: idle_content.into(),
            hover_content: hover_content.into(),
            style_idle: Box::new(style_idle),
            style_hover: Box::new(style_hover),
        }
    }
}

#[derive(Debug, Default)]
struct HoverableSwapState {
    is_hovered: bool,
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for HoverableSwap<'a, Message, Theme, Renderer>
where
    Theme: iced::widget::container::Catalog + 'a,
    Renderer: renderer::Renderer + 'a,
    Message: Clone + 'a,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<HoverableSwapState>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(HoverableSwapState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![
            Tree::new(&self.idle_content),
            Tree::new(&self.hover_content),
        ]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.idle_content, &self.hover_content]);
    }

    fn size(&self) -> Size<Length> {
        // Use max of both contents so layout is stable
        let idle_size = self.idle_content.as_widget().size();
        let hover_size = self.hover_content.as_widget().size();
        Size::new(
            max_length(idle_size.width, hover_size.width),
            max_length(idle_size.height, hover_size.height),
        )
    }

    fn size_hint(&self) -> Size<Length> {
        let idle_hint = self.idle_content.as_widget().size_hint();
        let hover_hint = self.hover_content.as_widget().size_hint();
        Size::new(
            max_length(idle_hint.width, hover_hint.width),
            max_length(idle_hint.height, hover_hint.height),
        )
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Layout both children
        let idle_node =
            self.idle_content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits);
        let hover_node =
            self.hover_content
                .as_widget_mut()
                .layout(&mut tree.children[1], renderer, limits);

        // Use larger of the two as our size
        let size = Size::new(
            idle_node.size().width.max(hover_node.size().width),
            idle_node.size().height.max(hover_node.size().height),
        );

        // Reposition children to fill our bounds (at origin)
        let idle_node = idle_node.move_to(Point::ORIGIN);
        let hover_node = hover_node.move_to(Point::ORIGIN);

        // Store both layouts
        layout::Node::with_children(size, vec![idle_node, hover_node])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let state: &HoverableSwapState = tree.state.downcast_ref();
        let children: Vec<_> = layout.children().collect();

        // Only operate on visible child
        if state.is_hovered {
            self.hover_content.as_widget_mut().operate(
                &mut tree.children[1],
                children[1],
                renderer,
                operation,
            );
        } else {
            self.idle_content.as_widget_mut().operate(
                &mut tree.children[0],
                children[0],
                renderer,
                operation,
            );
        }
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
        let state: &mut HoverableSwapState = tree.state.downcast_mut();
        let bounds = layout.bounds();
        let children: Vec<_> = layout.children().collect();

        // Update hover state based on cursor
        state.is_hovered = cursor.is_over(bounds);

        // Forward event to currently visible child
        if state.is_hovered {
            self.hover_content.as_widget_mut().update(
                &mut tree.children[1],
                event,
                children[1],
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        } else {
            self.idle_content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                children[0],
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state: &HoverableSwapState = tree.state.downcast_ref();
        let children: Vec<_> = layout.children().collect();

        if state.is_hovered {
            self.hover_content.as_widget().mouse_interaction(
                &tree.children[1],
                children[1],
                cursor,
                viewport,
                renderer,
            )
        } else {
            self.idle_content.as_widget().mouse_interaction(
                &tree.children[0],
                children[0],
                cursor,
                viewport,
                renderer,
            )
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let _state: &HoverableSwapState = tree.state.downcast_ref();
        let bounds = layout.bounds();
        let children: Vec<_> = layout.children().collect();

        // Compute fresh hover check (in case UI moved under cursor)
        let is_hovered = cursor.is_over(bounds);

        // Choose style and content based on hover
        let (style, content, child_tree, child_layout) = if is_hovered {
            (
                (self.style_hover)(theme),
                &self.hover_content,
                &tree.children[1],
                children[1],
            )
        } else {
            (
                (self.style_idle)(theme),
                &self.idle_content,
                &tree.children[0],
                children[0],
            )
        };

        // Draw background
        if let Some(background) = style.background {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: style.border,
                    ..Default::default()
                },
                background,
            );
        }

        // Draw content
        content.as_widget().draw(
            child_tree,
            renderer,
            theme,
            &renderer::Style::default(),
            child_layout,
            cursor,
            viewport,
        );
    }
}

impl<'a, Message, Theme, Renderer> From<HoverableSwap<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Theme: iced::widget::container::Catalog + 'a,
    Renderer: renderer::Renderer + 'a,
    Message: Clone + 'a,
{
    fn from(widget: HoverableSwap<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}

/// Convenience function to wrap with hover content swap
pub fn hoverable_swap<'a, Message, Theme, Renderer>(
    idle_content: impl Into<Element<'a, Message, Theme, Renderer>>,
    hover_content: impl Into<Element<'a, Message, Theme, Renderer>>,
    style_idle: impl Fn(&Theme) -> iced::widget::container::Style + 'a,
    style_hover: impl Fn(&Theme) -> iced::widget::container::Style + 'a,
) -> Element<'a, Message, Theme, Renderer>
where
    Theme: iced::widget::container::Catalog + 'a,
    Renderer: renderer::Renderer + 'a,
    Message: Clone + 'a,
{
    HoverableSwap::new(idle_content, hover_content, style_idle, style_hover).into()
}

fn max_length(a: Length, b: Length) -> Length {
    // Fill > FillPortion > Fixed > Shrink
    match (a, b) {
        (Length::Fill, _) | (_, Length::Fill) => Length::Fill,
        (Length::FillPortion(a), Length::FillPortion(b)) => Length::FillPortion(a.max(b)),
        (Length::FillPortion(a), _) | (_, Length::FillPortion(a)) => Length::FillPortion(a),
        (Length::Fixed(a), Length::Fixed(b)) => Length::Fixed(a.max(b)),
        (Length::Fixed(a), Length::Shrink) | (Length::Shrink, Length::Fixed(a)) => Length::Fixed(a),
        (Length::Shrink, Length::Shrink) => Length::Shrink,
    }
}

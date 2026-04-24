//! Tab wrapper emitting drag-reorder messages. Forwards events to the child
//! first so the inner close button can claim its own presses.

use iced::advanced::widget::tree::{State, Tag, Tree};
use iced::advanced::widget::{Operation, Widget};
use iced::advanced::{layout, mouse, renderer, Clipboard, Layout, Shell};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme};

use crate::core::TabId;
use crate::message::{AppMessage, Message};

pub struct DraggableTab<'a> {
    content: Element<'a, Message>,
    tab_id: TabId,
    is_pressed: bool,
    is_dragging: bool,
    any_drag_active: bool,
}

impl<'a> DraggableTab<'a> {
    pub fn new(
        content: impl Into<Element<'a, Message>>,
        tab_id: TabId,
        is_pressed: bool,
        is_dragging: bool,
        any_drag_active: bool,
    ) -> Self {
        Self {
            content: content.into(),
            tab_id,
            is_pressed,
            is_dragging,
            any_drag_active,
        }
    }
}

impl<'a> Widget<Message, Theme, Renderer> for DraggableTab<'a> {
    fn tag(&self) -> Tag {
        Tag::of::<()>()
    }

    fn state(&self) -> State {
        State::None
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
        );
        if shell.is_event_captured() {
            return;
        }

        let bounds = layout.bounds();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_over(bounds) {
                    shell.publish(Message::App(AppMessage::TabPressed(self.tab_id, pos)));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let Some(pos) = cursor.position() else {
                    return;
                };
                if self.is_pressed || self.is_dragging {
                    shell.publish(Message::App(AppMessage::TabDragCursorMoved(pos)));
                }
                if self.any_drag_active && !self.is_dragging && cursor.is_over(bounds) {
                    shell.publish(Message::App(AppMessage::TabDragHover(self.tab_id)));
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.is_pressed || self.is_dragging =>
            {
                shell.publish(Message::App(AppMessage::TabDragReleased));
                shell.capture_event();
            }
            _ => {}
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
        if self.is_dragging {
            return mouse::Interaction::Grabbing;
        }
        let over = cursor.is_over(layout.bounds());
        let inner = self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        );
        match inner {
            mouse::Interaction::None if over => mouse::Interaction::Pointer,
            other => other,
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
}

impl<'a> From<DraggableTab<'a>> for Element<'a, Message> {
    fn from(widget: DraggableTab<'a>) -> Self {
        Element::new(widget)
    }
}

//! Vertical list whose rows can be dragged into a new order. The vertical
//! sibling of the tab bar: the same `DragState` decides click vs drag and
//! routes hovers to slots, and the same `PositionTracker` eases displaced
//! rows to their new place.
//!
//! The caller owns the canonical order and reacts to `on_select(K)` (a row
//! clicked without dragging) and `on_reorder(Vec<K>)` (a drag released with
//! a changed order). Widgets inside a row (buttons) get events first, so a
//! press on them never starts a drag.
//!
//! While a row is dragged it is drawn "lifted" — on a raised card with a
//! drop shadow — follows the cursor vertically within the list's bounds, and
//! the other rows slide out of its way.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::Instant;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::tree::{State, Tag, Tree};
use iced::advanced::widget::{Operation, Widget};
use iced::advanced::Renderer as _;
use iced::advanced::{mouse, renderer, Clipboard, Shell};
use iced::{
    window, Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Shadow, Size, Theme,
    Vector,
};

use crate::widgets::tab_bar::state::{DragAxis, DragOutcome, DragState};
use crate::widgets::tab_bar::tracker::PositionTracker;

/// Look of the card a dragged row is drawn on.
#[derive(Debug, Clone, Copy)]
pub struct LiftedStyle {
    pub background: Color,
    pub border: Border,
    pub shadow: Shadow,
}

pub struct ReorderList<'a, K: Eq + Hash + Copy + 'static, M> {
    rows: Vec<Element<'a, M>>,
    keys: Vec<K>,
    lifted: Option<LiftedStyle>,
    on_select: Option<Box<dyn Fn(K) -> M + 'a>>,
    on_reorder: Option<Box<dyn Fn(Vec<K>) -> M + 'a>>,
}

impl<'a, K: Eq + Hash + Copy + 'static, M: Clone + 'a> ReorderList<'a, K, M> {
    pub fn new(rows: Vec<(K, Element<'a, M>)>) -> Self {
        let (keys, rows) = rows.into_iter().unzip();
        Self {
            rows,
            keys,
            lifted: None,
            on_select: None,
            on_reorder: None,
        }
    }

    pub fn lifted_style(mut self, style: LiftedStyle) -> Self {
        self.lifted = Some(style);
        self
    }

    pub fn on_select(mut self, f: impl Fn(K) -> M + 'a) -> Self {
        self.on_select = Some(Box::new(f));
        self
    }

    pub fn on_reorder(mut self, f: impl Fn(Vec<K>) -> M + 'a) -> Self {
        self.on_reorder = Some(Box::new(f));
        self
    }

    /// `(key, top, bottom)` per row as it would stack in `working_order`,
    /// using each row's real height.
    fn working_slots(&self, working_order: Option<&[K]>, layout: Layout<'_>) -> Vec<(K, f32, f32)> {
        let heights: HashMap<K, f32> = self
            .keys
            .iter()
            .zip(layout.children())
            .map(|(k, child)| (*k, child.bounds().height))
            .collect();
        let order = working_order.unwrap_or(&self.keys);
        stack_slots(order, &heights, layout.bounds().y)
    }

    fn slot_spans(&self, working_order: Option<&[K]>, layout: Layout<'_>) -> Vec<(f32, f32)> {
        self.working_slots(working_order, layout)
            .into_iter()
            .map(|(_, top, bottom)| (top, bottom))
            .collect()
    }

    fn row_at(&self, cursor: Point, layout: Layout<'_>) -> Option<(K, f32)> {
        self.keys
            .iter()
            .zip(layout.children())
            .find(|(_, child)| child.bounds().contains(cursor))
            .map(|(k, child)| (*k, child.bounds().y))
    }

    fn drive_drag(
        &self,
        state: &mut ListState<K>,
        cursor: Point,
        layout: Layout<'_>,
        now: Instant,
    ) -> bool {
        let working = state.drag.working_order().map(<[K]>::to_vec);
        let slots = self.slot_spans(working.as_deref(), layout);
        let ListState { drag, tracker } = state;
        drag.on_cursor_moved(cursor, &slots, |k| tracker.is_animating_key(k, now))
    }

    /// Vertical translation that draws row `key` (laid out at `layout_y`) in
    /// its current visual position.
    fn row_translation(
        &self,
        state: &ListState<K>,
        key: K,
        child: Rectangle,
        list: Rectangle,
        sim: &HashMap<K, f32>,
        now: Instant,
    ) -> f32 {
        if state.drag.dragging_key() == Some(key) {
            return match (state.drag.cursor(), state.drag.grab_offset()) {
                (Some(cursor), Some(grab)) => {
                    lifted_top(cursor.y, grab, list, child.height) - child.y
                }
                _ => 0.0,
            };
        }
        let target = sim.get(&key).copied().unwrap_or(child.y);
        target + state.tracker.offset_for(key, now) - child.y
    }
}

/// `(key, top, bottom)` for rows stacked from `top` in `order`, each as tall
/// as `heights` says (unknown keys collapse to zero height).
fn stack_slots<K: Eq + Hash + Copy>(
    order: &[K],
    heights: &HashMap<K, f32>,
    top: f32,
) -> Vec<(K, f32, f32)> {
    let mut y = top;
    order
        .iter()
        .map(|k| {
            let h = heights.get(k).copied().unwrap_or(0.0);
            let slot = (*k, y, y + h);
            y += h;
            slot
        })
        .collect()
}

/// Top edge of the dragged row: under the cursor at the point it was
/// grabbed, but never outside the list.
fn lifted_top(cursor_y: f32, grab: f32, list: Rectangle, row_height: f32) -> f32 {
    let lowest = (list.y + list.height - row_height).max(list.y);
    (cursor_y - grab).clamp(list.y, lowest)
}

struct ListState<K: Eq + Hash + Copy + 'static> {
    drag: DragState<K>,
    tracker: PositionTracker<K>,
}

impl<K: Eq + Hash + Copy + 'static> Default for ListState<K> {
    fn default() -> Self {
        Self {
            drag: DragState::with_axis(DragAxis::Vertical),
            tracker: PositionTracker::new(),
        }
    }
}

impl<'a, K, M> Widget<M, Theme, Renderer> for ReorderList<'a, K, M>
where
    K: Eq + Hash + Copy + 'static,
    M: Clone + 'a,
{
    fn tag(&self) -> Tag {
        Tag::of::<ListState<K>>()
    }

    fn state(&self) -> State {
        State::new(ListState::<K>::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.rows.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.rows);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = limits.max().width;
        let row_limits =
            layout::Limits::new(Size::new(width, 0.0), Size::new(width, f32::INFINITY));
        let mut y = 0.0;
        let nodes: Vec<layout::Node> = self
            .rows
            .iter_mut()
            .zip(tree.children.iter_mut())
            .map(|(row, row_tree)| {
                let node = row
                    .as_widget_mut()
                    .layout(row_tree, renderer, &row_limits)
                    .move_to(Point::new(0.0, y));
                y += node.size().height;
                node
            })
            .collect();
        layout::Node::with_children(
            limits.resolve(Length::Fill, Length::Fixed(y), Size::new(width, y)),
            nodes,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        for ((row, row_tree), row_layout) in self
            .rows
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            row.as_widget_mut()
                .operate(row_tree, row_layout, renderer, operation);
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
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        // A drag owns its release, wherever the cursor is.
        {
            let state: &mut ListState<K> = tree.state.downcast_mut();
            if state.drag.is_dragging()
                && matches!(
                    event,
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                )
            {
                if let DragOutcome::Reorder(order) = state.drag.on_released() {
                    if let Some(f) = self.on_reorder.as_ref() {
                        shell.publish(f(order));
                    }
                }
                shell.capture_event();
                shell.request_redraw();
                return;
            }
        }

        // Rows don't track the cursor while another row is being dragged
        // over them, so no hover highlight chases the moving rows.
        let dragging = tree.state.downcast_ref::<ListState<K>>().drag.is_dragging();
        let row_cursor = if dragging {
            mouse::Cursor::Unavailable
        } else {
            cursor
        };
        for ((row, row_tree), row_layout) in self
            .rows
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            row.as_widget_mut().update(
                row_tree, event, row_layout, row_cursor, renderer, clipboard, shell, viewport,
            );
            if shell.is_event_captured() {
                return;
            }
        }

        let state: &mut ListState<K> = tree.state.downcast_mut();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                // A press while still tracking means the previous release
                // never reached us; start over rather than resume it.
                if state.drag.is_tracking_pointer() {
                    state.drag.cancel();
                    shell.request_redraw();
                }
                let Some(pos) = cursor.position_over(layout.bounds()) else {
                    return;
                };
                if let Some((key, top)) = self.row_at(pos, layout) {
                    state.drag.on_press(key, pos, top, &self.keys);
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if !state.drag.is_tracking_pointer() {
                    return;
                }
                // `land` keeps tracking once the pointer leaves an enclosing
                // scrollable: it then hands us a levitating cursor that is
                // still translated into the list's (scrolled) coordinates,
                // unlike the raw event position.
                let Some(position) = cursor.land().position() else {
                    return;
                };
                let was_dragging = state.drag.is_dragging();
                self.drive_drag(state, position, layout, Instant::now());
                if was_dragging || state.drag.is_dragging() {
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                match state.drag.on_released() {
                    DragOutcome::Click(key) => {
                        if let Some(f) = self.on_select.as_ref() {
                            shell.publish(f(key));
                            shell.capture_event();
                        }
                    }
                    DragOutcome::Reorder(order) => {
                        if let Some(f) = self.on_reorder.as_ref() {
                            shell.publish(f(order));
                            shell.capture_event();
                        }
                    }
                    DragOutcome::None => {}
                }
                shell.request_redraw();
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                state.drag.clear_committed_if_matches(&self.keys);
                let working = state.drag.working_order().map(<[K]>::to_vec);
                let sim: HashMap<K, f32> = self
                    .working_slots(working.as_deref(), layout)
                    .into_iter()
                    .map(|(k, top, _)| (k, top))
                    .collect();
                state.tracker.settle(&sim, *now);
                if state.drag.is_dragging() {
                    // A displaced row finishing its slide can unblock a swap
                    // even while the mouse is still.
                    if let Some(cursor) = state.drag.cursor() {
                        self.drive_drag(state, cursor, layout, *now);
                    }
                    shell.request_redraw();
                }
                if state.tracker.is_animating(*now) {
                    shell.request_redraw();
                }
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
        let state: &ListState<K> = tree.state.downcast_ref();
        if state.drag.is_dragging() {
            return mouse::Interaction::Grabbing;
        }
        for ((row, row_tree), row_layout) in self
            .rows
            .iter()
            .zip(tree.children.iter())
            .zip(layout.children())
        {
            let inner = row
                .as_widget()
                .mouse_interaction(row_tree, row_layout, cursor, viewport, renderer);
            if inner != mouse::Interaction::None {
                return inner;
            }
        }
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
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
        let state: &ListState<K> = tree.state.downcast_ref();
        let list = layout.bounds();
        let now = Instant::now();
        let working = state.drag.working_order().map(<[K]>::to_vec);
        let sim: HashMap<K, f32> = self
            .working_slots(working.as_deref(), layout)
            .into_iter()
            .map(|(k, top, _)| (k, top))
            .collect();
        let dragging = state.drag.dragging_key();

        let mut lifted = None;
        for (((row, row_tree), row_layout), key) in self
            .rows
            .iter()
            .zip(tree.children.iter())
            .zip(layout.children())
            .zip(self.keys.iter())
        {
            let child = row_layout.bounds();
            let dy = self.row_translation(state, *key, child, list, &sim, now);
            if dragging == Some(*key) {
                lifted = Some((row, row_tree, row_layout, dy));
                continue;
            }
            let row_cursor = if dragging.is_some() {
                mouse::Cursor::Unavailable
            } else {
                cursor
            };
            renderer.with_translation(Vector::new(0.0, dy), |r| {
                row.as_widget()
                    .draw(row_tree, r, theme, style, row_layout, row_cursor, viewport);
            });
        }

        // The dragged row goes on its own layer so it paints above the rest.
        if let Some((row, row_tree, row_layout, dy)) = lifted {
            let bounds = row_layout.bounds() + Vector::new(0.0, dy);
            let layer = Rectangle {
                x: bounds.x - 16.0,
                y: bounds.y - 16.0,
                width: bounds.width + 32.0,
                height: bounds.height + 32.0,
            };
            renderer.with_layer(layer, |r| {
                if let Some(lifted) = self.lifted {
                    r.fill_quad(
                        renderer::Quad {
                            bounds,
                            border: lifted.border,
                            shadow: lifted.shadow,
                            ..Default::default()
                        },
                        lifted.background,
                    );
                }
                // The row is drawn at its layout position shifted by `dy`;
                // shift the cursor back so its hover state lines up.
                let row_cursor = cursor - Vector::new(0.0, dy);
                r.with_translation(Vector::new(0.0, dy), |r| {
                    row.as_widget()
                        .draw(row_tree, r, theme, style, row_layout, row_cursor, &layer);
                });
            });
        }
    }
}

impl<'a, K, M> From<ReorderList<'a, K, M>> for Element<'a, M>
where
    K: Eq + Hash + Copy + 'static,
    M: Clone + 'a,
{
    fn from(list: ReorderList<'a, K, M>) -> Self {
        Element::new(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_slots_follow_order_and_heights() {
        let heights: HashMap<u32, f32> = [(1, 40.0), (2, 20.0), (3, 40.0)].into_iter().collect();
        assert_eq!(
            stack_slots(&[1, 2, 3], &heights, 100.0),
            vec![(1, 100.0, 140.0), (2, 140.0, 160.0), (3, 160.0, 200.0)]
        );
        // Reordered: the short row moves up and the others close the gap.
        assert_eq!(
            stack_slots(&[2, 3, 1], &heights, 100.0),
            vec![(2, 100.0, 120.0), (3, 120.0, 160.0), (1, 160.0, 200.0)]
        );
        assert_eq!(stack_slots(&[9], &heights, 0.0), vec![(9, 0.0, 0.0)]);
    }

    #[test]
    fn lifted_row_follows_cursor_within_list() {
        let list = Rectangle {
            x: 0.0,
            y: 100.0,
            width: 300.0,
            height: 120.0,
        };
        // Grabbed 10px below its top: the row keeps that offset.
        assert_eq!(lifted_top(150.0, 10.0, list, 40.0), 140.0);
        // Dragged above or below the list: pinned to its edges.
        assert_eq!(lifted_top(20.0, 10.0, list, 40.0), 100.0);
        assert_eq!(lifted_top(400.0, 10.0, list, 40.0), 180.0);
        // A row taller than the list stays pinned to the top.
        assert_eq!(lifted_top(400.0, 10.0, list, 200.0), 100.0);
    }
}

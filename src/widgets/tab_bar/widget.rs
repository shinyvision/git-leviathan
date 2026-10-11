//! Self-contained tab bar with internal drag-reorder, FLIP-style nudge
//! animation, horizontal scrolling, and click selection. The caller owns the
//! canonical tab list and reacts to `on_select(K)` for clicks, `on_reorder
//! (Vec<K>)` once a drag finishes with a changed order, and optionally
//! `on_context_menu(K, Point)` for right clicks. All drag mechanics —
//! threshold detection, hover routing, visual translation, slide-in
//! animation — and all scroll mechanics live inside the widget.
//!
//! Tabs always lay out at their natural width (labels never wrap). When the
//! strip is wider than the space it is given it scrolls horizontally: the
//! mouse wheel and trackpad scroll it, the active tab is eased into view
//! whenever it changes, dragging a tab against either edge scrolls toward
//! that edge, and soft fades mark the edges that hide more tabs.
//!
//! Coordinates: children are laid out in *content space* as if the strip
//! were scrolled to the start. A screen point `p` maps to the content point
//! `p + (offset, 0)`; drawing applies the inverse translation, the same
//! scheme iced's own `Scrollable` uses. Drag state is kept in content space.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::hash::Hash;
use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::tree::{State, Tag, Tree};
use iced::advanced::widget::{Operation, Widget};
use iced::advanced::Renderer as _;
use iced::advanced::{mouse, renderer, Clipboard, Shell};
use iced::{
    gradient,
    widget::{container, row, text},
    window, Alignment, Background, Border, Color, Element, Event, Length, Padding, Point, Radians,
    Rectangle, Renderer, Size, Theme, Vector,
};

use crate::services::text_measurement::cached_truncate_name;
use crate::style;
use crate::theme;
use crate::widgets::primitives::hoverable::{HoverStatus, Hoverable};

use super::state::{DragOutcome, DragState};
use super::tracker::PositionTracker;

/// Labels longer than this are cut with an ellipsis so one long repository
/// name cannot crowd out the rest of the strip.
const MAX_LABEL_WIDTH: f32 = 220.0;
/// Pixels scrolled per wheel "line" (mouse wheels report lines, trackpads
/// report pixels).
const WHEEL_LINE_PX: f32 = 56.0;
/// Exponential approach rate (per second) of the eased scroll offset.
const SCROLL_EASE_RATE: f32 = 18.0;
/// Space kept between a revealed tab and the strip edge, so the edge fade
/// never sits on top of the tab being revealed.
const REVEAL_MARGIN: f32 = 28.0;
/// Width of the fade drawn over an edge that hides more tabs.
const EDGE_FADE_WIDTH: f32 = 28.0;
/// Distance from an edge within which a dragged tab scrolls the strip.
const DRAG_EDGE_ZONE: f32 = 40.0;
/// Peak drag auto-scroll speed in pixels per second.
const DRAG_SCROLL_SPEED: f32 = 700.0;
/// Upper bound on a single animation step, so a stalled frame does not jump.
const MAX_FRAME_STEP: Duration = Duration::from_millis(50);
const DEFAULT_FRAME_STEP: Duration = Duration::from_millis(16);

pub struct TabItem<'a, K, M> {
    pub key: K,
    pub label: String,
    pub leading: Option<Element<'a, M>>,
    pub trailing: Option<Element<'a, M>>,
}

impl<'a, K, M> TabItem<'a, K, M> {
    pub fn new(key: K, label: impl Into<String>) -> Self {
        Self {
            key,
            label: label.into(),
            leading: None,
            trailing: None,
        }
    }

    pub fn leading(mut self, element: impl Into<Element<'a, M>>) -> Self {
        self.leading = Some(element.into());
        self
    }

    pub fn trailing(mut self, element: impl Into<Element<'a, M>>) -> Self {
        self.trailing = Some(element.into());
        self
    }
}

type ContextMenuFn<'a, K, M> = Box<dyn Fn(K, Point) -> M + 'a>;

pub struct TabBar<'a, K: Eq + Hash + Copy + 'static, M> {
    children: Vec<Element<'a, M>>,
    keys: Vec<Option<K>>,
    active: Option<K>,
    on_select: Option<Box<dyn Fn(K) -> M + 'a>>,
    on_reorder: Option<Box<dyn Fn(Vec<K>) -> M + 'a>>,
    on_context_menu: Option<ContextMenuFn<'a, K, M>>,
}

impl<'a, K: Eq + Hash + Copy + 'static, M: Clone + 'a> TabBar<'a, K, M> {
    pub fn new(items: Vec<TabItem<'a, K, M>>, active: K) -> Self {
        let mut children: Vec<Element<'a, M>> = Vec::with_capacity(items.len());
        let mut keys: Vec<Option<K>> = Vec::with_capacity(items.len());
        let mut active_present = false;
        for item in items {
            let TabItem {
                key,
                label,
                leading,
                trailing,
            } = item;
            let is_active = key == active;
            active_present |= is_active;
            children.push(tab_element(label, is_active, leading, trailing));
            keys.push(Some(key));
        }
        Self {
            children,
            keys,
            active: active_present.then_some(active),
            on_select: None,
            on_reorder: None,
            on_context_menu: None,
        }
    }

    pub fn leading_slot(mut self, element: impl Into<Element<'a, M>>) -> Self {
        self.children.insert(0, element.into());
        self.keys.insert(0, None);
        self
    }

    pub fn trailing_slot(mut self, element: impl Into<Element<'a, M>>) -> Self {
        self.children.push(element.into());
        self.keys.push(None);
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

    /// Right click on a tab. Receives the tab key and the cursor position in
    /// window coordinates, ready to anchor a context menu.
    pub fn on_context_menu(mut self, f: impl Fn(K, Point) -> M + 'a) -> Self {
        self.on_context_menu = Some(Box::new(f));
        self
    }

    fn tab_input_order(&self) -> Vec<K> {
        self.keys.iter().filter_map(|k| *k).collect()
    }

    /// First tab's layout x (start of the tab strip) and per-key tab width
    /// from the input-order layout. Widths are stable across reorders since
    /// children are always laid out in caller order.
    fn strip_metrics(&self, layout: Layout<'_>) -> (Option<f32>, HashMap<K, f32>) {
        let mut widths = HashMap::new();
        let mut start = None;
        for (child_layout, key) in layout.children().zip(self.keys.iter()) {
            if let Some(k) = key {
                let b = child_layout.bounds();
                if start.is_none() {
                    start = Some(b.x);
                }
                widths.insert(*k, b.width);
            }
        }
        (start, widths)
    }

    /// `(key, left_x, right_x)` per tab as it would lay out in `working_order`
    /// using each tab's actual width. The dragged tab's width therefore
    /// determines the gap left behind, regardless of which tab(s) it
    /// displaces.
    fn working_slots(&self, working_order: Option<&[K]>, layout: Layout<'_>) -> Vec<(K, f32, f32)> {
        let (Some(start), widths) = self.strip_metrics(layout) else {
            return Vec::new();
        };
        let order: Vec<K> = match working_order {
            Some(o) => o.to_vec(),
            None => self.tab_input_order(),
        };
        let mut out = Vec::with_capacity(order.len());
        let mut x = start;
        for k in order {
            let w = widths.get(&k).copied().unwrap_or(0.0);
            out.push((k, x, x + w));
            x += w;
        }
        out
    }

    fn working_slot_spans(
        &self,
        working_order: Option<&[K]>,
        layout: Layout<'_>,
    ) -> Vec<(f32, f32)> {
        self.working_slots(working_order, layout)
            .into_iter()
            .map(|(_, l, r)| (l, r))
            .collect()
    }

    /// Tab whose input-order layout bounds contain the (content-space)
    /// cursor, plus its slot's left edge (used as the grab anchor). Press
    /// hit-testing — not drag routing — so we use input-order bounds.
    fn pressed_tab_at(&self, cursor: Point, layout: Layout<'_>) -> Option<(K, f32)> {
        if !cursor.x.is_finite() {
            return None;
        }
        for (child_layout, key) in layout.children().zip(self.keys.iter()) {
            if let Some(k) = key {
                let b = child_layout.bounds();
                if cursor.x >= b.x && cursor.x <= b.x + b.width && b.contains(cursor) {
                    return Some((*k, b.x));
                }
            }
        }
        None
    }

    fn simulated_positions(
        &self,
        working_order: Option<&[K]>,
        layout: Layout<'_>,
    ) -> HashMap<K, f32> {
        self.working_slots(working_order, layout)
            .into_iter()
            .map(|(k, l, _)| (k, l))
            .collect()
    }

    /// Feed a (content-space) cursor position to the drag state machine.
    /// Returns true when the working order changed.
    fn drive_drag(
        &self,
        state: &mut TabBarState<K>,
        content: Point,
        layout: Layout<'_>,
        now: Instant,
    ) -> bool {
        let working = state.drag.working_order().map(|s| s.to_vec());
        let slots = self.working_slot_spans(working.as_deref(), layout);
        let TabBarState { drag, tracker, .. } = state;
        drag.on_cursor_moved(content, &slots, |k| tracker.is_animating_key(k, now))
    }
}

/// Horizontal scroll position of the strip.
struct ScrollState<K> {
    /// Offset currently drawn.
    offset: f32,
    /// Offset `offset` eases toward.
    target: f32,
    /// Timestamp of the previous easing / auto-scroll step.
    last_frame: Option<Instant>,
    /// `(active key, tab count)` the strip last scrolled into view. A change
    /// in either (tab switch, tab opened) reveals the active tab again.
    revealed: Option<(Option<K>, usize)>,
    /// Last cursor position in window coordinates while a press or drag is
    /// tracked. Lets edge auto-scroll keep moving the dragged tab while the
    /// mouse itself is still.
    screen_cursor: Option<Point>,
    /// Tab keys of the previous layout. When none of them survive (the strip
    /// now shows a different workspace) the old scroll position means
    /// nothing, so the new active tab is revealed without easing.
    last_keys: Vec<K>,
}

impl<K> Default for ScrollState<K> {
    fn default() -> Self {
        Self {
            offset: 0.0,
            target: 0.0,
            last_frame: None,
            revealed: None,
            screen_cursor: None,
            last_keys: Vec::new(),
        }
    }
}

impl<K> ScrollState<K> {
    fn translation(&self) -> Vector {
        Vector::new(self.offset, 0.0)
    }

    fn clamp(&mut self, max: f32) {
        self.offset = self.offset.clamp(0.0, max);
        self.target = self.target.clamp(0.0, max);
    }

    fn is_settled(&self) -> bool {
        (self.target - self.offset).abs() < 0.5
    }

    fn frame_step(&mut self, now: Instant) -> f32 {
        let step = self
            .last_frame
            .map(|last| now.saturating_duration_since(last))
            .unwrap_or(DEFAULT_FRAME_STEP)
            .min(MAX_FRAME_STEP);
        self.last_frame = Some(now);
        step.as_secs_f32()
    }
}

struct TabBarState<K: Eq + Hash + Copy + 'static> {
    drag: DragState<K>,
    tracker: PositionTracker<K>,
    scroll: ScrollState<K>,
}

impl<K: Eq + Hash + Copy + 'static> Default for TabBarState<K> {
    fn default() -> Self {
        Self {
            drag: DragState::new(),
            tracker: PositionTracker::new(),
            scroll: ScrollState::default(),
        }
    }
}

/// Width of everything laid out in the strip, measured from its left edge.
fn content_width(layout: Layout<'_>) -> f32 {
    let left = layout.bounds().x;
    layout
        .children()
        .map(|child| {
            let b = child.bounds();
            b.x + b.width - left
        })
        .fold(0.0, f32::max)
}

fn max_offset(layout: Layout<'_>) -> f32 {
    (content_width(layout) - layout.bounds().width).max(0.0)
}

/// Offset that brings the content span `[left, right]` (relative to the
/// strip start) fully into a `viewport`-wide window, moving as little as
/// possible from `current`.
fn reveal_offset(current: f32, left: f32, right: f32, viewport: f32, max: f32) -> f32 {
    let margin = REVEAL_MARGIN.min(((viewport - (right - left)) / 2.0).max(0.0));
    let target = if left - margin < current {
        left - margin
    } else if right + margin > current + viewport {
        right + margin - viewport
    } else {
        current
    };
    target.clamp(0.0, max)
}

impl<'a, K, M> Widget<M, Theme, Renderer> for TabBar<'a, K, M>
where
    K: Eq + Hash + Copy + 'static,
    M: Clone + 'a,
{
    fn tag(&self) -> Tag {
        Tag::of::<TabBarState<K>>()
    }

    fn state(&self) -> State {
        State::new(TabBarState::<K>::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(theme::TAB_HEIGHT as f32))
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let height = theme::TAB_HEIGHT as f32;
        let size = limits.resolve(Length::Fill, Length::Fixed(height), Size::ZERO);
        // Unbounded width: every tab takes its natural width and the strip
        // scrolls instead of squeezing (and wrapping) the labels.
        let child_limits = layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, height));

        let mut x = 0.0;
        let mut active_span = None;
        let nodes: Vec<layout::Node> = self
            .children
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(self.keys.iter())
            .map(|((child, child_tree), key)| {
                let node = child
                    .as_widget_mut()
                    .layout(child_tree, renderer, &child_limits);
                let child_size = node.size();
                let node =
                    node.move_to(Point::new(x, ((height - child_size.height) / 2.0).max(0.0)));
                if key.is_some() && *key == self.active {
                    active_span = Some((x, x + child_size.width));
                }
                x += child_size.width;
                node
            })
            .collect();

        // Keep the scroll position valid for the new content (tab closed,
        // window resized) and reveal the active tab when it changed. Doing it
        // here rather than on the next redraw means a newly opened tab never
        // flashes at the old scroll position.
        let state: &mut TabBarState<K> = tree.state.downcast_mut();
        let max = (x - size.width).max(0.0);
        state.scroll.clamp(max);
        let keys: Vec<K> = self.keys.iter().flatten().copied().collect();
        let replaced = !keys.is_empty() && !keys.iter().any(|k| state.scroll.last_keys.contains(k));
        let reveal_key = (self.active, keys.len());
        // Hold the strip still from press to release: a scroll between press
        // and threshold would read as pointer motion and turn a click into a
        // drag. The reveal runs on the next layout instead.
        if state.scroll.revealed != Some(reveal_key) && !state.drag.is_tracking_pointer() {
            if let Some((left, right)) = active_span {
                let snap = state.scroll.revealed.is_none() || replaced;
                let from = if replaced { 0.0 } else { state.scroll.target };
                let target = reveal_offset(from, left, right, size.width, max);
                state.scroll.target = target;
                if snap {
                    state.scroll.offset = target;
                }
            }
            state.scroll.revealed = Some(reveal_key);
        }
        state.scroll.last_keys = keys;

        layout::Node::with_children(size, nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        for ((child, child_tree), child_layout) in self
            .children
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            child
                .as_widget_mut()
                .operate(child_tree, child_layout, renderer, operation);
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
        let bounds = layout.bounds();
        let max = max_offset(layout);

        // A drag in progress owns the next ButtonReleased: if a child captured
        // it (e.g. a tab-embedded button), `on_released` would never run and
        // the dragged tab would stay glued to the cursor. Finalize the drag
        // here before deferring to children in that case.
        {
            let state: &mut TabBarState<K> = tree.state.downcast_mut();
            state.scroll.clamp(max);
            if state.drag.is_dragging()
                && matches!(
                    event,
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                )
            {
                state.scroll.screen_cursor = None;
                match state.drag.on_released() {
                    DragOutcome::None | DragOutcome::Click(_) => {}
                    DragOutcome::Reorder(order) => {
                        if let Some(f) = self.on_reorder.as_ref() {
                            shell.publish(f(order));
                        }
                    }
                }
                shell.capture_event();
                shell.request_redraw();
                return;
            }

            // Wheel / trackpad scrolling. Vertical wheel motion maps onto the
            // horizontal axis so a plain mouse wheel scrolls the strip.
            if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
                let pressed = state.drag.is_tracking_pointer() && !state.drag.is_dragging();
                if max > 0.0 && cursor.is_over(bounds) && !pressed {
                    let (amount, smooth) = match *delta {
                        mouse::ScrollDelta::Lines { x, y } => {
                            let lines = if x.abs() > y.abs() { -x } else { -y };
                            (lines * WHEEL_LINE_PX, true)
                        }
                        mouse::ScrollDelta::Pixels { x, y } => {
                            (if x.abs() > y.abs() { -x } else { -y }, false)
                        }
                    };
                    let scroll = &mut state.scroll;
                    scroll.target = (scroll.target + amount).clamp(0.0, max);
                    if !smooth {
                        // Trackpads already deliver smooth, high-rate deltas;
                        // easing on top of them only adds lag.
                        scroll.offset = scroll.target;
                    }
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
            }
        }

        let translation = tree
            .state
            .downcast_ref::<TabBarState<K>>()
            .scroll
            .translation();
        // Children only see the cursor while it is over the visible strip, so
        // a tab scrolled out of view never reacts to a cursor that is really
        // over a neighbouring widget.
        let content_cursor = if cursor.is_over(bounds) {
            cursor + translation
        } else {
            mouse::Cursor::Unavailable
        };
        let content_viewport = Rectangle {
            x: bounds.x + translation.x,
            ..bounds
        };

        for ((child, child_tree), child_layout) in self
            .children
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                child_tree,
                event,
                child_layout,
                content_cursor,
                renderer,
                clipboard,
                shell,
                &content_viewport,
            );
            if shell.is_event_captured() {
                return;
            }
        }
        let _ = viewport;

        let state: &mut TabBarState<K> = tree.state.downcast_mut();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if state.drag.is_tracking_pointer() {
                    state.drag.cancel();
                }
                let Some(content) = content_cursor.position() else {
                    return;
                };
                if let Some((key, left)) = self.pressed_tab_at(content, layout) {
                    let order = self.tab_input_order();
                    state.drag.on_press(key, content, left, &order);
                    state.scroll.screen_cursor = cursor.position();
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let (Some(screen), Some(content)) = (cursor.position(), content_cursor.position())
                else {
                    return;
                };
                if let (Some(f), Some((key, _))) = (
                    self.on_context_menu.as_ref(),
                    self.pressed_tab_at(content, layout),
                ) {
                    shell.publish(f(key, screen));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                // Nothing to do unless a press is pending or a drag is active —
                // bail before the per-move slot allocations, which otherwise
                // run on every mouse move anywhere in the window.
                if !state.drag.is_tracking_pointer() {
                    return;
                }
                let Some(screen) = cursor.position() else {
                    return;
                };
                state.scroll.screen_cursor = Some(screen);
                let content = screen + translation;
                let changed = self.drive_drag(state, content, layout, Instant::now());
                if changed {
                    shell.invalidate_layout();
                    shell.request_redraw();
                } else if state.drag.is_dragging() {
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.scroll.screen_cursor = None;
                let outcome = state.drag.on_released();
                match outcome {
                    DragOutcome::None => {}
                    DragOutcome::Click(k) => {
                        if let Some(f) = self.on_select.as_ref() {
                            shell.publish(f(k));
                            shell.capture_event();
                        }
                    }
                    DragOutcome::Reorder(order) => {
                        if let Some(f) = self.on_reorder.as_ref() {
                            shell.publish(f(order));
                            shell.capture_event();
                        }
                    }
                }
                shell.request_redraw();
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                state
                    .drag
                    .clear_committed_if_matches(&self.tab_input_order());
                let working = state.drag.working_order().map(|s| s.to_vec());
                let sim = self.simulated_positions(working.as_deref(), layout);
                state.tracker.settle(&sim, *now);

                let mut scrolled = false;
                if state.drag.is_dragging() {
                    // Edge auto-scroll: the closer the cursor is to (or the
                    // further past) an edge, the faster the strip scrolls.
                    let step = state.scroll.frame_step(*now);
                    if let Some(screen) = state.scroll.screen_cursor {
                        let left_zone = bounds.x + DRAG_EDGE_ZONE;
                        let right_zone = bounds.x + bounds.width - DRAG_EDGE_ZONE;
                        let pressure = if screen.x < left_zone {
                            -((left_zone - screen.x) / DRAG_EDGE_ZONE).min(1.5)
                        } else if screen.x > right_zone {
                            ((screen.x - right_zone) / DRAG_EDGE_ZONE).min(1.5)
                        } else {
                            0.0
                        };
                        let before = state.scroll.offset;
                        let next = (before + pressure * DRAG_SCROLL_SPEED * step).clamp(0.0, max);
                        if (next - before).abs() > f32::EPSILON {
                            state.scroll.offset = next;
                            state.scroll.target = next;
                            scrolled = true;
                        }
                    }
                    // Re-run drag routing even when the mouse is still: the
                    // strip may have scrolled under it, or a displaced tab's
                    // nudge animation may have finished and unblocked a swap.
                    let content = state
                        .scroll
                        .screen_cursor
                        .map(|p| p + state.scroll.translation())
                        .or_else(|| state.drag.cursor());
                    if let Some(content) = content {
                        if self.drive_drag(state, content, layout, *now) {
                            shell.invalidate_layout();
                        }
                    }
                    shell.request_redraw();
                } else if state.drag.is_tracking_pointer() {
                    // Pressed, not yet dragging: hold still (see `layout`).
                    state.scroll.last_frame = None;
                } else if !state.scroll.is_settled() {
                    let step = state.scroll.frame_step(*now);
                    let scroll = &mut state.scroll;
                    let approach = 1.0 - (-SCROLL_EASE_RATE * step).exp();
                    scroll.offset += (scroll.target - scroll.offset) * approach;
                    if scroll.is_settled() {
                        scroll.offset = scroll.target;
                    }
                    scrolled = true;
                } else {
                    state.scroll.last_frame = None;
                }

                if scrolled || state.tracker.is_animating(*now) {
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
        let state: &TabBarState<K> = tree.state.downcast_ref();
        if state.drag.is_dragging() {
            return mouse::Interaction::Grabbing;
        }
        let bounds = layout.bounds();
        if !cursor.is_over(bounds) {
            return mouse::Interaction::None;
        }
        let translation = state.scroll.translation();
        let content_cursor = cursor + translation;
        let content_viewport = Rectangle {
            x: bounds.x + translation.x,
            ..bounds
        };
        let _ = viewport;
        let mut best = mouse::Interaction::None;
        for ((child, child_tree), child_layout) in self
            .children
            .iter()
            .zip(tree.children.iter())
            .zip(layout.children())
        {
            let inner = child.as_widget().mouse_interaction(
                child_tree,
                child_layout,
                content_cursor,
                &content_viewport,
                renderer,
            );
            if inner != mouse::Interaction::None {
                best = inner;
            }
        }
        if best == mouse::Interaction::None {
            if let Some(p) = content_cursor.position() {
                let slots = self.working_slots(None, layout);
                if slots.iter().any(|(_, l, r)| p.x >= *l && p.x <= *r) {
                    return mouse::Interaction::Pointer;
                }
            }
        }
        best
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style_default: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // Background + outer border are painted by the surrounding
        // `tab_bar_view` container; widget only renders tabs themselves.
        let state: &TabBarState<K> = tree.state.downcast_ref();
        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };
        let max = max_offset(layout);
        let offset = state.scroll.offset.clamp(0.0, max);
        let translation = Vector::new(offset, 0.0);
        let content_cursor = if cursor.is_over(bounds) {
            cursor + translation
        } else {
            mouse::Cursor::Unavailable
        };
        let content_viewport = Rectangle {
            x: clip.x + offset,
            ..clip
        };

        let now = Instant::now();
        let working = state.drag.working_order().map(|s| s.to_vec());
        let sim = self.simulated_positions(working.as_deref(), layout);
        let cursor_x = state.drag.cursor().map(|p| p.x);
        let grab_offset = state.drag.grab_offset();
        let dragging_key = state.drag.dragging_key();

        renderer.with_layer(clip, |renderer| {
            renderer.with_translation(Vector::new(-offset, 0.0), |renderer| {
                for (((child, child_tree), child_layout), key) in self
                    .children
                    .iter()
                    .zip(tree.children.iter())
                    .zip(layout.children())
                    .zip(self.keys.iter())
                {
                    let child_bounds = child_layout.bounds();
                    let layout_x = child_bounds.x;
                    let is_dragged = matches!(key, Some(k) if Some(*k) == dragging_key);
                    let translate_x = match key {
                        Some(_) if is_dragged => match (cursor_x, grab_offset) {
                            (Some(cx), Some(g)) => cx - g - layout_x,
                            _ => 0.0,
                        },
                        Some(k) => {
                            let target_x = sim.get(k).copied().unwrap_or(layout_x);
                            let anim_offset = state.tracker.offset_for(*k, now);
                            target_x + anim_offset - layout_x
                        }
                        None => 0.0,
                    };

                    // Skip tabs scrolled entirely out of view.
                    let drawn = Rectangle {
                        x: child_bounds.x + translate_x,
                        ..child_bounds
                    };
                    if !is_dragged && drawn.intersection(&content_viewport).is_none() {
                        continue;
                    }

                    let draw_inner = |renderer: &mut Renderer| {
                        if translate_x.abs() >= 0.5 {
                            renderer.with_translation(Vector::new(translate_x, 0.0), |r| {
                                child.as_widget().draw(
                                    child_tree,
                                    r,
                                    theme,
                                    style_default,
                                    child_layout,
                                    content_cursor,
                                    &content_viewport,
                                );
                            });
                        } else {
                            child.as_widget().draw(
                                child_tree,
                                renderer,
                                theme,
                                style_default,
                                child_layout,
                                content_cursor,
                                &content_viewport,
                            );
                        }
                    };

                    if is_dragged {
                        renderer.with_layer(content_viewport, draw_inner);
                    } else {
                        draw_inner(renderer);
                    }
                }
            });

            // Fades over edges that hide more tabs. Inset by the container's
            // 1px border so its top/bottom lines stay crisp.
            let fade_bounds = |x: f32| Rectangle {
                x,
                y: bounds.y + 1.0,
                width: EDGE_FADE_WIDTH.min(bounds.width / 2.0),
                height: (bounds.height - 2.0).max(0.0),
            };
            let solid = theme::BG_TOOLBAR;
            let clear = Color { a: 0.0, ..solid };
            if offset > 0.5 {
                fill_gradient(renderer, fade_bounds(bounds.x), solid, clear);
            }
            if offset < max - 0.5 {
                let width = EDGE_FADE_WIDTH.min(bounds.width / 2.0);
                fill_gradient(
                    renderer,
                    fade_bounds(bounds.x + bounds.width - width),
                    clear,
                    solid,
                );
            }
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, M, Theme, Renderer>> {
        let offset = tree.state.downcast_ref::<TabBarState<K>>().scroll.offset;
        let translation = translation - Vector::new(offset, 0.0);
        let mut overlays: Vec<_> = self
            .children
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
            .filter_map(|((child, child_tree), child_layout)| {
                child.as_widget_mut().overlay(
                    child_tree,
                    child_layout,
                    renderer,
                    viewport,
                    translation,
                )
            })
            .collect();
        overlays.pop()
    }
}

/// Fill `bounds` with a left-to-right gradient from `from` to `to`.
fn fill_gradient(renderer: &mut Renderer, bounds: Rectangle, from: Color, to: Color) {
    let gradient = gradient::Linear::new(Radians(FRAC_PI_2))
        .add_stop(0.0, from)
        .add_stop(1.0, to);
    renderer.fill_quad(
        renderer::Quad {
            bounds,
            border: Border::default(),
            ..Default::default()
        },
        Background::Gradient(gradient.into()),
    );
}

impl<'a, K, M> From<TabBar<'a, K, M>> for Element<'a, M>
where
    K: Eq + Hash + Copy + 'static,
    M: Clone + 'a,
{
    fn from(widget: TabBar<'a, K, M>) -> Self {
        Element::new(widget)
    }
}

fn tab_element<'a, M>(
    label: String,
    active: bool,
    leading: Option<Element<'a, M>>,
    trailing: Option<Element<'a, M>>,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    let mut row_children: Vec<Element<'a, M>> = Vec::new();
    if let Some(l) = leading {
        row_children.push(l);
    }
    row_children.push(
        text(cached_truncate_name(&label, MAX_LABEL_WIDTH))
            .size(theme::FONT_SM)
            .wrapping(text::Wrapping::None)
            .style(if active {
                style::primary_text
            } else {
                style::secondary_text
            })
            .into(),
    );
    if let Some(t) = trailing {
        row_children.push(t);
    }

    let inner = container(row(row_children).spacing(4).align_y(Alignment::Center))
        .height(Length::Fill)
        .padding(Padding::from([0, 10]))
        .align_y(iced::alignment::Vertical::Center);

    let tab_border = Border {
        color: theme::BORDER,
        width: 1.0,
        radius: 0.0.into(),
    };

    let visual: Element<'a, M> = if active {
        inner
            .style(move |_: &Theme| iced::widget::container::Style {
                background: Some(theme::BG_HOVER.into()),
                border: tab_border,
                ..Default::default()
            })
            .into()
    } else {
        Hoverable::new(inner, move |_: &Theme, status: HoverStatus| {
            let bg = if status.is_hovered() {
                theme::BG_HOVER
            } else {
                theme::BG_BASE
            };
            iced::widget::container::Style {
                background: Some(bg.into()),
                border: tab_border,
                ..Default::default()
            }
        })
        .into()
    };

    container(visual)
        .height(Length::Fixed(theme::TAB_HEIGHT as f32))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_keeps_visible_tab_in_place() {
        // Tab at 100..200 inside a 400px window scrolled to 50: already
        // visible with margin to spare.
        assert_eq!(reveal_offset(50.0, 100.0, 200.0, 400.0, 1000.0), 50.0);
    }

    #[test]
    fn reveal_scrolls_right_with_margin() {
        // Tab at 900..1000 in a 400px window at offset 0 → right edge plus
        // margin lands on the window's right edge.
        let target = reveal_offset(0.0, 900.0, 1000.0, 400.0, 1000.0);
        assert_eq!(target, 1000.0 + REVEAL_MARGIN - 400.0);
    }

    #[test]
    fn reveal_scrolls_left_with_margin() {
        let target = reveal_offset(500.0, 100.0, 200.0, 400.0, 1000.0);
        assert_eq!(target, 100.0 - REVEAL_MARGIN);
    }

    #[test]
    fn reveal_clamps_to_scroll_range() {
        assert_eq!(reveal_offset(500.0, 0.0, 100.0, 400.0, 1000.0), 0.0);
        assert_eq!(reveal_offset(0.0, 1300.0, 1400.0, 400.0, 1000.0), 1000.0);
    }

    #[test]
    fn reveal_margin_shrinks_for_tabs_wider_than_window() {
        // A tab nearly as wide as the window gets whatever margin fits.
        let target = reveal_offset(0.0, 600.0, 990.0, 400.0, 2000.0);
        assert!((590.0..=600.0).contains(&target));
    }
}

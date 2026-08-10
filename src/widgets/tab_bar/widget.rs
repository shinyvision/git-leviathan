//! Self-contained tab bar with internal drag-reorder, FLIP-style nudge
//! animation, and click selection. The caller owns the canonical tab list
//! and reacts to two messages: `on_select(K)` for clicks and `on_reorder
//! (Vec<K>)` once a drag finishes with a changed order. All drag mechanics —
//! threshold detection, hover routing, visual translation, slide-in animation
//! — live inside the widget.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::Instant;

use iced::advanced::layout::{self, flex, Layout};
use iced::advanced::widget::tree::{State, Tag, Tree};
use iced::advanced::widget::{Operation, Widget};
use iced::advanced::Renderer as _;
use iced::advanced::{mouse, renderer, Clipboard, Shell};
use iced::{
    widget::{container, row, text},
    window, Alignment, Border, Element, Event, Length, Padding, Point, Rectangle, Renderer, Size,
    Theme, Vector,
};

use crate::style;
use crate::theme;
use crate::widgets::primitives::hoverable::{HoverStatus, Hoverable};
use crate::widgets::shared::horizontal_space;

use super::state::{DragOutcome, DragState};
use super::tracker::PositionTracker;

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

pub struct TabBar<'a, K: Eq + Hash + Copy + 'static, M> {
    children: Vec<Element<'a, M>>,
    keys: Vec<Option<K>>,
    on_select: Option<Box<dyn Fn(K) -> M + 'a>>,
    on_reorder: Option<Box<dyn Fn(Vec<K>) -> M + 'a>>,
}

impl<'a, K: Eq + Hash + Copy + 'static, M: Clone + 'a> TabBar<'a, K, M> {
    pub fn new(items: Vec<TabItem<'a, K, M>>, active: K) -> Self {
        let mut children: Vec<Element<'a, M>> = Vec::with_capacity(items.len() + 1);
        let mut keys: Vec<Option<K>> = Vec::with_capacity(items.len() + 1);
        for item in items {
            let TabItem {
                key,
                label,
                leading,
                trailing,
            } = item;
            children.push(tab_element(label, key == active, leading, trailing));
            keys.push(Some(key));
        }
        children.push(horizontal_space().into());
        keys.push(None);
        Self {
            children,
            keys,
            on_select: None,
            on_reorder: None,
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

    /// Tab whose input-order layout bounds contain the cursor, plus its
    /// slot's left edge (used as the grab anchor). Press hit-testing — not
    /// drag routing — so we use input-order bounds.
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
}

struct TabBarState<K: Eq + Hash + Copy + 'static> {
    drag: DragState<K>,
    tracker: PositionTracker<K>,
}

impl<K: Eq + Hash + Copy + 'static> Default for TabBarState<K> {
    fn default() -> Self {
        Self {
            drag: DragState::new(),
            tracker: PositionTracker::new(),
        }
    }
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
        flex::resolve(
            flex::Axis::Horizontal,
            renderer,
            limits,
            Length::Fill,
            Length::Fixed(theme::TAB_HEIGHT as f32),
            Padding::ZERO,
            0.0,
            Alignment::Center,
            &mut self.children,
            &mut tree.children,
        )
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
        // A drag in progress owns the next ButtonReleased: if a child captured
        // it (e.g. a tab-embedded button), `on_released` would never run and
        // the dragged tab would stay glued to the cursor. Finalize the drag
        // here before deferring to children in that case.
        {
            let dragging = {
                let state: &mut TabBarState<K> = tree.state.downcast_mut();
                state.drag.is_dragging()
            };
            if dragging
                && matches!(
                    event,
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                )
            {
                let state: &mut TabBarState<K> = tree.state.downcast_mut();
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
        }

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
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
            if shell.is_event_captured() {
                return;
            }
        }

        let state: &mut TabBarState<K> = tree.state.downcast_mut();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(pos) = cursor.position() else {
                    return;
                };
                if let Some((key, left)) = self.pressed_tab_at(pos, layout) {
                    let order = self.tab_input_order();
                    state.drag.on_press(key, pos, left, &order);
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
                let Some(pos) = cursor.position() else {
                    return;
                };
                let working = state.drag.working_order().map(|s| s.to_vec());
                let slots: Vec<(f32, f32)> = self
                    .working_slots(working.as_deref(), layout)
                    .into_iter()
                    .map(|(_, l, r)| (l, r))
                    .collect();
                let now_inst = Instant::now();
                let TabBarState { drag, tracker } = &mut *state;
                let changed =
                    drag.on_cursor_moved(pos, &slots, |k| tracker.is_animating_key(k, now_inst));
                if changed {
                    shell.invalidate_layout();
                    shell.request_redraw();
                } else if state.drag.is_dragging() {
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
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

                if state.drag.is_dragging() {
                    if let Some(cur) = state.drag.cursor() {
                        let working2 = state.drag.working_order().map(|s| s.to_vec());
                        let slots: Vec<(f32, f32)> = self
                            .working_slots(working2.as_deref(), layout)
                            .into_iter()
                            .map(|(_, l, r)| (l, r))
                            .collect();
                        let TabBarState { drag, tracker } = &mut *state;
                        let now_copy = *now;
                        let changed = drag.on_cursor_moved(cur, &slots, |k| {
                            tracker.is_animating_key(k, now_copy)
                        });
                        if changed {
                            shell.invalidate_layout();
                            shell.request_redraw();
                        }
                    }
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
        let state: &TabBarState<K> = tree.state.downcast_ref();
        if state.drag.is_dragging() {
            return mouse::Interaction::Grabbing;
        }
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
                cursor,
                viewport,
                renderer,
            );
            if inner != mouse::Interaction::None {
                best = inner;
            }
        }
        if best == mouse::Interaction::None
            && cursor
                .position()
                .map(|p| layout.bounds().contains(p))
                .unwrap_or(false)
        {
            let slots = self.working_slots(None, layout);
            if let Some(p) = cursor.position() {
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
        let now = Instant::now();
        let working = state.drag.working_order().map(|s| s.to_vec());
        let sim = self.simulated_positions(working.as_deref(), layout);
        let cursor_x = state.drag.cursor().map(|p| p.x);
        let grab_offset = state.drag.grab_offset();
        let dragging_key = state.drag.dragging_key();

        for (((child, child_tree), child_layout), key) in self
            .children
            .iter()
            .zip(tree.children.iter())
            .zip(layout.children())
            .zip(self.keys.iter())
        {
            let layout_x = child_layout.bounds().x;
            let translate_x = match key {
                Some(k) if Some(*k) == dragging_key => match (cursor_x, grab_offset) {
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
            let needs_translation = translate_x.abs() >= 0.5;
            let needs_layer = matches!(key, Some(k) if Some(*k) == dragging_key);

            let draw_inner = |renderer: &mut Renderer| {
                if needs_translation {
                    renderer.with_translation(Vector::new(translate_x, 0.0), |r| {
                        child.as_widget().draw(
                            child_tree,
                            r,
                            theme,
                            style_default,
                            child_layout,
                            cursor,
                            viewport,
                        );
                    });
                } else {
                    child.as_widget().draw(
                        child_tree,
                        renderer,
                        theme,
                        style_default,
                        child_layout,
                        cursor,
                        viewport,
                    );
                }
            };

            if needs_layer {
                renderer.with_layer(*viewport, draw_inner);
            } else {
                draw_inner(renderer);
            }
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, M, Theme, Renderer>> {
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
        text(label)
            .size(theme::FONT_SM)
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

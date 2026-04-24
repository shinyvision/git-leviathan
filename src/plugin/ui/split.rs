//! Resizable split container.
//!
//! Lua plugins declare `{kind="resizable_split", id, direction, children}`.
//! Rust holds drag state + per-split pixel sizes. Children before the last are
//! rendered with `Length::Fixed`; the last absorbs remainder via `Length::Fill`.
//! Drag divider `i` adjusts `sizes[i]` (grows) and, if `i+1 < sizes.len()`,
//! shrinks `sizes[i+1]` to compensate.

use iced::{
    mouse,
    widget::{column, container, row, MouseArea, Space},
    Background, Element, Length, Theme,
};
use serde_json::Value;

use crate::message::Message;
use crate::plugin::message::PluginMessage;
use crate::theme;

use super::super::bridge::widget_tree::{self, BuildCtx, DispatchScope};

pub const DIVIDER_THICKNESS: f32 = 4.0;
pub const MIN_PANEL_SIZE: f32 = 80.0;
pub const MAX_PANEL_SIZE: f32 = 4000.0;
pub const DEFAULT_PANEL_SIZE: f32 = 300.0;

/// Convert a plugin-declared split into an iced element tree.
/// The host owns `split_states`; this function only reads current sizes.
pub fn build(node: &Value, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    // `resizable_split` really only makes sense inside a screen. Emit a
    // placeholder rather than a confusing split elsewhere — but keep the
    // widget_tree contract (return an Element) so plugins don't crash.
    if !matches!(ctx.scope, DispatchScope::Screen { .. }) {
        return Space::new().into();
    }

    let id = node
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("split")
        .to_string();
    let direction = node
        .get("direction")
        .and_then(Value::as_str)
        .unwrap_or("horizontal")
        .to_string();
    let children_nodes: Vec<&Value> = node
        .get("children")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();

    let n = children_nodes.len();
    if n == 0 {
        return Space::new().width(Length::Fill).height(Length::Fill).into();
    }
    if n == 1 {
        return widget_tree::build(children_nodes[0], ctx);
    }

    let is_vertical = direction == "vertical";
    let split_key = format!("{}:{}:{}", ctx.plugin_id, ctx.scope.storage_key(), id);
    // One size per child (length `n`). All children render as
    // `Length::FillPortion` so both edges of every child are clamp-able —
    // this is what lets the last panel honour its own `min_*`/`max_*` (if the
    // last child were `Length::Fill`, divider N-2 could push it arbitrarily
    // small with no feedback loop).
    let sizes = ctx
        .split_states
        .get(&split_key)
        .cloned()
        .unwrap_or_else(|| vec![DEFAULT_PANEL_SIZE; n]);

    // Per-child size limits: plugins declare `min_width/max_width` (or
    // `_height` for vertical splits) on each child's `container`. Limits are
    // captured at drag-begin and clamped every move in `apply_drag_delta`.
    let limits: Vec<(f32, f32)> = children_nodes
        .iter()
        .map(|c| widget_tree::container_size_limits(c, is_vertical))
        .collect();

    let mut items: Vec<Element<'static, Message>> = Vec::with_capacity(n * 2 - 1);

    for (i, child_node) in children_nodes.iter().enumerate() {
        let built: Element<'static, Message> = widget_tree::build(child_node, ctx);
        let size = sizes.get(i).copied().unwrap_or(DEFAULT_PANEL_SIZE);
        let portion = (size as u16).max(1);
        let wrapped: Element<'static, Message> = if is_vertical {
            container(built)
                .width(Length::Fill)
                .height(Length::FillPortion(portion))
                .into()
        } else {
            container(built)
                .width(Length::FillPortion(portion))
                .height(Length::Fill)
                .into()
        };
        items.push(wrapped);
        if i + 1 < n {
            let is_dragging_this = ctx
                .active_drag
                .map(|(k, idx)| k == split_key && idx == i)
                .unwrap_or(false);
            items.push(divider(
                split_key.clone(),
                i,
                n,
                is_vertical,
                is_dragging_this,
                limits.clone(),
            ));
        }
    }

    if is_vertical {
        column(items).width(Length::Fill).height(Length::Fill).into()
    } else {
        row(items).width(Length::Fill).height(Length::Fill).into()
    }
}

fn divider(
    split_key: String,
    index: usize,
    child_count: usize,
    is_vertical: bool,
    is_dragging_this: bool,
    limits: Vec<(f32, f32)>,
) -> Element<'static, Message> {
    let bar: Element<Message> = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(if is_vertical {
            Length::Fill
        } else {
            Length::Fixed(DIVIDER_THICKNESS)
        })
        .height(if is_vertical {
            Length::Fixed(DIVIDER_THICKNESS)
        } else {
            Length::Fill
        })
        .style(move |_: &Theme| container::Style {
            background: if is_dragging_this {
                Some(Background::Color(theme::ACCENT_BLUE))
            } else {
                None
            },
            ..Default::default()
        })
        .into();

    let cursor = if is_vertical {
        mouse::Interaction::ResizingVertically
    } else {
        mouse::Interaction::ResizingHorizontally
    };

    MouseArea::new(bar)
        .on_press(Message::Plugin(PluginMessage::SplitDragBegin {
            split_key,
            divider_index: index,
            child_count,
            is_vertical,
            limits,
        }))
        .interaction(cursor)
        .into()
}

/// Apply a drag delta to a split's stored sizes. `divider_index` identifies
/// which divider moved (valid: `0..sizes.len()-1`). `delta` is pixels (positive
/// = right/down). `limits` holds `(min, max)` for every child — length matches
/// `sizes`.
///
/// Both children on either side of the divider clamp against their own
/// `(min, max)`. If the neighbour's clamp absorbs less delta than we asked for,
/// the current side is reconciled so `sizes[i] + sizes[i+1]` stays conserved —
/// meaning neither panel shrinks past another's min by going around it.
pub fn apply_drag_delta(
    initial_sizes: &[f32],
    divider_index: usize,
    delta: f32,
    limits: &[(f32, f32)],
) -> Vec<f32> {
    let mut sizes = initial_sizes.to_vec();
    let i = divider_index;
    let next = i + 1;
    if next >= sizes.len() {
        return sizes;
    }

    let limit_at = |idx: usize| limits.get(idx).copied().unwrap_or((MIN_PANEL_SIZE, MAX_PANEL_SIZE));

    let (min_i, max_i) = limit_at(i);
    let (min_next, max_next) = limit_at(next);
    let old_i = sizes[i];
    let old_next = sizes[next];

    let new_i = (old_i + delta).clamp(min_i, max_i);
    let attempted_delta = new_i - old_i;
    let new_next = (old_next - attempted_delta).clamp(min_next, max_next);
    let reconciled_delta = old_next - new_next;
    sizes[i] = (old_i + reconciled_delta).clamp(min_i, max_i);
    sizes[next] = new_next;

    sizes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_limits(n: usize) -> Vec<(f32, f32)> {
        vec![(MIN_PANEL_SIZE, MAX_PANEL_SIZE); n]
    }

    #[test]
    fn drag_delta_balances_neighbors() {
        let initial = vec![300.0, 300.0, 300.0];
        let limits = default_limits(3);
        let new = apply_drag_delta(&initial, 1, 50.0, &limits);
        assert_eq!(new[0], 300.0);
        assert!((new[1] - 350.0).abs() < 0.01);
        assert!((new[2] - 250.0).abs() < 0.01);
    }

    #[test]
    fn drag_delta_clamps_to_default_min() {
        let initial = vec![300.0, 300.0, 300.0];
        let limits = default_limits(3);
        let new = apply_drag_delta(&initial, 0, -10_000.0, &limits);
        assert!((new[0] - MIN_PANEL_SIZE).abs() < 0.01);
    }

    #[test]
    fn drag_delta_honors_neighbor_min() {
        let initial = vec![300.0, 300.0, 300.0];
        // Baz has min=250; dragging divider 1 right attempts to grow Bar by
        // 100 (→ 400), which would shrink Baz to 200 — Baz min caps effective
        // delta at 50.
        let limits = vec![
            (MIN_PANEL_SIZE, MAX_PANEL_SIZE),
            (MIN_PANEL_SIZE, MAX_PANEL_SIZE),
            (250.0, MAX_PANEL_SIZE),
        ];
        let new = apply_drag_delta(&initial, 1, 100.0, &limits);
        assert!((new[1] - 350.0).abs() < 0.01);
        assert!((new[2] - 250.0).abs() < 0.01);
    }

    #[test]
    fn drag_delta_honors_own_max() {
        let initial = vec![300.0, 300.0, 300.0];
        let limits = vec![
            (MIN_PANEL_SIZE, 400.0),
            (MIN_PANEL_SIZE, MAX_PANEL_SIZE),
            (MIN_PANEL_SIZE, MAX_PANEL_SIZE),
        ];
        let new = apply_drag_delta(&initial, 0, 500.0, &limits);
        assert!((new[0] - 400.0).abs() < 0.01);
    }

    #[test]
    fn drag_delta_cannot_shrink_last_via_middle() {
        // Regression: previously Baz (last) had no stored size so growing Bar
        // via divider 1 was unclamped. Now all N sizes tracked → Baz's min
        // applies.
        let initial = vec![300.0, 300.0, 300.0];
        let limits = vec![
            (MIN_PANEL_SIZE, MAX_PANEL_SIZE),
            (MIN_PANEL_SIZE, MAX_PANEL_SIZE),
            (200.0, MAX_PANEL_SIZE),
        ];
        let new = apply_drag_delta(&initial, 1, 10_000.0, &limits);
        assert!((new[2] - 200.0).abs() < 0.01);
        // Bar grew by exactly what Baz yielded: 300 - 200 = 100.
        assert!((new[1] - 400.0).abs() < 0.01);
    }
}

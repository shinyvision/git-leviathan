//! Resizable split container.
//!
//! Lua plugins declare `{kind="resizable_split", id, direction, children}`.
//! The host decodes that into a [`ResizableSplitNode`] AST and we render
//! from the typed AST. Drag state + per-split pixel sizes live on the
//! host; this builder only reads current sizes.

use iced::{
    mouse,
    widget::{column, container, row, MouseArea, Space},
    Background, Element, Length, Theme,
};

use crate::message::Message;
use crate::plugin::message::PluginMessage;
use crate::theme;

use super::super::bridge::widget_tree::{self, BuildCtx, DispatchScope};
use super::widget_ast::{AstSplitDirection, ResizableSplitNode};

pub const DIVIDER_THICKNESS: f32 = 4.0;
pub const MIN_PANEL_SIZE: f32 = 80.0;
pub const MAX_PANEL_SIZE: f32 = 4000.0;
pub const DEFAULT_PANEL_SIZE: f32 = 300.0;

/// Convert a plugin-declared split AST into an iced element tree.
pub fn build(node: &ResizableSplitNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    // `resizable_split` really only makes sense inside a screen. Emit a
    // placeholder rather than a confusing split elsewhere.
    if !matches!(ctx.scope, DispatchScope::Screen { .. }) {
        return Space::new().into();
    }

    let id = node.split_id.clone().unwrap_or_else(|| "split".to_string());
    let is_vertical = matches!(node.direction, AstSplitDirection::Vertical);
    let n = node.children.len();
    if n == 0 {
        return Space::new().width(Length::Fill).height(Length::Fill).into();
    }
    if n == 1 {
        return widget_tree::build(&node.children[0], ctx);
    }

    let split_key = format!("{}:{}:{}", ctx.plugin_id, ctx.scope.storage_key(), id);
    let sizes = ctx
        .split_states
        .get(&split_key)
        .cloned()
        .unwrap_or_else(|| vec![DEFAULT_PANEL_SIZE; n]);

    let limits: Vec<(f32, f32)> = node
        .children
        .iter()
        .map(|c| widget_tree::container_size_limits(c, is_vertical))
        .collect();

    let mut items: Vec<Element<'static, Message>> = Vec::with_capacity(n * 2 - 1);

    for (i, child_ast) in node.children.iter().enumerate() {
        let built: Element<'static, Message> = widget_tree::build(child_ast, ctx);
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
        column(items)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
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

/// Apply a drag delta to a split's stored sizes.
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

    let limit_at = |idx: usize| {
        limits
            .get(idx)
            .copied()
            .unwrap_or((MIN_PANEL_SIZE, MAX_PANEL_SIZE))
    };

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
        let initial = vec![300.0, 300.0, 300.0];
        let limits = vec![
            (MIN_PANEL_SIZE, MAX_PANEL_SIZE),
            (MIN_PANEL_SIZE, MAX_PANEL_SIZE),
            (200.0, MAX_PANEL_SIZE),
        ];
        let new = apply_drag_delta(&initial, 1, 10_000.0, &limits);
        assert!((new[2] - 200.0).abs() < 0.01);
        assert!((new[1] - 400.0).abs() < 0.01);
    }
}

//! `{kind = "container", child?, bg?, width?, height?,
//!   max_width?, max_height?, center_x?, center_y?}`.
//!
//! Padding is **not** a container property. Plugins wrap the child in a
//! `padding` widget when they want spacing. Container owns background,
//! sizing, and centering only.
//!
//! Also exposes `container_size_limits` — the `resizable_split` widget
//! reads per-pane `(min, max)` from container children at render time so
//! drag clamps against each pane's own bounds.

use iced::{
    widget::{container, Space},
    Background, Element, Length, Theme,
};
use serde_json::Value;

use crate::message::Message;

use super::common::{parse_color, parse_length};
use super::BuildCtx;

pub(super) fn build(node: &Value, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let child: Element<'static, Message> = match node.get("child") {
        Some(c) => super::build(c, ctx),
        None => Space::new().into(),
    };
    let bg = parse_color(node.get("bg"));
    let mut cont = container(child);
    if let Some(w) = parse_length(node.get("width")) {
        cont = cont.width(w);
    } else {
        cont = cont.width(Length::Fill);
    }
    if let Some(h) = parse_length(node.get("height")) {
        cont = cont.height(h);
    } else {
        cont = cont.height(Length::Fill);
    }
    if let Some(max_w) = node.get("max_width").and_then(Value::as_f64) {
        cont = cont.max_width(max_w as f32);
    }
    if let Some(max_h) = node.get("max_height").and_then(Value::as_f64) {
        cont = cont.max_height(max_h as f32);
    }
    let center_x = node.get("center_x").and_then(Value::as_bool).unwrap_or(false);
    let center_y = node.get("center_y").and_then(Value::as_bool).unwrap_or(false);
    if center_x {
        cont = cont.center_x(Length::Fill);
    }
    if center_y {
        cont = cont.center_y(Length::Fill);
    }
    if let Some(color) = bg {
        cont = cont.style(move |_: &Theme| container::Style {
            background: Some(Background::Color(color)),
            ..Default::default()
        });
    }
    cont.into()
}

/// Extract `(min, max)` size limits from a container node for the given
/// axis. Falls back to the global split defaults when unspecified or when
/// the node is not a container.
pub fn container_size_limits(node: &Value, is_vertical: bool) -> (f32, f32) {
    let (min_key, max_key) = if is_vertical {
        ("min_height", "max_height")
    } else {
        ("min_width", "max_width")
    };
    let min = node
        .get(min_key)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .unwrap_or(crate::plugin::ui::split::MIN_PANEL_SIZE);
    let max = node
        .get(max_key)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .unwrap_or(crate::plugin::ui::split::MAX_PANEL_SIZE);
    (min, max)
}

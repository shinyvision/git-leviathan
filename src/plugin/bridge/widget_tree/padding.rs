//! `{kind = "padding", top?, right?, bottom?, left?, width?, height?, child}`
//! — a pure padding wrapper.
//!
//! Per-side padding in pixels; missing sides default to `0`. The wrapper's
//! `width`/`height` default to `shrink` so a padding widget sizes to its
//! child + padding. Plugins that want the wrapper to expand (e.g. to fill
//! a parent row) set `width = "fill"` explicitly.
//!
//! Implementation: an `iced::Container` configured with padding and
//! nothing else — no background, no centering. Container is the slimmest
//! iced primitive that exposes padding, so "padding as widget" rides on
//! it without code duplication.

use iced::{
    widget::{container, Space},
    Element, Length, Padding,
};
use serde_json::Value;

use crate::message::Message;

use super::common::parse_length;
use super::BuildCtx;

pub(super) fn build(node: &Value, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let child: Element<'static, Message> = match node.get("child") {
        Some(c) => super::build(c, ctx),
        None => Space::new().into(),
    };
    let top = side(node, "top");
    let right = side(node, "right");
    let bottom = side(node, "bottom");
    let left = side(node, "left");
    let padding = Padding {
        top,
        right,
        bottom,
        left,
    };
    let w = parse_length(node.get("width")).unwrap_or(Length::Shrink);
    let h = parse_length(node.get("height")).unwrap_or(Length::Shrink);
    container(child).padding(padding).width(w).height(h).into()
}

fn side(node: &Value, key: &str) -> f32 {
    node.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32
}

//! `{kind = "row", children, spacing?, width?, height?, align_y?}`.
//! Defaults: width=fill, height=shrink.
//!
//! Padding is not a row property — wrap the row in a `padding` widget
//! when you want outer spacing. `spacing` (gap between children) stays
//! here; it's an intrinsic property of row layout, not padding.

use iced::{widget::row, Element, Length};
use serde_json::Value;

use crate::message::Message;

use super::build_children;
use super::common::{parse_alignment_y, parse_length};
use super::BuildCtx;

pub(super) fn build(node: &Value, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let children = build_children(node, ctx);
    let spacing = node.get("spacing").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let w = parse_length(node.get("width")).unwrap_or(Length::Fill);
    let h = parse_length(node.get("height")).unwrap_or(Length::Shrink);
    let mut r = row(children).spacing(spacing).width(w).height(h);
    if let Some(align) = parse_alignment_y(node.get("align_y")) {
        r = r.align_y(align);
    }
    r.into()
}

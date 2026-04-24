//! `{kind = "column", children, spacing?, width?, height?, align_x?}`.
//! Defaults: width=fill, height=fill.
//!
//! Padding is not a column property — wrap the column in a `padding`
//! widget when you want outer spacing. `spacing` (gap between children)
//! stays here; it's an intrinsic property of column layout, not padding.

use iced::{widget::column, Element, Length};
use serde_json::Value;

use crate::message::Message;

use super::build_children;
use super::common::{parse_alignment_x, parse_length};
use super::BuildCtx;

pub(super) fn build(node: &Value, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let children = build_children(node, ctx);
    let spacing = node.get("spacing").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let w = parse_length(node.get("width")).unwrap_or(Length::Fill);
    let h = parse_length(node.get("height")).unwrap_or(Length::Fill);
    let mut col = column(children).spacing(spacing).width(w).height(h);
    if let Some(align) = parse_alignment_x(node.get("align_x")) {
        col = col.align_x(align);
    }
    col.into()
}

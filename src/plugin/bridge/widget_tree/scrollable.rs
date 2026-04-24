//! `{kind = "scrollable", child, width?, height?}`.
//!
//! Vertical scroll by default. The child's height should be `shrink` so
//! iced can measure content size; `fill` children never overflow and the
//! scrollbar never appears.

use iced::{
    widget::{scrollable, Space},
    Element, Length,
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
    let w = parse_length(node.get("width")).unwrap_or(Length::Fill);
    let h = parse_length(node.get("height")).unwrap_or(Length::Fill);
    scrollable(child).width(w).height(h).into()
}

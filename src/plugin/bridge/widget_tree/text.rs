//! `{kind = "text", value, size?, color?}` — static label.

use iced::{widget::text, Element, Theme};
use serde_json::Value;

use crate::message::Message;
use crate::theme;

use super::common::parse_color;

pub(super) fn build(node: &Value) -> Element<'static, Message> {
    let value = node
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let size = node.get("size").and_then(Value::as_f64).unwrap_or(14.0) as f32;
    let color = parse_color(node.get("color")).unwrap_or(theme::TEXT_PRIMARY);
    text(value)
        .size(size)
        .style(move |_: &Theme| iced::widget::text::Style { color: Some(color) })
        .into()
}

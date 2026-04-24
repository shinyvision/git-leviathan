//! `{kind = "space", width?, height?}` — flexible or fixed gap.
//! `width = "fill"` inside a row pushes subsequent items to the right.

use iced::{widget::Space, Element, Length};
use serde_json::Value;

use crate::message::Message;

use super::common::parse_length;

pub(super) fn build(node: &Value) -> Element<'static, Message> {
    let w = parse_length(node.get("width")).unwrap_or(Length::Shrink);
    let h = parse_length(node.get("height")).unwrap_or(Length::Shrink);
    Space::new().width(w).height(h).into()
}

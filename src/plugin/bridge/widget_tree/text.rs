//! `{kind = "text", value, size?, color?}` — static label.

use iced::{widget::text, Element, Theme};

use crate::message::Message;
use crate::plugin::ui::widget_ast::TextNode;
use crate::theme;

use super::common::opt_color_to_iced;

pub(super) fn build(node: &TextNode) -> Element<'static, Message> {
    let value = node.value.clone();
    let size = node.size;
    let color = opt_color_to_iced(&node.color).unwrap_or(theme::TEXT_PRIMARY);
    text(value)
        .size(size)
        .style(move |_: &Theme| iced::widget::text::Style { color: Some(color) })
        .into()
}

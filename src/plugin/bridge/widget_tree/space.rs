//! `{kind = "space", width?, height?}` — flexible or fixed gap.

use iced::{widget::Space, Element, Length};

use crate::message::Message;
use crate::plugin::ui::widget_ast::SpaceNode;

use super::common::length_or;

pub(super) fn build(node: &SpaceNode) -> Element<'static, Message> {
    let w = length_or(node.width, Length::Shrink);
    let h = length_or(node.height, Length::Shrink);
    Space::new().width(w).height(h).into()
}

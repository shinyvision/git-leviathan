//! `{kind = "padding", top?, right?, bottom?, left?, width?, height?, child}`
//! — pure padding wrapper.

use iced::{
    widget::{container, Space},
    Element, Length, Padding,
};

use crate::message::Message;
use crate::plugin::ui::widget_ast::PaddingNode;

use super::common::length_or;
use super::BuildCtx;

pub(super) fn build(node: &PaddingNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let child: Element<'static, Message> = match &node.child {
        Some(c) => super::build(c, ctx),
        None => Space::new().into(),
    };
    let padding = Padding {
        top: node.top,
        right: node.right,
        bottom: node.bottom,
        left: node.left,
    };
    let w = length_or(node.width, Length::Shrink);
    let h = length_or(node.height, Length::Shrink);
    container(child).padding(padding).width(w).height(h).into()
}

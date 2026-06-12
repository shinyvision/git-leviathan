//! `{kind = "mouse_area", child, on_click?, value?}` — generic click target.

use iced::{
    mouse,
    widget::{MouseArea, Space},
    Element,
};

use crate::message::Message;
use crate::plugin::ui::widget_ast::MouseAreaNode;

use super::common::ScopeDispatch;
use super::BuildCtx;

pub(super) fn build(node: &MouseAreaNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let child: Element<'static, Message> = match &node.child {
        Some(c) => super::build(c, ctx),
        None => Space::new().into(),
    };
    let event = node.on_click.clone().unwrap_or_default();
    let value = node.value.clone();
    let msg = if event.is_empty() {
        Message::noop()
    } else {
        ScopeDispatch::from_ctx(ctx).publish(event, value)
    };
    MouseArea::new(child)
        .on_press(msg)
        .interaction(mouse::Interaction::Pointer)
        .into()
}

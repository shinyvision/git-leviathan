//! `{kind = "mouse_area", child, on_click?, value?}` — generic click target.

use iced::{
    mouse,
    widget::{MouseArea, Space},
    Element,
};

use crate::message::Message;
use crate::plugin::message::PluginMessage;
use crate::plugin::ui::widget_ast::MouseAreaNode;

use super::{BuildCtx, DispatchScope};

pub(super) fn build(node: &MouseAreaNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let child: Element<'static, Message> = match &node.child {
        Some(c) => super::build(c, ctx),
        None => Space::new().into(),
    };
    let event = node.on_click.clone().unwrap_or_default();
    let value = node.value.clone();
    let plugin_id = ctx.plugin_id.to_string();
    let msg = if event.is_empty() {
        Message::noop()
    } else {
        match ctx.scope {
            DispatchScope::Screen { screen_id } => Message::Plugin(PluginMessage::Event {
                plugin_id,
                screen_id: screen_id.to_string(),
                event,
                value,
            }),
            DispatchScope::Slot {
                region,
                container,
                slot_id,
            } => Message::Plugin(PluginMessage::SlotClicked {
                plugin_id,
                region: region.to_string(),
                container: container.to_string(),
                slot_id: slot_id.to_string(),
                event,
                value,
            }),
        }
    };
    MouseArea::new(child)
        .on_press(msg)
        .interaction(mouse::Interaction::Pointer)
        .into()
}

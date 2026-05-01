//! `{kind = "mouse_area", child, on_click?, value?}` — generic click
//! target that wraps any child, routes clicks through the current
//! dispatch scope (screen → `PluginMessage::Event`; slot →
//! `PluginMessage::SlotClicked`).

use iced::{
    mouse,
    widget::{MouseArea, Space},
    Element,
};
use serde_json::Value;

use crate::message::Message;
use crate::plugin::message::PluginMessage;

use super::{BuildCtx, DispatchScope};

pub(super) fn build(node: &Value, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let child: Element<'static, Message> = match node.get("child") {
        Some(c) => super::build(c, ctx),
        None => Space::new().into(),
    };
    let event = node
        .get("on_click")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let value = node.get("value").cloned().unwrap_or(Value::Null);
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
            }),
        }
    };
    MouseArea::new(child)
        .on_press(msg)
        .interaction(mouse::Interaction::Pointer)
        .into()
}

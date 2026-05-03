//! `{kind = "tablist", tabs, active?, orderable?, on_select?, on_close?,
//! on_reorder?}` — Lua-facing wrapper around the native
//! [`crate::widgets::tab_bar::TabBar`].

use std::collections::HashMap;

use iced::Element;
use serde_json::Value;

use crate::message::Message;
use crate::plugin::message::PluginMessage;
use crate::plugin::ui::widget_ast::TablistNode;
use crate::widgets::tab_bar::parts::{close_button, folder_icon};
use crate::widgets::tab_bar::{TabBar, TabItem};

use super::{BuildCtx, DispatchScope};

pub(super) fn build(node: &TablistNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let active_idx = node.tabs.iter().position(|t| t.id == node.active);

    let on_select = node.on_select.clone();
    let on_close = node.on_close.clone();
    let on_reorder = node.on_reorder.clone();

    let dispatch = Dispatch::from_ctx(ctx);
    let key_for_id: Vec<u64> = node.tabs.iter().map(|t| stable_key(&t.id)).collect();
    let id_for_key: HashMap<u64, Value> = key_for_id
        .iter()
        .zip(node.tabs.iter())
        .map(|(k, t)| (*k, t.id.clone()))
        .collect();

    let active_key = active_idx.map(|i| key_for_id[i]).unwrap_or(u64::MAX);

    let items: Vec<TabItem<'static, u64, Message>> = node
        .tabs
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let mut item = TabItem::new(key_for_id[i], spec.name.clone())
                .leading(folder_icon(active_idx == Some(i)));
            if let Some(evt) = &on_close {
                item = item.trailing(close_button(dispatch.publish(evt.clone(), spec.id.clone())));
            }
            item
        })
        .collect();

    let mut bar = TabBar::new(items, active_key);
    if let Some(evt) = on_select {
        let dispatch = dispatch.clone();
        let map = id_for_key.clone();
        bar = bar.on_select(move |k| {
            let value = map.get(&k).cloned().unwrap_or(Value::Null);
            dispatch.publish(evt.clone(), value)
        });
    }
    if node.orderable {
        if let Some(evt) = on_reorder {
            let dispatch = dispatch.clone();
            let map = id_for_key.clone();
            bar = bar.on_reorder(move |order: Vec<u64>| {
                let new_order: Vec<Value> = order
                    .into_iter()
                    .map(|k| map.get(&k).cloned().unwrap_or(Value::Null))
                    .collect();
                dispatch.publish(evt.clone(), Value::Array(new_order))
            });
        }
    }
    bar.into()
}

/// Stable 64-bit hash of an arbitrary JSON value. Used as the TabBar
/// key per tab so reorders / inserts / removes preserve identity.
fn stable_key(id: &Value) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    serde_json::to_string(id).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// Owned snapshot of `BuildCtx.scope` used inside `'static` event closures.
#[derive(Clone)]
enum Dispatch {
    Screen {
        plugin_id: String,
        screen_id: String,
    },
    Slot {
        plugin_id: String,
        region: String,
        container: String,
        slot_id: String,
    },
    Overlay {
        plugin_id: String,
        overlay_id: String,
    },
}

impl Dispatch {
    fn from_ctx(ctx: &BuildCtx<'_>) -> Self {
        let plugin_id = ctx.plugin_id.to_string();
        match ctx.scope {
            DispatchScope::Screen { screen_id } => Self::Screen {
                plugin_id,
                screen_id: screen_id.to_string(),
            },
            DispatchScope::Slot {
                region,
                container,
                slot_id,
            } => Self::Slot {
                plugin_id,
                region: region.to_string(),
                container: container.to_string(),
                slot_id: slot_id.to_string(),
            },
            DispatchScope::Overlay { overlay_id } => Self::Overlay {
                plugin_id,
                overlay_id: overlay_id.to_string(),
            },
        }
    }

    fn publish(&self, event: String, value: Value) -> Message {
        match self.clone() {
            Self::Screen {
                plugin_id,
                screen_id,
            } => Message::Plugin(PluginMessage::Event {
                plugin_id,
                screen_id,
                event,
                value,
            }),
            Self::Slot {
                plugin_id,
                region,
                container,
                slot_id,
            } => Message::Plugin(PluginMessage::SlotClicked {
                plugin_id,
                region,
                container,
                slot_id,
                event,
                value,
            }),
            Self::Overlay {
                plugin_id,
                overlay_id,
            } => Message::Plugin(PluginMessage::OverlayEvent {
                plugin_id,
                overlay_id,
                event,
                value,
            }),
        }
    }
}

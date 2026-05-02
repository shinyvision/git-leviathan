//! `{kind = "tablist", tabs, active?, orderable?, on_select?, on_close?,
//! on_reorder?}` — Lua-facing wrapper around the native
//! [`crate::widgets::tab_bar::TabBar`].
//!
//! Tabs are arbitrary: the plugin supplies them in `tabs`, each entry
//! `{ id = <any>, name = "..." }`. `active` matches against one of those
//! ids. Drag-reorder mechanics live entirely in the Rust widget;
//! `orderable` (default `false`) toggles whether dragging is allowed.
//!
//! Events route the same way as `button`/`mouse_area`:
//! `PluginMessage::Event` in screen scope, `PluginMessage::SlotClicked`
//! in slot scope. Payload `value` is the affected tab id (`on_select`,
//! `on_close`) or an array of ids in the new order (`on_reorder`).
//!
//! TabBar is generic over `Hash + Eq + Copy`; this widget keys each tab
//! by a `u64` derived from a stable hash of its JSON id, so tab identity
//! survives reorders — the FLIP tracker can follow tabs across position
//! changes instead of confusing "tab at index N" between frames.

use std::collections::HashMap;

use iced::Element;
use serde_json::Value;

use crate::message::Message;
use crate::plugin::message::PluginMessage;
use crate::widgets::tab_bar::parts::{close_button, folder_icon};
use crate::widgets::tab_bar::{TabBar, TabItem};

use super::{BuildCtx, DispatchScope};

pub(super) fn build(node: &Value, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let orderable = node
        .get("orderable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tabs = read_tabs(node);
    let active_id = node.get("active").cloned().unwrap_or(Value::Null);
    let active_idx = tabs.iter().position(|t| t.id == active_id);

    let on_select = read_event(node, "on_select");
    let on_close = read_event(node, "on_close");
    let on_reorder = read_event(node, "on_reorder");

    let dispatch = Dispatch::from_ctx(ctx);
    let key_for_id: Vec<u64> = tabs.iter().map(|t| stable_key(&t.id)).collect();
    let id_for_key: HashMap<u64, Value> = key_for_id
        .iter()
        .zip(tabs.iter())
        .map(|(k, t)| (*k, t.id.clone()))
        .collect();

    let active_key = active_idx.map(|i| key_for_id[i]).unwrap_or(u64::MAX);

    let items: Vec<TabItem<'static, u64, Message>> = tabs
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
    if orderable {
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

struct TabSpec {
    id: Value,
    name: String,
}

fn read_tabs(node: &Value) -> Vec<TabSpec> {
    node.get("tabs")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|t| TabSpec {
                    id: t.get("id").cloned().unwrap_or(Value::Null),
                    name: t.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_event(node: &Value, key: &str) -> Option<String> {
    node.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Stable 64-bit hash of an arbitrary JSON value. Used as the TabBar
/// key per tab so reorders / inserts / removes preserve identity across
/// renders. Collision space is 2^64 over a few-dozen-tabs working set,
/// so collisions are vanishingly unlikely in practice.
fn stable_key(id: &Value) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    serde_json::to_string(id).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// Owned snapshot of `BuildCtx.scope` used inside `'static` event
/// closures (the borrowed `DispatchScope` can't escape the build call).
#[derive(Clone)]
enum Dispatch {
    Screen { plugin_id: String, screen_id: String },
    Slot {
        plugin_id: String,
        region: String,
        container: String,
        slot_id: String,
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
        }
    }

    fn publish(&self, event: String, value: Value) -> Message {
        match self.clone() {
            Self::Screen { plugin_id, screen_id } => Message::Plugin(PluginMessage::Event {
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
        }
    }
}

//! Lua widget tree → `iced::Element` converter.
//!
//! Plugins return a nested table from `view(state)` (or declare one on a
//! `main_bar.add{…}` call). The host serialises it to `serde_json::Value`
//! (via `mlua::LuaSerdeExt::from_value`) and hands it here. `build` walks
//! the value and dispatches each node to the submodule responsible for
//! that widget kind.
//!
//! The converter is **scope-agnostic**: the same tree can describe a
//! plugin screen or a main-bar slot. Interactive widgets (button,
//! mouse_area) dispatch differently per scope — see [`DispatchScope`].
//!
//! Adding a new widget kind:
//! 1. Create `<kind>.rs` with `pub(super) fn build(node, ctx) -> Element<'static, …>`.
//! 2. Add `mod <kind>;` below and a match arm in the dispatch.
//! 3. If the widget exposes layout hints consumed outside (see
//!    `container::container_size_limits`), re-export them at module level.
//!
//! Every widget build returns `Element<'static, Message>` — all content is
//! owned (strings, svg handles) so the Element never borrows from the
//! input `Value`. This lets callers store widget trees on the heap and
//! render them on demand without lifetime gymnastics.

use std::path::Path;

use iced::Element;
use serde_json::Value;

use crate::message::Message;
use crate::plugin::ui::split;

mod button;
mod column;
mod common;
mod container;
mod icon;
mod image;
mod mouse_area;
mod padding;
mod row;
mod scrollable;
mod space;
mod text;

// Consumed outside this module by `plugin::ui::split`.
pub use container::container_size_limits;

use common::error_text;

/// What the tree is rendering inside — drives how interactive widgets
/// route their clicks.
///
/// - `Screen` clicks emit `PluginMessage::Event { plugin_id, screen_id,
///   event, value }`. The plugin's `update(state, event, value)` sees them.
/// - `MainBarSlot` clicks emit `PluginMessage::SlotClicked { plugin_id,
///   slot_id }`. The slot's `on_click` Lua fn is invoked directly.
#[derive(Clone, Copy)]
pub enum DispatchScope<'a> {
    Screen { screen_id: &'a str },
    MainBarSlot { slot_id: &'a str },
}

impl<'a> DispatchScope<'a> {
    /// Unique-per-scope string used to key host-side state (e.g. split
    /// sizes) so a plugin's screen and slot can both hold their own
    /// widgets without stepping on each other.
    pub fn storage_key(&self) -> String {
        match self {
            DispatchScope::Screen { screen_id } => format!("screen:{screen_id}"),
            DispatchScope::MainBarSlot { slot_id } => format!("slot:{slot_id}"),
        }
    }
}

/// Per-render state the converter needs. `split_states` is a reference
/// into the host so resizable splits render with current ratios;
/// `active_drag` identifies the divider being dragged. `plugin_root` is
/// the sandbox root used to resolve plugin-bundled assets (icons).
pub struct BuildCtx<'a> {
    pub plugin_id: &'a str,
    pub scope: DispatchScope<'a>,
    pub plugin_root: &'a Path,
    pub split_states: &'a std::collections::HashMap<String, Vec<f32>>,
    pub active_drag: Option<(&'a str, usize)>,
}

pub fn build(node: &Value, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let Some(kind) = node.get("kind").and_then(Value::as_str) else {
        return error_text("missing kind".to_string());
    };
    match kind {
        "text" => text::build(node),
        "button" => button::build(node, ctx),
        "row" => row::build(node, ctx),
        "column" => column::build(node, ctx),
        "container" => container::build(node, ctx),
        "padding" => padding::build(node, ctx),
        "space" => space::build(node),
        "icon" => icon::build(node, ctx),
        "image" => image::build(node, ctx),
        "scrollable" => scrollable::build(node, ctx),
        "mouse_area" => mouse_area::build(node, ctx),
        "resizable_split" => split::build(node, ctx),
        other => error_text(format!("unknown kind: {other}")),
    }
}

/// Recursively build each child of a row/column node. Lifts the common
/// "children is an array of nodes" pattern out of every container-ish kind.
pub(super) fn build_children(
    node: &Value,
    ctx: &BuildCtx<'_>,
) -> Vec<Element<'static, Message>> {
    node.get("children")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(|child| build(child, ctx)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct TestCtx {
        splits: HashMap<String, Vec<f32>>,
        root: std::path::PathBuf,
    }

    impl TestCtx {
        fn new() -> Self {
            Self {
                splits: HashMap::new(),
                root: std::path::PathBuf::from("/tmp/plugin-test"),
            }
        }
        fn ctx(&self) -> BuildCtx<'_> {
            BuildCtx {
                plugin_id: "p1",
                scope: DispatchScope::Screen { screen_id: "s1" },
                plugin_root: &self.root,
                split_states: &self.splits,
                active_drag: None,
            }
        }
    }

    #[test]
    fn unknown_kind_does_not_panic() {
        let t = TestCtx::new();
        let node = serde_json::json!({ "kind": "not_a_widget" });
        let _ = build(&node, &t.ctx());
    }

    #[test]
    fn missing_kind_renders_error() {
        let t = TestCtx::new();
        let node = serde_json::json!({ "value": "no kind" });
        let _ = build(&node, &t.ctx());
    }

    #[test]
    fn every_kind_builds_without_panic() {
        let t = TestCtx::new();
        let ctx = t.ctx();
        let _ = build(&serde_json::json!({ "kind": "text", "value": "hello" }), &ctx);
        let _ = build(&serde_json::json!({ "kind": "button", "text": "b" }), &ctx);
        let _ = build(&serde_json::json!({ "kind": "row", "children": [] }), &ctx);
        let _ = build(&serde_json::json!({ "kind": "column", "children": [] }), &ctx);
        let _ = build(&serde_json::json!({ "kind": "container" }), &ctx);
        let _ = build(&serde_json::json!({ "kind": "space" }), &ctx);
        let _ = build(&serde_json::json!({ "kind": "icon", "path": "icons/x.svg" }), &ctx);
        let _ = build(&serde_json::json!({ "kind": "image", "path": "assets/x.png" }), &ctx);
        let _ = build(
            &serde_json::json!({ "kind": "scrollable", "child": { "kind": "space" } }),
            &ctx,
        );
        let _ = build(
            &serde_json::json!({
                "kind": "mouse_area",
                "on_click": "e",
                "child": { "kind": "space" }
            }),
            &ctx,
        );
    }

    #[test]
    fn slot_scope_builds_button() {
        let t = TestCtx::new();
        let ctx = BuildCtx {
            plugin_id: "p1",
            scope: DispatchScope::MainBarSlot { slot_id: "plugin.p1.foo" },
            plugin_root: &t.root,
            split_states: &t.splits,
            active_drag: None,
        };
        let _ = build(
            &serde_json::json!({ "kind": "button", "text": "Slot", "on_click": "click" }),
            &ctx,
        );
    }
}

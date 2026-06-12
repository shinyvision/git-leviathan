//! Typed widget AST → `iced::Element` converter.
//!
//! widget AST: the renderer consumes a [`WidgetAst`] only. The boundary
//! decoder validates plugin Lua tables once at the boundary; the build
//! step here is pure AST → iced. The renderer cannot panic on a
//! malformed tree because there is no malformed tree to see — anything
//! invalid was rejected upstream and rendered as an
//! [`build_error_widget`].
//!
//! The converter is **scope-agnostic**: the same tree can describe a
//! plugin screen or a main-bar slot. Interactive widgets (button,
//! mouse_area, tablist) dispatch differently per scope — see
//! [`DispatchScope`].
//!
//! Adding a new widget kind:
//! 1. Add a `WidgetNode::*` variant + AST struct in
//!    `crate::plugin::ui::widget_ast`.
//! 2. Add a decoder branch in `widget_ast::decode_node`.
//! 3. Create `<kind>.rs` here with
//!    `pub(super) fn build(node, ctx) -> Element<'static, …>`.
//! 4. Add `mod <kind>;` below and a match arm in the dispatch.
//!
//! Every widget build returns `Element<'static, Message>` — content is
//! owned (strings, svg handles) so the Element never borrows from the
//! AST.

use std::path::Path;

use iced::Element;

use crate::message::Message;
use crate::plugin::ui::split;
use crate::plugin::ui::widget_ast::{WidgetAst, WidgetNode};

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
mod semantic;
mod space;
mod tablist;
mod terminal;
mod text;
mod text_input;

pub use common::build_error_widget;
pub(crate) use image::clear_for_root as clear_image_cache_for_root;
pub(crate) use text_input::plugin_text_input_id;
// Container size limits read by `plugin::ui::split` for per-pane clamps.
pub use container::container_size_limits;

/// What the tree is rendering inside — drives how interactive widgets
/// route their clicks.
#[derive(Clone, Copy)]
pub enum DispatchScope<'a> {
    Screen {
        screen_id: &'a str,
    },
    Slot {
        region: &'a str,
        container: &'a str,
        slot_id: &'a str,
    },
    Overlay {
        overlay_id: &'a str,
    },
}

impl<'a> DispatchScope<'a> {
    /// Unique-per-scope string used to key host-side state (e.g. split
    /// sizes) so a plugin's screen and slot can both hold their own
    /// widgets without stepping on each other.
    pub fn storage_key(&self) -> String {
        match self {
            DispatchScope::Screen { screen_id } => format!("screen:{screen_id}"),
            DispatchScope::Slot {
                region,
                container,
                slot_id,
            } => format!("slot:{region}:{container}:{slot_id}"),
            DispatchScope::Overlay { overlay_id } => format!("overlay:{overlay_id}"),
        }
    }
}

/// Per-render state the converter needs.
pub struct BuildCtx<'a> {
    pub plugin_id: &'a str,
    pub scope: DispatchScope<'a>,
    pub plugin_root: &'a Path,
    pub split_states: &'a std::collections::HashMap<String, Vec<f32>>,
    pub active_drag: Option<(&'a str, usize)>,
}

pub fn build(ast: &WidgetAst, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    match &ast.node {
        WidgetNode::Text(n) => text::build(n),
        WidgetNode::Button(n) => button::build(n, ctx),
        WidgetNode::TextInput(n) => text_input::build(ast, n, ctx),
        WidgetNode::Row(n) => row::build(n, ctx),
        WidgetNode::Column(n) => column::build(n, ctx),
        WidgetNode::Container(n) => container::build(n, ctx),
        WidgetNode::Padding(n) => padding::build(n, ctx),
        WidgetNode::Space(n) => space::build(n),
        WidgetNode::Icon(n) => icon::build(n, ctx),
        WidgetNode::Image(n) => image::build(n, ctx),
        WidgetNode::Scrollable(n) => scrollable::build(n, ctx),
        WidgetNode::MouseArea(n) => mouse_area::build(n, ctx),
        WidgetNode::Tablist(n) => tablist::build(n, ctx),
        WidgetNode::Terminal(n) => terminal::build(n),
        WidgetNode::ResizableSplit(n) => split::build(n, ctx),
        WidgetNode::Semantic(n) => semantic::build(ast, n, ctx),
        WidgetNode::Layout(n) => semantic::build_layout(n, ctx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::ui::widget_ast::decode;
    use serde_json::json;
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

    fn ast(value: serde_json::Value) -> WidgetAst {
        decode(&value).expect("decode")
    }

    #[test]
    fn every_kind_builds_without_panic() {
        let t = TestCtx::new();
        let ctx = t.ctx();
        let _ = build(&ast(json!({ "kind": "text", "value": "hello" })), &ctx);
        let _ = build(&ast(json!({ "kind": "button", "text": "b" })), &ctx);
        let _ = build(
            &ast(json!({
                "kind": "text_input",
                "placeholder": "Filter",
                "value": "",
                "on_input": "filter.changed"
            })),
            &ctx,
        );
        let _ = build(&ast(json!({ "kind": "row", "children": [] })), &ctx);
        let _ = build(&ast(json!({ "kind": "column", "children": [] })), &ctx);
        let _ = build(&ast(json!({ "kind": "container" })), &ctx);
        let _ = build(&ast(json!({ "kind": "space" })), &ctx);
        let _ = build(&ast(json!({ "kind": "icon", "path": "icons/x.svg" })), &ctx);
        let _ = build(
            &ast(json!({ "kind": "image", "path": "assets/x.png" })),
            &ctx,
        );
        let _ = build(
            &ast(json!({ "kind": "scrollable", "child": { "kind": "space" } })),
            &ctx,
        );
        let _ = build(
            &ast(json!({
                "kind": "mouse_area",
                "on_click": "e",
                "child": { "kind": "space" }
            })),
            &ctx,
        );
        let _ = build(
            &ast(json!({
                "kind": "tablist",
                "tabs": [],
                "active": serde_json::Value::Null,
            })),
            &ctx,
        );
        let _ = build(
            &ast(json!({ "kind": "terminal", "session": 1, "height": 180 })),
            &ctx,
        );
        let _ = build(
            &ast(json!({ "kind": "resizable_split", "children": [{ "kind": "space" }] })),
            &ctx,
        );
    }

    #[test]
    fn slot_scope_builds_button() {
        let t = TestCtx::new();
        let ctx = BuildCtx {
            plugin_id: "p1",
            scope: DispatchScope::Slot {
                region: "main_bar",
                container: "right",
                slot_id: "plugin.p1.foo",
            },
            plugin_root: &t.root,
            split_states: &t.splits,
            active_drag: None,
        };
        let _ = build(
            &ast(json!({ "kind": "button", "text": "Slot", "on_click": "click" })),
            &ctx,
        );
    }

    #[test]
    fn error_widget_renders() {
        let _ = build_error_widget("widget.unknown_kind", "rwo is not known");
    }
}

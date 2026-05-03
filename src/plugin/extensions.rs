//! extension-point registries.
//!
//! Plugins register overlays, context-menu items, and graph / diff
//! decorations via the `leviathan.ui.{overlay, context_menu,
//! graph_decoration, diff_decoration}` Lua APIs. Each registration
//! lands in [`ExtensionRegistry`] keyed by `plugin_id` so the host
//! can drop everything a plugin owns on unload.
//!
//! The store is renderer-agnostic: tests inspect the projected
//! summaries and Iced rendering happens in later work. The
//! registries are deliberately small — value-typed entries with
//! deterministic ordering.

use std::cell::RefCell;
use std::rc::Rc;

use git_leviathan_plugin_api::descriptor::decoration::{DiffDecoration, GraphDecoration};

use crate::plugin::ui::widget_ast::WidgetAst;

/// One overlay registered by a plugin. The widget tree is captured
/// statically — overlay re-rendering on plugin state change is out of
/// scope for extension points.
#[derive(Debug, Clone)]
pub struct OverlayRecord {
    pub plugin_id: String,
    pub id: String,
    pub priority: i32,
    pub dismissible: bool,
    pub widget: WidgetAst,
    pub source_location: Option<String>,
}

/// One context-menu item registered at an extension point address
/// (e.g. `repository.diff.context_menu`). The host sorts items by
/// priority ascending; ties broken by `(plugin_id, id)` so output is
/// deterministic.
#[derive(Debug, Clone)]
pub struct ContextMenuItemRecord {
    pub plugin_id: String,
    pub region: String,
    pub id: String,
    pub label: String,
    pub command: String,
    pub priority: i32,
    pub condition_capability: Option<String>,
    pub source_location: Option<String>,
}

/// One graph decoration bound to a commit hash. The decoration AST
/// itself is the typed [`GraphDecoration`] from the descriptor crate.
#[derive(Debug, Clone)]
pub struct GraphDecorationRecord {
    pub plugin_id: String,
    pub id: String,
    pub commit_hash: String,
    pub decoration: GraphDecoration,
    pub source_location: Option<String>,
}

/// One diff decoration. Carries enough address (file/line/hunk_id)
/// inside the decoration AST itself.
#[derive(Debug, Clone)]
pub struct DiffDecorationRecord {
    pub plugin_id: String,
    pub id: String,
    pub decoration: DiffDecoration,
    pub source_location: Option<String>,
}

/// Cheap-clone (Rc-backed) extension store. Owned by the
/// [`crate::plugin::host::PluginHost`] and shared into every plugin's
/// Lua factory so registration calls land here without the host
/// holding a `&mut self` borrow.
#[derive(Debug, Default, Clone)]
pub struct ExtensionRegistry {
    inner: Rc<RefCell<ExtensionInner>>,
}

#[derive(Debug, Default)]
struct ExtensionInner {
    overlays: Vec<OverlayRecord>,
    context_menu_items: Vec<ContextMenuItemRecord>,
    graph_decorations: Vec<GraphDecorationRecord>,
    diff_decorations: Vec<DiffDecorationRecord>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_overlay(&self, record: OverlayRecord) {
        let mut inner = self.inner.borrow_mut();
        // Replace any prior overlay with the same (plugin_id, id) so a
        // plugin re-registering during the same init sees stable
        // single-occupancy semantics.
        inner
            .overlays
            .retain(|o| !(o.plugin_id == record.plugin_id && o.id == record.id));
        inner.overlays.push(record);
    }

    pub fn add_context_menu_item(&self, record: ContextMenuItemRecord) {
        let mut inner = self.inner.borrow_mut();
        inner.context_menu_items.retain(|item| {
            !(item.plugin_id == record.plugin_id
                && item.region == record.region
                && item.id == record.id)
        });
        inner.context_menu_items.push(record);
    }

    pub fn add_graph_decoration(&self, record: GraphDecorationRecord) {
        let mut inner = self.inner.borrow_mut();
        inner.graph_decorations.retain(|d| {
            !(d.plugin_id == record.plugin_id
                && d.commit_hash == record.commit_hash
                && d.id == record.id)
        });
        inner.graph_decorations.push(record);
    }

    pub fn add_diff_decoration(&self, record: DiffDecorationRecord) {
        let mut inner = self.inner.borrow_mut();
        inner
            .diff_decorations
            .retain(|d| !(d.plugin_id == record.plugin_id && d.id == record.id));
        inner.diff_decorations.push(record);
    }

    /// Drop every record owned by `plugin_id`. Used on `unload` /
    /// staged reload swap so the new generation starts clean.
    pub fn clear_for_plugin(&self, plugin_id: &str) {
        let mut inner = self.inner.borrow_mut();
        inner.overlays.retain(|o| o.plugin_id != plugin_id);
        inner
            .context_menu_items
            .retain(|item| item.plugin_id != plugin_id);
        inner.graph_decorations.retain(|d| d.plugin_id != plugin_id);
        inner.diff_decorations.retain(|d| d.plugin_id != plugin_id);
    }

    pub fn overlays(&self) -> Vec<OverlayRecord> {
        let inner = self.inner.borrow();
        let mut v = inner.overlays.clone();
        // Highest priority first; ties broken by (plugin_id, id) so
        // ordering is fully deterministic.
        v.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(a.plugin_id.cmp(&b.plugin_id))
                .then(a.id.cmp(&b.id))
        });
        v
    }

    /// All context-menu items for `region`, sorted by priority
    /// ascending (lower priority renders first), ties broken by
    /// `(plugin_id, id)`.
    pub fn context_menu_items(&self, region: &str) -> Vec<ContextMenuItemRecord> {
        let inner = self.inner.borrow();
        let mut v: Vec<ContextMenuItemRecord> = inner
            .context_menu_items
            .iter()
            .filter(|item| item.region == region)
            .cloned()
            .collect();
        v.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then(a.plugin_id.cmp(&b.plugin_id))
                .then(a.id.cmp(&b.id))
        });
        v
    }

    /// Every context-menu item across every extension point. Sorted
    /// by `(region, priority, plugin_id, id)` so devtools snapshots
    /// stay deterministic.
    pub fn all_context_menu_items(&self) -> Vec<ContextMenuItemRecord> {
        let inner = self.inner.borrow();
        let mut v = inner.context_menu_items.clone();
        v.sort_by(|a, b| {
            a.region
                .cmp(&b.region)
                .then(a.priority.cmp(&b.priority))
                .then(a.plugin_id.cmp(&b.plugin_id))
                .then(a.id.cmp(&b.id))
        });
        v
    }

    /// All graph decorations attached to `commit_hash`, sorted by
    /// `(plugin_id, id)`.
    pub fn graph_decorations_for(&self, commit_hash: &str) -> Vec<GraphDecorationRecord> {
        let inner = self.inner.borrow();
        let mut v: Vec<GraphDecorationRecord> = inner
            .graph_decorations
            .iter()
            .filter(|d| d.commit_hash == commit_hash)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id).then(a.id.cmp(&b.id)));
        v
    }

    pub fn all_graph_decorations(&self) -> Vec<GraphDecorationRecord> {
        let inner = self.inner.borrow();
        let mut v = inner.graph_decorations.clone();
        v.sort_by(|a, b| {
            a.commit_hash
                .cmp(&b.commit_hash)
                .then(a.plugin_id.cmp(&b.plugin_id))
                .then(a.id.cmp(&b.id))
        });
        v
    }

    pub fn all_diff_decorations(&self) -> Vec<DiffDecorationRecord> {
        let inner = self.inner.borrow();
        let mut v = inner.diff_decorations.clone();
        v.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id).then(a.id.cmp(&b.id)));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::ui::widget_ast::WidgetAst;
    use git_leviathan_plugin_api::descriptor::decoration::{
        DiffDecoration, GraphDecoration, HintSeverity, MarkerShape,
    };

    fn fake_widget() -> WidgetAst {
        crate::plugin::ui::widget_ast::decode(&serde_json::json!({
            "kind": "text",
            "value": "hi"
        }))
        .expect("decode static text")
    }

    #[test]
    fn overlays_sort_by_priority_descending() {
        let reg = ExtensionRegistry::new();
        for (plugin_id, id, priority) in [("p1", "a", 1), ("p2", "b", 5), ("p1", "c", 3)] {
            reg.add_overlay(OverlayRecord {
                plugin_id: plugin_id.into(),
                id: id.into(),
                priority,
                dismissible: true,
                widget: fake_widget(),
                source_location: None,
            });
        }
        let v = reg.overlays();
        assert_eq!(
            v.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            ["b", "c", "a"]
        );
    }

    #[test]
    fn context_menu_items_sort_by_priority_ascending() {
        let reg = ExtensionRegistry::new();
        for (id, prio) in [("c", 30), ("a", 10), ("b", 20)] {
            reg.add_context_menu_item(ContextMenuItemRecord {
                plugin_id: "p".into(),
                region: "repository.diff.context_menu".into(),
                id: id.into(),
                label: id.to_uppercase(),
                command: format!("cmd.{id}"),
                priority: prio,
                condition_capability: None,
                source_location: None,
            });
        }
        let v = reg.context_menu_items("repository.diff.context_menu");
        assert_eq!(
            v.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn clear_for_plugin_removes_only_owner_records() {
        let reg = ExtensionRegistry::new();
        reg.add_graph_decoration(GraphDecorationRecord {
            plugin_id: "p1".into(),
            id: "g1".into(),
            commit_hash: "abc".into(),
            decoration: GraphDecoration::Marker {
                shape: MarkerShape::Dot,
                color: "#fff".into(),
            },
            source_location: None,
        });
        reg.add_graph_decoration(GraphDecorationRecord {
            plugin_id: "p2".into(),
            id: "g2".into(),
            commit_hash: "abc".into(),
            decoration: GraphDecoration::Lane {
                index: 1,
                color: "#0f0".into(),
            },
            source_location: None,
        });
        reg.add_diff_decoration(DiffDecorationRecord {
            plugin_id: "p1".into(),
            id: "d1".into(),
            decoration: DiffDecoration::LineHint {
                severity: HintSeverity::Info,
                text: "x".into(),
                file: "f".into(),
                line: 1,
            },
            source_location: None,
        });

        // Per-commit lookup returns both rows attached to `abc`,
        // sorted by `(plugin_id, id)` for deterministic rendering.
        let abc_rows = reg.graph_decorations_for("abc");
        assert_eq!(abc_rows.len(), 2);
        assert_eq!(abc_rows[0].plugin_id, "p1");
        assert_eq!(abc_rows[1].plugin_id, "p2");
        assert!(reg.graph_decorations_for("missing").is_empty());

        reg.clear_for_plugin("p1");

        assert_eq!(reg.all_graph_decorations().len(), 1);
        assert_eq!(reg.all_graph_decorations()[0].plugin_id, "p2");
        assert!(reg.all_diff_decorations().is_empty());

        // After clearing p1, the per-commit lookup reflects the
        // remaining owner.
        let abc_after = reg.graph_decorations_for("abc");
        assert_eq!(abc_after.len(), 1);
        assert_eq!(abc_after[0].plugin_id, "p2");
    }
}

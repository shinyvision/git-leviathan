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
use std::collections::HashMap;
use std::rc::Rc;

use git_leviathan_plugin_api::descriptor::decoration::{DiffDecoration, GraphDecoration};
use mlua::RegistryKey;

use crate::plugin::ui::widget_ast::WidgetAst;

pub const MAX_EXTENSION_RECORDS_PER_KIND: usize = 1024;

/// One overlay registered by a plugin. The widget tree is captured
/// statically — overlay re-rendering on plugin state change is out of
/// scope for extension points.
#[derive(Debug, Clone)]
pub struct OverlayRecord {
    pub plugin_id: String,
    pub id: String,
    pub priority: i32,
    pub dismissible: bool,
    pub key_events: Vec<String>,
    pub widget: WidgetAst,
    pub source_location: Option<String>,
}

impl OverlayRecord {
    pub fn listens_for_key(&self, key: &str) -> bool {
        self.key_events.iter().any(|candidate| candidate == key)
    }
}

pub fn normalize_overlay_key_event(raw: &str) -> Option<&'static str> {
    let compact: String = raw
        .trim()
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .flat_map(char::to_lowercase)
        .collect();

    Some(match compact.as_str() {
        "enter" | "return" => "enter",
        "esc" | "escape" => "esc",
        "tab" => "tab",
        "space" => "space",
        "backspace" => "backspace",
        "delete" | "del" => "delete",
        "arrowup" | "up" => "up",
        "arrowdown" | "down" => "down",
        "arrowleft" | "left" => "left",
        "arrowright" | "right" => "right",
        "home" => "home",
        "end" => "end",
        "pageup" => "pageup",
        "pagedown" => "pagedown",
        "f1" => "f1",
        "f2" => "f2",
        "f3" => "f3",
        "f4" => "f4",
        "f5" => "f5",
        "f6" => "f6",
        "f7" => "f7",
        "f8" => "f8",
        "f9" => "f9",
        "f10" => "f10",
        "f11" => "f11",
        "f12" => "f12",
        "f13" => "f13",
        "f14" => "f14",
        "f15" => "f15",
        "f16" => "f16",
        "f17" => "f17",
        "f18" => "f18",
        "f19" => "f19",
        "f20" => "f20",
        "f21" => "f21",
        "f22" => "f22",
        "f23" => "f23",
        "f24" => "f24",
        _ => return None,
    })
}

/// Lua callbacks owned by plugin overlays. Shared between the Lua API
/// closure and the loaded host record so overlays registered after init
/// still dispatch.
#[derive(Default)]
pub struct OverlayCallbacks {
    handlers: HashMap<String, RegistryKey>,
}

impl OverlayCallbacks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, overlay_id: String, key: RegistryKey) {
        self.handlers.insert(overlay_id, key);
    }

    pub fn remove(&mut self, overlay_id: &str) {
        self.handlers.remove(overlay_id);
    }

    pub fn get(&self, overlay_id: &str) -> Option<&RegistryKey> {
        self.handlers.get(overlay_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationProviderTarget {
    GraphRowBadge,
    DiffLineGutter,
}

impl DecorationProviderTarget {
    pub fn point_id(self) -> &'static str {
        match self {
            Self::GraphRowBadge => "repository.graph.row_badge",
            Self::DiffLineGutter => "repository.diff.line_gutter",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::GraphRowBadge => "graph_row_badge",
            Self::DiffLineGutter => "diff_line_gutter",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecorationProviderRecord {
    pub plugin_id: String,
    pub id: String,
    pub point_id: String,
    pub target: DecorationProviderTarget,
    pub source_location: Option<String>,
}

impl DecorationProviderRecord {
    pub fn handle(&self) -> String {
        format!("{}:{}", self.point_id, self.id)
    }
}

#[derive(Default)]
pub struct DecorationProviderCallbacks {
    handlers: HashMap<String, RegistryKey>,
}

impl DecorationProviderCallbacks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, handle: String, key: RegistryKey) {
        self.handlers.insert(handle, key);
    }

    pub fn remove(&mut self, handle: &str) {
        self.handlers.remove(handle);
    }

    pub fn get(&self, handle: &str) -> Option<&RegistryKey> {
        self.handlers.get(handle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationInvalidationReason {
    Selection,
    DiffLoaded,
    RefsChanged,
    PluginState,
}

impl DecorationInvalidationReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::DiffLoaded => "diff_load",
            Self::RefsChanged => "refs_change",
            Self::PluginState => "plugin_state",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecorationInvalidationRecord {
    pub revision: u64,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ExtensionContributionRecord {
    pub plugin_id: String,
    pub point_id: String,
    pub kind: String,
    pub id: String,
    pub resource_kind: String,
    pub owner_key: String,
    pub dynamic: bool,
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
    decoration_providers: Vec<DecorationProviderRecord>,
    decoration_revision: u64,
    decoration_invalidations: Vec<DecorationInvalidationRecord>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_overlay(&self, record: OverlayRecord) {
        let mut inner = self.inner.borrow_mut();
        inner
            .overlays
            .retain(|o| !(o.plugin_id == record.plugin_id && o.id == record.id));
        inner.overlays.push(record);
        cap_vec(&mut inner.overlays);
    }

    pub fn remove_overlay(&self, plugin_id: &str, id: &str) {
        let mut inner = self.inner.borrow_mut();
        inner
            .overlays
            .retain(|o| !(o.plugin_id == plugin_id && o.id == id));
    }

    pub fn add_context_menu_item(&self, record: ContextMenuItemRecord) {
        let mut inner = self.inner.borrow_mut();
        inner.context_menu_items.retain(|item| {
            !(item.plugin_id == record.plugin_id
                && item.region == record.region
                && item.id == record.id)
        });
        inner.context_menu_items.push(record);
        cap_vec(&mut inner.context_menu_items);
    }

    pub fn add_graph_decoration(&self, record: GraphDecorationRecord) {
        let mut inner = self.inner.borrow_mut();
        inner.graph_decorations.retain(|d| {
            !(d.plugin_id == record.plugin_id
                && d.commit_hash == record.commit_hash
                && d.id == record.id)
        });
        inner.graph_decorations.push(record);
        cap_vec(&mut inner.graph_decorations);
        invalidate_inner(&mut inner, DecorationInvalidationReason::PluginState);
    }

    pub fn add_diff_decoration(&self, record: DiffDecorationRecord) {
        let mut inner = self.inner.borrow_mut();
        inner
            .diff_decorations
            .retain(|d| !(d.plugin_id == record.plugin_id && d.id == record.id));
        inner.diff_decorations.push(record);
        cap_vec(&mut inner.diff_decorations);
        invalidate_inner(&mut inner, DecorationInvalidationReason::PluginState);
    }

    pub fn add_decoration_provider(&self, record: DecorationProviderRecord) {
        let mut inner = self.inner.borrow_mut();
        inner.decoration_providers.retain(|d| {
            !(d.plugin_id == record.plugin_id && d.point_id == record.point_id && d.id == record.id)
        });
        inner.decoration_providers.push(record);
        cap_vec(&mut inner.decoration_providers);
        invalidate_inner(&mut inner, DecorationInvalidationReason::PluginState);
    }

    pub fn remove_context_menu_item(&self, plugin_id: &str, point_id: &str, id: &str) {
        let mut inner = self.inner.borrow_mut();
        inner.context_menu_items.retain(|item| {
            !(item.plugin_id == plugin_id && item.region == point_id && item.id == id)
        });
    }

    pub fn remove_graph_decoration(&self, plugin_id: &str, commit_hash: &str, id: &str) {
        let mut inner = self.inner.borrow_mut();
        inner
            .graph_decorations
            .retain(|d| !(d.plugin_id == plugin_id && d.commit_hash == commit_hash && d.id == id));
        invalidate_inner(&mut inner, DecorationInvalidationReason::PluginState);
    }

    pub fn remove_diff_decoration(&self, plugin_id: &str, id: &str) {
        let mut inner = self.inner.borrow_mut();
        inner
            .diff_decorations
            .retain(|d| !(d.plugin_id == plugin_id && d.id == id));
        invalidate_inner(&mut inner, DecorationInvalidationReason::PluginState);
    }

    pub fn remove_decoration_provider(&self, plugin_id: &str, point_id: &str, id: &str) {
        let mut inner = self.inner.borrow_mut();
        inner
            .decoration_providers
            .retain(|d| !(d.plugin_id == plugin_id && d.point_id == point_id && d.id == id));
        invalidate_inner(&mut inner, DecorationInvalidationReason::PluginState);
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
        inner
            .decoration_providers
            .retain(|d| d.plugin_id != plugin_id);
        invalidate_inner(&mut inner, DecorationInvalidationReason::PluginState);
    }

    pub fn replace_plugin_records_from(&self, plugin_id: &str, staged: &ExtensionRegistry) {
        let staged_inner = staged.inner.borrow();
        let mut inner = self.inner.borrow_mut();
        inner.overlays.retain(|o| o.plugin_id != plugin_id);
        inner
            .context_menu_items
            .retain(|item| item.plugin_id != plugin_id);
        inner.graph_decorations.retain(|d| d.plugin_id != plugin_id);
        inner.diff_decorations.retain(|d| d.plugin_id != plugin_id);
        inner
            .decoration_providers
            .retain(|d| d.plugin_id != plugin_id);

        inner.overlays.extend(
            staged_inner
                .overlays
                .iter()
                .filter(|o| o.plugin_id == plugin_id)
                .cloned(),
        );
        inner.context_menu_items.extend(
            staged_inner
                .context_menu_items
                .iter()
                .filter(|item| item.plugin_id == plugin_id)
                .cloned(),
        );
        inner.graph_decorations.extend(
            staged_inner
                .graph_decorations
                .iter()
                .filter(|d| d.plugin_id == plugin_id)
                .cloned(),
        );
        inner.diff_decorations.extend(
            staged_inner
                .diff_decorations
                .iter()
                .filter(|d| d.plugin_id == plugin_id)
                .cloned(),
        );
        inner.decoration_providers.extend(
            staged_inner
                .decoration_providers
                .iter()
                .filter(|d| d.plugin_id == plugin_id)
                .cloned(),
        );

        cap_vec(&mut inner.overlays);
        cap_vec(&mut inner.context_menu_items);
        cap_vec(&mut inner.graph_decorations);
        cap_vec(&mut inner.diff_decorations);
        cap_vec(&mut inner.decoration_providers);
        invalidate_inner(&mut inner, DecorationInvalidationReason::PluginState);
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

    pub fn diff_decorations_for_line(&self, file: &str, line: u32) -> Vec<DiffDecorationRecord> {
        let inner = self.inner.borrow();
        let mut v: Vec<DiffDecorationRecord> = inner
            .diff_decorations
            .iter()
            .filter(|d| diff_decoration_matches_line(&d.decoration, file, line))
            .cloned()
            .collect();
        v.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id).then(a.id.cmp(&b.id)));
        v
    }

    pub fn decoration_providers(
        &self,
        target: DecorationProviderTarget,
    ) -> Vec<DecorationProviderRecord> {
        let inner = self.inner.borrow();
        let mut v: Vec<DecorationProviderRecord> = inner
            .decoration_providers
            .iter()
            .filter(|p| p.target == target)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id).then(a.id.cmp(&b.id)));
        v
    }

    pub fn all_decoration_providers(&self) -> Vec<DecorationProviderRecord> {
        let inner = self.inner.borrow();
        let mut v = inner.decoration_providers.clone();
        v.sort_by(|a, b| {
            a.point_id
                .cmp(&b.point_id)
                .then(a.plugin_id.cmp(&b.plugin_id))
                .then(a.id.cmp(&b.id))
        });
        v
    }

    pub fn invalidate_decorations(&self, reason: DecorationInvalidationReason) {
        invalidate_inner(&mut self.inner.borrow_mut(), reason);
    }

    pub fn decoration_revision(&self) -> u64 {
        self.inner.borrow().decoration_revision
    }

    pub fn decoration_invalidations(&self) -> Vec<DecorationInvalidationRecord> {
        self.inner.borrow().decoration_invalidations.clone()
    }

    pub fn all_contributions(&self) -> Vec<ExtensionContributionRecord> {
        let inner = self.inner.borrow();
        let mut rows = Vec::new();
        for o in &inner.overlays {
            rows.push(ExtensionContributionRecord {
                plugin_id: o.plugin_id.clone(),
                point_id: "overlays".into(),
                kind: "Overlay".into(),
                id: o.id.clone(),
                resource_kind: "overlay".into(),
                owner_key: format!("{}:{}", o.plugin_id, o.id),
                dynamic: false,
                source_location: o.source_location.clone(),
            });
        }
        for c in &inner.context_menu_items {
            rows.push(ExtensionContributionRecord {
                plugin_id: c.plugin_id.clone(),
                point_id: c.region.clone(),
                kind: "ContextMenu".into(),
                id: c.id.clone(),
                resource_kind: "context_menu_item".into(),
                owner_key: format!("{}:{}:{}", c.plugin_id, c.region, c.id),
                dynamic: false,
                source_location: c.source_location.clone(),
            });
        }
        for g in &inner.graph_decorations {
            rows.push(ExtensionContributionRecord {
                plugin_id: g.plugin_id.clone(),
                point_id: graph_point_id(&g.decoration).into(),
                kind: "Decoration".into(),
                id: g.id.clone(),
                resource_kind: "graph_decoration".into(),
                owner_key: format!("{}:{}:{}", g.plugin_id, g.commit_hash, g.id),
                dynamic: false,
                source_location: g.source_location.clone(),
            });
        }
        for d in &inner.diff_decorations {
            rows.push(ExtensionContributionRecord {
                plugin_id: d.plugin_id.clone(),
                point_id: diff_point_id(&d.decoration).into(),
                kind: "Decoration".into(),
                id: d.id.clone(),
                resource_kind: "diff_decoration".into(),
                owner_key: format!("{}:{}", d.plugin_id, d.id),
                dynamic: false,
                source_location: d.source_location.clone(),
            });
        }
        for p in &inner.decoration_providers {
            rows.push(ExtensionContributionRecord {
                plugin_id: p.plugin_id.clone(),
                point_id: p.point_id.clone(),
                kind: "Decoration".into(),
                id: p.id.clone(),
                resource_kind: format!("{}_provider", p.target.as_str()),
                owner_key: format!("{}:{}:{}", p.plugin_id, p.point_id, p.id),
                dynamic: true,
                source_location: p.source_location.clone(),
            });
        }
        rows.sort_by(|a, b| {
            a.point_id
                .cmp(&b.point_id)
                .then(a.plugin_id.cmp(&b.plugin_id))
                .then(a.id.cmp(&b.id))
        });
        rows
    }
}

fn cap_vec<T>(values: &mut Vec<T>) {
    if values.len() > MAX_EXTENSION_RECORDS_PER_KIND {
        let overflow = values.len() - MAX_EXTENSION_RECORDS_PER_KIND;
        values.drain(0..overflow);
    }
}

fn invalidate_inner(inner: &mut ExtensionInner, reason: DecorationInvalidationReason) {
    inner.decoration_revision = inner.decoration_revision.wrapping_add(1);
    inner
        .decoration_invalidations
        .push(DecorationInvalidationRecord {
            revision: inner.decoration_revision,
            reason: reason.as_str().to_string(),
        });
    if inner.decoration_invalidations.len() > 32 {
        let overflow = inner.decoration_invalidations.len() - 32;
        inner.decoration_invalidations.drain(0..overflow);
    }
}

fn diff_decoration_matches_line(decoration: &DiffDecoration, file: &str, line: u32) -> bool {
    match decoration {
        DiffDecoration::LineHint {
            file: f, line: l, ..
        }
        | DiffDecoration::LineGutter {
            file: f, line: l, ..
        } => f == file && *l == line,
        DiffDecoration::HunkBadge { .. } => false,
    }
}

fn graph_point_id(decoration: &GraphDecoration) -> &'static str {
    match decoration {
        GraphDecoration::Badge { .. } => "repository.graph.row_badge",
        GraphDecoration::Icon { .. } => "repository.graph.row_icon",
        GraphDecoration::Marker { .. } => "repository.graph.row_marker",
        GraphDecoration::Lane { .. } => "repository.graph.lane",
    }
}

fn diff_point_id(decoration: &DiffDecoration) -> &'static str {
    match decoration {
        DiffDecoration::LineHint { .. } => "repository.diff.line_hint",
        DiffDecoration::HunkBadge { .. } => "repository.diff.hunk_badge",
        DiffDecoration::LineGutter { .. } => "repository.diff.line_gutter",
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
                key_events: Vec::new(),
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
    fn remove_overlay_drops_only_matching_owner_and_id() {
        let reg = ExtensionRegistry::new();
        for (plugin_id, id) in [("p1", "same"), ("p2", "same"), ("p1", "other")] {
            reg.add_overlay(OverlayRecord {
                plugin_id: plugin_id.into(),
                id: id.into(),
                priority: 0,
                dismissible: true,
                key_events: Vec::new(),
                widget: fake_widget(),
                source_location: None,
            });
        }

        reg.remove_overlay("p1", "same");

        let remaining = reg
            .overlays()
            .into_iter()
            .map(|o| (o.plugin_id, o.id))
            .collect::<Vec<_>>();
        assert_eq!(
            remaining,
            vec![
                ("p1".to_string(), "other".to_string()),
                ("p2".to_string(), "same".to_string())
            ]
        );
    }

    #[test]
    fn overlay_key_names_normalize_common_named_aliases() {
        assert_eq!(normalize_overlay_key_event("Tab"), Some("tab"));
        assert_eq!(normalize_overlay_key_event("ArrowUp"), Some("up"));
        assert_eq!(normalize_overlay_key_event("page-down"), Some("pagedown"));
        assert_eq!(normalize_overlay_key_event("F12"), Some("f12"));
        assert_eq!(normalize_overlay_key_event("nope"), None);
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

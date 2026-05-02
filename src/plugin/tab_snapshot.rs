//! Snapshot of `App::tabs` shared with the plugin layer.
//!
//! Pushed into every plugin's `leviathan.tab_registry` global, threaded into
//! widget rendering contexts so the `tablist` widget kind can render the live
//! tab list without re-querying app state. Plain owned data so it can be
//! cloned into iced messages and `'static` widget closures freely.

use crate::core::TabId;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TabsSnapshot {
    pub tabs: Vec<TabSnapshotEntry>,
    pub active_id: Option<TabId>,
    pub active_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TabSnapshotEntry {
    pub id: TabId,
    pub path: String,
    pub name: String,
}

impl TabsSnapshot {
    pub fn paths(&self) -> Vec<&str> {
        self.tabs.iter().map(|t| t.path.as_str()).collect()
    }
}

/// Pending tab-registry mutation requested by Lua. The host queues these
/// during plugin calls; the app drains the queue after each update tick
/// and applies them through the canonical `TabManager` paths so settings
/// persistence and screen lifecycle stay in sync.
#[derive(Debug, Clone)]
pub enum TabRegistryOp {
    Add(String),
    Remove(String),
    Select(String),
    Reorder(Vec<String>),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TabChange {
    pub added: bool,
    pub removed: bool,
    pub reordered: bool,
    pub selected_changed: bool,
}

impl TabChange {
    pub fn diff(prev: &TabsSnapshot, current: &TabsSnapshot) -> Self {
        use std::collections::HashSet;
        let prev_paths: HashSet<&str> = prev.paths().into_iter().collect();
        let cur_paths: HashSet<&str> = current.paths().into_iter().collect();
        let added = cur_paths.difference(&prev_paths).next().is_some();
        let removed = prev_paths.difference(&cur_paths).next().is_some();
        let reordered = prev_paths == cur_paths && prev.paths() != current.paths();
        let selected_changed = prev.active_path != current.active_path;
        Self { added, removed, reordered, selected_changed }
    }
}

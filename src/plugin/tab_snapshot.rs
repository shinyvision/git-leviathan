//! Snapshot of `App::tabs` shared with the plugin layer.
//!
//! Pushed into every plugin's `leviathan.tab_registry` global, threaded into
//! widget rendering contexts so the `tablist` widget kind can render the live
//! tab list without re-querying app state. Plain owned data so it can be
//! cloned into iced messages and `'static` widget closures freely.
//!
//! The snapshot holds the tabs of the active workspace only — exactly what
//! the tab bar shows.

use crate::core::TabId;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TabsSnapshot {
    pub tabs: Vec<TabSnapshotEntry>,
    pub active_id: Option<TabId>,
    pub active_path: Option<String>,
    /// Workspace the tabs belong to (`WorkspaceId::raw`).
    pub workspace_id: u64,
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

    fn active_entry(&self) -> Option<TabSnapshotEntry> {
        self.active_id
            .and_then(|id| self.tabs.iter().find(|tab| tab.id == id).cloned())
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

#[derive(Debug, Clone, Default)]
pub struct TabChange {
    pub added: bool,
    pub removed: bool,
    pub reordered: bool,
    pub selected_changed: bool,
    pub added_entry: Option<TabSnapshotEntry>,
    pub removed_entry: Option<TabSnapshotEntry>,
    pub selected_entry: Option<TabSnapshotEntry>,
    pub count: usize,
}

impl TabChange {
    pub fn diff(prev: &TabsSnapshot, current: &TabsSnapshot) -> Self {
        use std::collections::HashSet;
        if prev.workspace_id != current.workspace_id {
            // A workspace switch swaps the visible tab set without opening,
            // closing or moving any tab, so only the selection is reported.
            let selected_changed = prev.active_path != current.active_path;
            return Self {
                selected_changed,
                selected_entry: if selected_changed {
                    current.active_entry()
                } else {
                    None
                },
                count: current.tabs.len(),
                ..Self::default()
            };
        }
        let prev_paths: HashSet<&str> = prev.paths().into_iter().collect();
        let cur_paths: HashSet<&str> = current.paths().into_iter().collect();
        let added_entry = current
            .tabs
            .iter()
            .find(|tab| !prev_paths.contains(tab.path.as_str()))
            .cloned();
        let removed_entry = prev
            .tabs
            .iter()
            .find(|tab| !cur_paths.contains(tab.path.as_str()))
            .cloned();
        let reordered = prev_paths == cur_paths && prev.paths() != current.paths();
        let selected_changed = prev.active_path != current.active_path;
        let selected_entry = if selected_changed {
            current.active_entry()
        } else {
            None
        };
        Self {
            added: added_entry.is_some(),
            removed: removed_entry.is_some(),
            reordered,
            selected_changed,
            added_entry,
            removed_entry,
            selected_entry,
            count: current.tabs.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, path: &str) -> TabSnapshotEntry {
        TabSnapshotEntry {
            id: TabId(id),
            path: path.to_string(),
            name: path.to_string(),
        }
    }

    fn snapshot(workspace_id: u64, tabs: &[(u64, &str)], active: Option<u64>) -> TabsSnapshot {
        let tabs: Vec<TabSnapshotEntry> = tabs.iter().map(|(id, p)| entry(*id, p)).collect();
        let active_id = active.map(TabId);
        let active_path = active_id
            .and_then(|id| tabs.iter().find(|t| t.id == id))
            .map(|t| t.path.clone());
        TabsSnapshot {
            tabs,
            active_id,
            active_path,
            workspace_id,
        }
    }

    #[test]
    fn same_workspace_reports_adds_and_removes() {
        let prev = snapshot(0, &[(1, "/a")], Some(1));
        let cur = snapshot(0, &[(1, "/a"), (2, "/b")], Some(2));
        let change = TabChange::diff(&prev, &cur);
        assert!(change.added);
        assert_eq!(change.added_entry.map(|e| e.path), Some("/b".into()));
        assert!(change.selected_changed);
    }

    #[test]
    fn workspace_switch_reports_only_selection() {
        let prev = snapshot(0, &[(1, "/a"), (2, "/b")], Some(1));
        let cur = snapshot(5, &[(3, "/c")], Some(3));
        let change = TabChange::diff(&prev, &cur);
        assert!(!change.added && !change.removed && !change.reordered);
        assert!(change.selected_changed);
        assert_eq!(change.selected_entry.map(|e| e.path), Some("/c".into()));
        assert_eq!(change.count, 1);
    }

    #[test]
    fn switch_to_empty_workspace_clears_selection() {
        let prev = snapshot(0, &[(1, "/a")], Some(1));
        let cur = snapshot(5, &[], None);
        let change = TabChange::diff(&prev, &cur);
        assert!(change.selected_changed);
        assert!(change.selected_entry.is_none());
        assert!(!change.removed);
    }
}

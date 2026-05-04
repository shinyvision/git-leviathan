//! `TabManager` — owns the open repository tabs and their `RepositoryScreen`s.
//!
//! Pulled out of `App` in keymaps. Everything tab-lifecycle-shaped lives here:
//! open, close, activate (with hibernate/rehydrate), circular Ctrl+Tab
//! navigation, and the "most recent repo" bookkeeping that persists across
//! app restarts via the settings DB.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use iced::{keyboard, Task};

use crate::{
    core::TabId,
    message::Message,
    screens::plugin::{PluginScreen, PluginScreenSummary},
    screens::repository::state::GatewayFleet,
    screens::RepositoryScreen,
    services::{resolve_primary_and_active, GitRepositoryGateway, Presenter, SettingsService},
};

pub struct TabEntry {
    pub id: TabId,
    pub name: String,
    pub kind: TabKind,
}

#[derive(Clone)]
pub enum TabKind {
    Repository { path: String },
    Plugin { key: String },
}

impl TabEntry {
    pub fn path_key(&self) -> &str {
        match &self.kind {
            TabKind::Repository { path } => path,
            TabKind::Plugin { key, .. } => key,
        }
    }

    pub fn repo_path(&self) -> Option<&str> {
        match &self.kind {
            TabKind::Repository { path } => Some(path),
            TabKind::Plugin { .. } => None,
        }
    }
}

pub struct TabManager {
    tabs: Vec<TabEntry>,
    screens: HashMap<TabId, RepositoryScreen>,
    plugin_screens: HashMap<TabId, PluginScreen>,
    active_tab_id: TabId,
    next_tab_id: TabId,
    /// Tracks whether the currently active tab has already been written to the
    /// SQLite `most_recent_repo` slot. Reset whenever the active tab changes
    /// so the next fetch for that tab persists it; kept true otherwise to
    /// avoid redundant DB writes on every fetch tick.
    active_tab_persisted_as_recent: bool,
    presenter: Arc<dyn Presenter>,
}

#[derive(Debug)]
pub enum OpenRepoError {
    NotARepo(PathBuf),
}

impl TabManager {
    pub fn new(presenter: Arc<dyn Presenter>) -> Self {
        Self {
            tabs: Vec::new(),
            screens: HashMap::new(),
            plugin_screens: HashMap::new(),
            active_tab_id: TabId(0),
            next_tab_id: TabId(0),
            active_tab_persisted_as_recent: false,
            presenter,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active_tab_id(&self) -> TabId {
        self.active_tab_id
    }

    pub fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }

    pub fn tab_id_for_path(&self, path: &str) -> Option<TabId> {
        self.tabs
            .iter()
            .find(|t| t.path_key() == path)
            .map(|t| t.id)
    }

    pub fn first_repository_tab_id(&self) -> Option<TabId> {
        self.tabs
            .iter()
            .find(|t| matches!(t.kind, TabKind::Repository { .. }))
            .map(|t| t.id)
    }

    pub fn active_entry(&self) -> Option<&TabEntry> {
        self.tabs.iter().find(|t| t.id == self.active_tab_id)
    }

    pub fn active_screen(&self) -> Option<&RepositoryScreen> {
        self.screens.get(&self.active_tab_id)
    }

    pub fn active_screen_mut(&mut self) -> Option<&mut RepositoryScreen> {
        self.screens.get_mut(&self.active_tab_id)
    }

    pub fn active_plugin_screen(&self) -> Option<&PluginScreen> {
        self.plugin_screens.get(&self.active_tab_id)
    }

    pub fn screen(&self, tab_id: TabId) -> Option<&RepositoryScreen> {
        self.screens.get(&tab_id)
    }

    pub fn screen_mut(&mut self, tab_id: TabId) -> Option<&mut RepositoryScreen> {
        self.screens.get_mut(&tab_id)
    }

    pub fn plugin_screen(&self, tab_id: TabId) -> Option<&PluginScreen> {
        self.plugin_screens.get(&tab_id)
    }

    /// Create one tab per `repo_paths` entry. Sets the active tab to the one
    /// matching `most_recent` (if any); else the first. Returns the initial-
    /// load tasks for each new screen.
    pub fn load_initial_repos(
        &mut self,
        repo_paths: Vec<String>,
        most_recent: Option<String>,
    ) -> Vec<Task<Message>> {
        let mut initial_tasks = Vec::new();
        let mut most_recent_tab_id: Option<TabId> = None;
        for repo_path in repo_paths {
            let (tab_id, task) = self.push_new_tab(repo_path.clone());
            if most_recent.as_deref() == Some(repo_path.as_str()) {
                most_recent_tab_id = Some(tab_id);
            }
            initial_tasks.push(task);
        }

        if let Some(id) = most_recent_tab_id {
            self.active_tab_id = id;
            self.active_tab_persisted_as_recent = true;
        } else if let Some(first) = self.tabs.first() {
            self.active_tab_id = first.id;
            self.active_tab_persisted_as_recent = false;
        }
        initial_tasks
    }

    fn push_new_tab(&mut self, repo_path: String) -> (TabId, Task<Message>) {
        let name = tab_name_from_path(&repo_path);
        let tab_id = self.next_tab_id;
        self.next_tab_id = TabId(self.next_tab_id.raw() + 1);

        let (primary_path, active_path) =
            match resolve_primary_and_active(std::path::Path::new(&repo_path)) {
                Ok(pair) => pair,
                Err(_) => {
                    let p = std::path::PathBuf::from(&repo_path);
                    (p.clone(), p)
                }
            };
        let primary_gateway =
            GitRepositoryGateway::from_path(primary_path.to_string_lossy().to_string());
        let active_gateway = if active_path == primary_path {
            primary_gateway.clone()
        } else {
            GitRepositoryGateway::from_path(active_path.to_string_lossy().to_string())
        };
        let fleet = GatewayFleet::new(primary_path, primary_gateway, active_path, active_gateway);
        let screen = RepositoryScreen::new(fleet, self.presenter.clone(), tab_id);
        let task = screen.initial_load_task(tab_id);

        self.tabs.push(TabEntry {
            id: tab_id,
            name,
            kind: TabKind::Repository { path: repo_path },
        });
        self.screens.insert(tab_id, screen);
        (tab_id, task)
    }

    /// Open a new tab for `path`. If already open, just switches to it and
    /// returns `Task::none()`. Fails if `path` isn't a git repo.
    pub fn open_path(&mut self, path: PathBuf) -> Result<Task<Message>, OpenRepoError> {
        let is_git_repo = path.join(".git").exists()
            || path.join("HEAD").exists() && path.join("objects").exists();
        if !is_git_repo {
            return Err(OpenRepoError::NotARepo(path));
        }
        let repo_path = path.to_string_lossy().to_string();

        if let Some(existing_tab) = self.tabs.iter().find(|t| t.path_key() == repo_path) {
            if self.active_tab_id != existing_tab.id {
                self.active_tab_id = existing_tab.id;
                self.active_tab_persisted_as_recent = false;
            }
            return Ok(Task::none());
        }

        if let Ok(settings) = SettingsService::new() {
            let _ = settings.add_repo(&repo_path);
        }

        let (tab_id, task) = self.push_new_tab(repo_path);
        self.active_tab_id = tab_id;
        self.active_tab_persisted_as_recent = false;
        Ok(task)
    }

    pub fn open_plugin_screen(
        &mut self,
        summary: PluginScreenSummary,
        bound_repo_path: Option<String>,
    ) -> TabId {
        let key = plugin_tab_key(&summary.plugin_id, &summary.screen_id);
        if let Some(existing_tab) = self.tabs.iter().find(|t| t.path_key() == key) {
            let id = existing_tab.id;
            let _ = self.activate_tab(id);
            return id;
        }

        let tab_id = self.next_tab_id;
        self.next_tab_id = TabId(self.next_tab_id.raw() + 1);
        let screen = PluginScreen::new(summary.clone(), bound_repo_path);
        self.tabs.push(TabEntry {
            id: tab_id,
            name: summary.title,
            kind: TabKind::Plugin { key },
        });
        self.plugin_screens.insert(tab_id, screen);
        let _ = self.activate_tab(tab_id);
        tab_id
    }

    /// Remove a tab on user-initiated close. Writes the removal to settings
    /// so it doesn't re-open on next launch.
    pub fn close_tab(&mut self, tab_id: TabId) {
        let tab_idx = self.tabs.iter().position(|t| t.id == tab_id);
        let repo_path = tab_idx.and_then(|i| self.tabs[i].repo_path().map(ToOwned::to_owned));
        self.remove_tab_inner(tab_id, tab_idx);
        if let (Ok(settings), Some(path)) = (SettingsService::new(), repo_path) {
            let _ = settings.remove_repo(&path);
        }
    }

    /// Remove a tab whose initial open failed (GitError). Does not touch
    /// settings — a repo that failed to open should stay in the persisted
    /// list so the user can retry next launch.
    pub fn drop_failed_tab(&mut self, tab_id: TabId) {
        let tab_idx = self.tabs.iter().position(|t| t.id == tab_id);
        self.remove_tab_inner(tab_id, tab_idx);
    }

    fn remove_tab_inner(&mut self, tab_id: TabId, tab_idx: Option<usize>) {
        self.tabs.retain(|t| t.id != tab_id);
        self.screens.remove(&tab_id);
        self.plugin_screens.remove(&tab_id);
        if self.active_tab_id == tab_id {
            let new_idx = tab_idx
                .map(|i| if i > 0 { i - 1 } else { 0 })
                .unwrap_or(0)
                .min(self.tabs.len().saturating_sub(1));
            if let Some(tab) = self.tabs.get(new_idx) {
                self.active_tab_id = tab.id;
                self.active_tab_persisted_as_recent = false;
            }
        }
    }

    /// Next tab id for Ctrl+Tab. `None` when fewer than two tabs exist.
    pub fn next_tab_id_circular(&self) -> Option<TabId> {
        if self.tabs.len() < 2 {
            return None;
        }
        let current_idx = self
            .tabs
            .iter()
            .position(|t| t.id == self.active_tab_id)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % self.tabs.len();
        Some(self.tabs[next_idx].id)
    }

    /// Previous tab id for Ctrl+Shift+Tab. `None` when fewer than two tabs exist.
    pub fn prev_tab_id_circular(&self) -> Option<TabId> {
        if self.tabs.len() < 2 {
            return None;
        }
        let current_idx = self
            .tabs
            .iter()
            .position(|t| t.id == self.active_tab_id)
            .unwrap_or(0);
        let prev_idx = (current_idx + self.tabs.len() - 1) % self.tabs.len();
        Some(self.tabs[prev_idx].id)
    }

    /// Select a tab directly (e.g. from tab-bar click). Skips work if it's
    /// already active or doesn't exist.
    pub fn select(&mut self, tab_id: TabId) -> Task<Message> {
        self.activate_tab(tab_id)
    }

    /// Apply a new tab ordering (from a drag-reorder commit). Reorders
    /// `self.tabs` to match `new_order` and persists. Ids not in
    /// `self.tabs` are ignored; missing ids retain their relative position
    /// at the end.
    pub fn reorder(&mut self, new_order: Vec<TabId>) {
        let mut by_id: HashMap<TabId, TabEntry> = self.tabs.drain(..).map(|t| (t.id, t)).collect();
        let mut reordered: Vec<TabEntry> = Vec::with_capacity(by_id.len());
        for id in new_order {
            if let Some(entry) = by_id.remove(&id) {
                reordered.push(entry);
            }
        }
        for (_, entry) in by_id {
            reordered.push(entry);
        }
        self.tabs = reordered;
        self.persist_tab_order();
    }

    fn persist_tab_order(&self) {
        if let Ok(settings) = SettingsService::new() {
            let paths: Vec<String> = self
                .tabs
                .iter()
                .filter_map(|t| t.repo_path().map(ToOwned::to_owned))
                .collect();
            let _ = settings.set_repo_order(&paths);
        }
    }

    /// Activate `new_tab_id`: hibernate the previous tab, rehydrate or
    /// re-select on the new one so its panels light up immediately. Caller
    /// handles anything fetch-related.
    pub fn activate_tab(&mut self, new_tab_id: TabId) -> Task<Message> {
        let prev_tab_id = self.active_tab_id;
        if prev_tab_id == new_tab_id {
            return Task::none();
        }
        if !self.tabs.iter().any(|t| t.id == new_tab_id) {
            return Task::none();
        }
        if let Some(prev) = self.screens.get_mut(&prev_tab_id) {
            prev.hibernate();
        }
        if let Some(prev) = self.plugin_screens.get_mut(&prev_tab_id) {
            prev.set_focused(false);
        }
        self.active_tab_id = new_tab_id;
        self.active_tab_persisted_as_recent = false;
        if let Some(screen) = self.screens.get_mut(&new_tab_id) {
            // Clear stale modifier state — the newly-active screen only
            // receives ModifiersChanged events from now on, so any keys the
            // user was (or wasn't) holding last time it was active must not
            // carry over. Default means "nothing held"; a real keypress will
            // update it before the user's next click lands.
            screen.on_modifiers_changed(keyboard::Modifiers::default());
            if screen.is_hibernated() {
                screen.rehydrate_task()
            } else {
                let selected_idx = screen.get_selected_commit_index();
                screen.select_commit(selected_idx)
            }
        } else {
            if let Some(screen) = self.plugin_screens.get_mut(&new_tab_id) {
                screen.set_focused(true);
            }
            Task::none()
        }
    }

    /// Called once per tab-activation after a fetch kicks off for that tab.
    /// Records the tab's path as "most recent" so the next app launch restores
    /// it as active. Idempotent across a single tab's active session.
    pub fn persist_most_recent_if_needed(&mut self, tab_id: TabId) {
        if tab_id != self.active_tab_id || self.active_tab_persisted_as_recent {
            return;
        }
        if let Some(repo_path) = self
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .and_then(|t| t.repo_path().map(ToOwned::to_owned))
        {
            if let Ok(settings) = SettingsService::new() {
                let _ = settings.set_most_recent_repo(&repo_path);
            }
            self.active_tab_persisted_as_recent = true;
        }
    }
}

pub fn tab_name_from_path(repo_path: &str) -> String {
    std::path::Path::new(repo_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(repo_path)
        .to_string()
}

pub fn plugin_tab_key(plugin_id: &str, screen_id: &str) -> String {
    format!("plugin://{plugin_id}/{screen_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::DefaultPresenter;

    fn make() -> TabManager {
        TabManager::new(Arc::new(DefaultPresenter::new()))
    }

    #[test]
    fn starts_empty() {
        let m = make();
        assert!(m.is_empty());
        assert_eq!(m.tabs().len(), 0);
        assert!(m.active_screen().is_none());
    }

    #[test]
    fn circular_nav_requires_two_tabs() {
        let m = make();
        assert_eq!(m.next_tab_id_circular(), None);
        assert_eq!(m.prev_tab_id_circular(), None);
    }

    #[test]
    fn tab_name_is_basename() {
        assert_eq!(tab_name_from_path("/foo/bar/my_repo"), "my_repo");
        assert_eq!(tab_name_from_path("my_repo"), "my_repo");
        assert_eq!(tab_name_from_path("/"), "/");
    }

    #[test]
    fn activate_same_tab_is_noop() {
        let mut m = make();
        // No tabs — activating any id is a no-op, returns Task::none().
        let _ = m.activate_tab(TabId(0));
        assert!(m.is_empty());
    }

    fn push_tab(m: &mut TabManager, id: u64, path: &str) {
        m.tabs.push(TabEntry {
            id: TabId(id),
            name: path.to_string(),
            kind: TabKind::Repository {
                path: path.to_string(),
            },
        });
    }

    #[test]
    fn reorder_applies_new_order() {
        let mut m = make();
        push_tab(&mut m, 1, "/a");
        push_tab(&mut m, 2, "/b");
        push_tab(&mut m, 3, "/c");
        m.reorder(vec![TabId(2), TabId(3), TabId(1)]);
        let order: Vec<u64> = m.tabs.iter().map(|t| t.id.0).collect();
        assert_eq!(order, vec![2, 3, 1]);
    }

    #[test]
    fn reorder_drops_unknown_ids_and_keeps_missing_at_end() {
        let mut m = make();
        push_tab(&mut m, 1, "/a");
        push_tab(&mut m, 2, "/b");
        push_tab(&mut m, 3, "/c");
        m.reorder(vec![TabId(99), TabId(2), TabId(1)]);
        let mut order: Vec<u64> = m.tabs.iter().map(|t| t.id.0).collect();
        // Missing tab 3 retained at end (insertion order from leftover map is
        // not guaranteed, so just check it's still there).
        assert_eq!(&order[..2], &[2, 1][..]);
        assert!(order.contains(&3));
        order.sort_unstable();
        assert_eq!(order, vec![1, 2, 3]);
    }

    #[test]
    fn open_rejects_non_git_path() {
        let mut m = make();
        let tmp = std::env::temp_dir().join("git_leviathan_tabs_test_not_a_repo");
        let _ = std::fs::create_dir_all(&tmp);
        let result = m.open_path(tmp.clone());
        assert!(matches!(result, Err(OpenRepoError::NotARepo(_))));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

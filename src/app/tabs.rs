//! Owns the open repository tabs and their `RepositoryScreen`s: open, close,
//! activate (with hibernate/rehydrate), circular Ctrl+Tab navigation, and
//! the "most recent repo" bookkeeping persisted via the settings DB.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use iced::{keyboard, Task};

use crate::{
    core::TabId,
    message::Message,
    screens::RepositoryScreen,
    screens::repository::state::GatewayFleet,
    services::{GitRepositoryGateway, Presenter, SettingsService, resolve_primary_and_active},
};

pub struct TabEntry {
    pub id: TabId,
    pub repo_path: String,
    pub name: String,
}

pub struct TabManager {
    tabs: Vec<TabEntry>,
    screens: HashMap<TabId, RepositoryScreen>,
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

    pub fn active_screen(&self) -> Option<&RepositoryScreen> {
        self.screens.get(&self.active_tab_id)
    }

    pub fn active_screen_mut(&mut self) -> Option<&mut RepositoryScreen> {
        self.screens.get_mut(&self.active_tab_id)
    }

    pub fn screen(&self, tab_id: TabId) -> Option<&RepositoryScreen> {
        self.screens.get(&tab_id)
    }

    pub fn screen_mut(&mut self, tab_id: TabId) -> Option<&mut RepositoryScreen> {
        self.screens.get_mut(&tab_id)
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

        let (primary_path, active_path) = match resolve_primary_and_active(
            std::path::Path::new(&repo_path),
        ) {
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
        let fleet = GatewayFleet::new(
            primary_path,
            primary_gateway,
            active_path,
            active_gateway,
            self.presenter.clone(),
        );
        let screen = RepositoryScreen::new(fleet, self.presenter.clone(), tab_id);
        let task = screen.initial_load_task(tab_id);

        self.tabs.push(TabEntry {
            id: tab_id,
            repo_path,
            name,
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

        if let Some(existing_tab) = self.tabs.iter().find(|t| t.repo_path == repo_path) {
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

    /// Remove a tab on user-initiated close. Writes the removal to settings
    /// so it doesn't re-open on next launch.
    pub fn close_tab(&mut self, tab_id: TabId) {
        let tab_idx = self.tabs.iter().position(|t| t.id == tab_id);
        let repo_path = tab_idx.map(|i| self.tabs[i].repo_path.clone());
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
            .map(|t| t.repo_path.clone())
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

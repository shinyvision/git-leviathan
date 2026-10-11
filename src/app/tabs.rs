//! `TabManager` — owns the open repository tabs and their `RepositoryScreen`s.
//!
//! Pulled out of `App` in keymaps. Everything tab-lifecycle-shaped lives here:
//! open, close, activate (with hibernate/rehydrate), circular Ctrl+Tab
//! navigation, and the "most recent repo" bookkeeping that persists across
//! app restarts via the settings DB.
//!
//! Workspaces: every tab belongs to one workspace and only the active
//! workspace's tabs are "visible" (shown in the tab bar, reachable with
//! Ctrl+Tab). The active tab is always a visible tab, or `NO_TAB` when the
//! active workspace is empty. `tabs` keeps one global order; a workspace's
//! tab order is the relative order of its members in it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use iced::{keyboard, Task};

use super::workspaces::{Workspace, Workspaces};
use crate::{
    core::{TabId, WorkspaceId},
    message::Message,
    screens::plugin::{PluginScreen, PluginScreenSummary},
    screens::repository::state::GatewayFleet,
    screens::RepositoryScreen,
    services::{resolve_primary_and_active, GitRepositoryGateway, Presenter, SettingsService},
};

/// `active_tab_id` while the active workspace has no tabs. Never allocated
/// to a real tab, so every per-tab lookup on it misses.
pub const NO_TAB: TabId = TabId(u64::MAX);

pub struct TabEntry {
    pub id: TabId,
    pub name: String,
    pub kind: TabKind,
    pub workspace: WorkspaceId,
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
    /// Monotonic counter bumped on any structural tab change (open, close,
    /// select, reorder). Lets `App` skip rebuilding the tab snapshot and the
    /// persisted-tab list on messages that left the tab set untouched.
    revision: u64,
    presenter: Arc<dyn Presenter>,
    /// Cached settings connection. `SettingsService::new` opens the SQLite DB
    /// and runs migrations; without caching, every tab open/close/reorder and
    /// every fetch-driven most-recent write reopened it on the UI thread.
    settings: Option<SettingsService>,
    /// False in unit tests so they never touch the user's settings DB.
    persistence: bool,
    workspaces: Workspaces,
    active_workspace: WorkspaceId,
    /// Tab each workspace had active when the user last left it.
    workspace_active_tab: HashMap<WorkspaceId, TabId>,
    /// Persisted form of `workspace_active_tab` (tab path keys), loaded at
    /// startup and resolved lazily the first time a workspace is entered —
    /// by then plugin tabs have been restored too.
    remembered_paths: BTreeMap<u64, String>,
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
            active_tab_id: NO_TAB,
            next_tab_id: TabId(0),
            active_tab_persisted_as_recent: false,
            revision: 0,
            presenter,
            settings: None,
            persistence: true,
            workspaces: Workspaces::default(),
            active_workspace: WorkspaceId::DEFAULT,
            workspace_active_tab: HashMap::new(),
            remembered_paths: BTreeMap::new(),
        }
    }

    /// A manager that never reads or writes the settings DB (tests).
    #[cfg(test)]
    pub fn without_persistence(mut self) -> Self {
        self.persistence = false;
        self
    }

    /// Lazily open (once) and return the cached settings connection. Returns
    /// `None` if the DB can't be opened; callers treat persistence as
    /// best-effort just as they did with the per-call `SettingsService::new`.
    fn settings(&mut self) -> Option<&SettingsService> {
        if !self.persistence {
            return None;
        }
        if self.settings.is_none() {
            self.settings = SettingsService::new().ok();
        }
        self.settings.as_ref()
    }

    /// True when the active workspace shows no tabs (there may still be tabs
    /// in other workspaces — see `has_any_tabs`).
    pub fn is_empty(&self) -> bool {
        !self
            .tabs
            .iter()
            .any(|t| t.workspace == self.active_workspace)
    }

    pub fn has_any_tabs(&self) -> bool {
        !self.tabs.is_empty()
    }

    /// Monotonic structural-change counter (see field doc).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn active_tab_id(&self) -> TabId {
        self.active_tab_id
    }

    /// Every open tab across all workspaces, in global order.
    pub fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }

    /// Tabs of the active workspace, in tab-bar order.
    pub fn visible_tabs(&self) -> impl Iterator<Item = &TabEntry> {
        let workspace = self.active_workspace;
        self.tabs.iter().filter(move |t| t.workspace == workspace)
    }

    fn visible_ids(&self) -> Vec<TabId> {
        self.visible_tabs().map(|t| t.id).collect()
    }

    pub fn tab_id_for_path(&self, path: &str) -> Option<TabId> {
        self.tabs
            .iter()
            .find(|t| t.path_key() == path)
            .map(|t| t.id)
    }

    pub fn entry(&self, tab_id: TabId) -> Option<&TabEntry> {
        self.tabs.iter().find(|t| t.id == tab_id)
    }

    /// First repository tab, preferring the active workspace.
    pub fn first_repository_tab_id(&self) -> Option<TabId> {
        let is_repo = |t: &&TabEntry| matches!(t.kind, TabKind::Repository { .. });
        self.visible_tabs()
            .find(is_repo)
            .or_else(|| self.tabs.iter().find(is_repo))
            .map(|t| t.id)
    }

    pub fn workspaces(&self) -> &Workspaces {
        &self.workspaces
    }

    pub fn active_workspace(&self) -> WorkspaceId {
        self.active_workspace
    }

    pub fn tab_count(&self, workspace: WorkspaceId) -> usize {
        self.tabs
            .iter()
            .filter(|t| t.workspace == workspace)
            .count()
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

    /// Load the persisted workspaces, the active workspace and each
    /// workspace's remembered tab. Call before `load_initial_repos`. With
    /// `force_default` (a repo passed on the command line / env) the default
    /// workspace is made active so that repo is visible.
    pub fn restore_workspaces(&mut self, force_default: bool) {
        let Some(settings) = self.settings() else {
            return;
        };
        let rows = settings.load_workspaces().unwrap_or_default();
        let active = settings.get_active_workspace().ok().flatten();
        let remembered = settings.load_workspace_active_tabs().unwrap_or_default();
        self.workspaces = Workspaces::from_persisted(rows);
        self.remembered_paths = remembered;
        self.active_workspace = match active.map(WorkspaceId) {
            Some(id) if !force_default && self.workspaces.contains(id) => id,
            _ => WorkspaceId::DEFAULT,
        };
        self.bump_revision();
    }

    /// Create one tab per `repos` entry, each in its stored workspace
    /// (unknown workspaces fall back to the default). Sets the active tab to
    /// the one matching `most_recent` when it is in the active workspace,
    /// else the workspace's remembered tab, else its first. Returns the
    /// initial-load tasks for each new screen.
    pub fn load_initial_repos(
        &mut self,
        repos: Vec<(String, WorkspaceId)>,
        most_recent: Option<String>,
    ) -> Vec<Task<Message>> {
        let mut initial_tasks = Vec::new();
        let mut most_recent_tab_id: Option<TabId> = None;
        for (repo_path, workspace) in repos {
            let workspace = if self.workspaces.contains(workspace) {
                workspace
            } else {
                WorkspaceId::DEFAULT
            };
            let (tab_id, task) = self.push_new_tab(repo_path.clone(), workspace);
            if most_recent.as_deref() == Some(repo_path.as_str())
                && workspace == self.active_workspace
            {
                most_recent_tab_id = Some(tab_id);
            }
            initial_tasks.push(task);
        }

        // The workspace's remembered tab is at least as fresh as the global
        // most-recent repo (both are written together on fetch); the latter
        // still covers settings saved before workspaces existed.
        if let Some(id) = self.remembered_path_tab(self.active_workspace) {
            self.active_tab_id = id;
            self.active_tab_persisted_as_recent = false;
        } else if let Some(id) = most_recent_tab_id {
            self.active_tab_id = id;
            self.active_tab_persisted_as_recent = true;
        } else if let Some(id) = self.remembered_tab_for(self.active_workspace) {
            self.active_tab_id = id;
            self.active_tab_persisted_as_recent = false;
        } else {
            self.active_tab_id = NO_TAB;
            self.active_tab_persisted_as_recent = false;
        }
        initial_tasks
    }

    /// The tab the active workspace remembers by path, once restored — for
    /// re-selecting a remembered plugin tab after plugin tabs come back.
    pub fn remembered_startup_tab(&self) -> Option<TabId> {
        self.remembered_path_tab(self.active_workspace)
    }

    fn remembered_path_tab(&self, workspace: WorkspaceId) -> Option<TabId> {
        self.remembered_paths
            .get(&workspace.raw())
            .and_then(|path| self.tab_id_for_path(path))
            .filter(|id| self.entry(*id).is_some_and(|t| t.workspace == workspace))
    }

    fn push_new_tab(
        &mut self,
        repo_path: String,
        workspace: WorkspaceId,
    ) -> (TabId, Task<Message>) {
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
            workspace,
        });
        self.screens.insert(tab_id, screen);
        self.bump_revision();
        (tab_id, task)
    }

    /// Open a new tab for `path` in the active workspace. If already open
    /// (in any workspace) just switches to it. Fails if `path` isn't a git
    /// repo.
    pub fn open_path(&mut self, path: PathBuf) -> Result<Task<Message>, OpenRepoError> {
        let is_git_repo = path.join(".git").exists()
            || path.join("HEAD").exists() && path.join("objects").exists();
        if !is_git_repo {
            return Err(OpenRepoError::NotARepo(path));
        }
        let repo_path = path.to_string_lossy().to_string();

        if let Some(existing) = self.tab_id_for_path(&repo_path) {
            return Ok(self.activate_tab(existing));
        }

        let workspace = self.active_workspace;
        if let Some(settings) = self.settings() {
            let _ = settings.add_repo(&repo_path);
            let _ = settings.set_repo_workspace(&repo_path, workspace.raw());
        }

        let (tab_id, task) = self.push_new_tab(repo_path, workspace);
        self.active_tab_id = tab_id;
        self.active_tab_persisted_as_recent = false;
        // `add_repo` files the repo first; store the in-memory order (new tab
        // last) so it reopens where it is shown.
        self.persist_tab_order();
        Ok(task)
    }

    /// Open (or focus) a plugin screen tab. New tabs go into `workspace`
    /// (when it exists) or the active workspace; a tab restored into another
    /// workspace is added without switching to it.
    pub fn open_plugin_screen(
        &mut self,
        summary: PluginScreenSummary,
        bound_repo_path: Option<String>,
        workspace: Option<WorkspaceId>,
    ) -> TabId {
        let key = plugin_tab_key(&summary.plugin_id, &summary.screen_id);
        if let Some(existing_tab) = self.tabs.iter().find(|t| t.path_key() == key) {
            let id = existing_tab.id;
            let _ = self.activate_tab(id);
            return id;
        }

        let workspace = workspace
            .filter(|w| self.workspaces.contains(*w))
            .unwrap_or(self.active_workspace);
        let tab_id = self.next_tab_id;
        self.next_tab_id = TabId(self.next_tab_id.raw() + 1);
        let screen = PluginScreen::new(summary.clone(), bound_repo_path);
        self.tabs.push(TabEntry {
            id: tab_id,
            name: summary.title,
            kind: TabKind::Plugin { key },
            workspace,
        });
        self.plugin_screens.insert(tab_id, screen);
        self.bump_revision();
        if workspace == self.active_workspace {
            let _ = self.activate_tab(tab_id);
        }
        tab_id
    }

    /// Remove a tab on user-initiated close. Writes the removal to settings
    /// so it doesn't re-open on next launch.
    pub fn close_tab(&mut self, tab_id: TabId) -> Task<Message> {
        let tab_idx = self.tabs.iter().position(|t| t.id == tab_id);
        let repo_path = tab_idx.and_then(|i| self.tabs[i].repo_path().map(ToOwned::to_owned));
        let task = self.remove_tab_inner(tab_id, tab_idx);
        if let Some(path) = repo_path {
            if let Some(settings) = self.settings() {
                let _ = settings.remove_repo(&path);
            }
        }
        task
    }

    /// Remove a tab whose initial open failed (GitError). Does not touch
    /// settings — a repo that failed to open should stay in the persisted
    /// list so the user can retry next launch.
    pub fn drop_failed_tab(&mut self, tab_id: TabId) -> Task<Message> {
        let tab_idx = self.tabs.iter().position(|t| t.id == tab_id);
        self.remove_tab_inner(tab_id, tab_idx)
    }

    fn remove_tab_inner(&mut self, tab_id: TabId, tab_idx: Option<usize>) -> Task<Message> {
        let Some(tab_idx) = tab_idx else {
            return Task::none();
        };
        let neighbor = self.neighbor_in_workspace(tab_id);
        self.tabs.remove(tab_idx);
        self.screens.remove(&tab_id);
        self.plugin_screens.remove(&tab_id);
        self.workspace_active_tab.retain(|_, t| *t != tab_id);
        self.bump_revision();
        if self.active_tab_id == tab_id {
            self.active_tab_persisted_as_recent = false;
            match neighbor {
                Some(new_active) => {
                    self.active_tab_id = new_active;
                    return self.wake_tab(new_active);
                }
                None => self.active_tab_id = NO_TAB,
            }
        }
        Task::none()
    }

    /// The tab that takes over when `tab_id` leaves its workspace: its left
    /// neighbour there, else its right one.
    fn neighbor_in_workspace(&self, tab_id: TabId) -> Option<TabId> {
        let workspace = self.entry(tab_id)?.workspace;
        let ids: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| t.workspace == workspace)
            .map(|t| t.id)
            .collect();
        let idx = ids.iter().position(|id| *id == tab_id)?;
        if idx > 0 {
            Some(ids[idx - 1])
        } else {
            ids.get(1).copied()
        }
    }

    /// Bring a tab that just became active back to life without a full
    /// `activate_tab` (the previous tab is gone, so there is nothing to
    /// hibernate). A hibernated repository screen rehydrates so its panels
    /// aren't left blank until the next tab switch.
    fn wake_tab(&mut self, tab_id: TabId) -> Task<Message> {
        if let Some(screen) = self.screens.get_mut(&tab_id) {
            if screen.is_hibernated() {
                return screen.rehydrate_task();
            }
        } else if let Some(screen) = self.plugin_screens.get_mut(&tab_id) {
            screen.set_focused(true);
        }
        Task::none()
    }

    /// Put the active tab to sleep and leave the active workspace without an
    /// active tab (it is empty, or its tabs just moved away).
    fn deactivate_current(&mut self) {
        let prev = self.active_tab_id;
        if let Some(screen) = self.screens.get_mut(&prev) {
            screen.hibernate();
        }
        if let Some(screen) = self.plugin_screens.get_mut(&prev) {
            screen.set_focused(false);
        }
        self.active_tab_id = NO_TAB;
        self.active_tab_persisted_as_recent = false;
        self.bump_revision();
    }

    /// Next tab id for Ctrl+Tab, within the active workspace. `None` when it
    /// has fewer than two tabs.
    pub fn next_tab_id_circular(&self) -> Option<TabId> {
        let ids = self.visible_ids();
        if ids.len() < 2 {
            return None;
        }
        let current_idx = ids
            .iter()
            .position(|id| *id == self.active_tab_id)
            .unwrap_or(0);
        Some(ids[(current_idx + 1) % ids.len()])
    }

    /// Previous tab id for Ctrl+Shift+Tab, within the active workspace.
    /// `None` when it has fewer than two tabs.
    pub fn prev_tab_id_circular(&self) -> Option<TabId> {
        let ids = self.visible_ids();
        if ids.len() < 2 {
            return None;
        }
        let current_idx = ids
            .iter()
            .position(|id| *id == self.active_tab_id)
            .unwrap_or(0);
        Some(ids[(current_idx + ids.len() - 1) % ids.len()])
    }

    /// Select a tab directly (e.g. from tab-bar click). Skips work if it's
    /// already active or doesn't exist.
    pub fn select(&mut self, tab_id: TabId) -> Task<Message> {
        self.activate_tab(tab_id)
    }

    /// Apply a new tab ordering (from a drag-reorder commit). The listed
    /// tabs take the slots they already occupy in the global order, in the
    /// new sequence, so reordering one workspace's tabs never moves another
    /// workspace's. Ids not in `self.tabs` are ignored; unlisted tabs keep
    /// their place. Persists the result.
    pub fn reorder(&mut self, new_order: Vec<TabId>) {
        let mut seen = HashSet::new();
        let order: Vec<TabId> = new_order
            .into_iter()
            .filter(|id| self.tabs.iter().any(|t| t.id == *id) && seen.insert(*id))
            .collect();
        let mut by_id: HashMap<TabId, TabEntry> = HashMap::new();
        let mut slots: Vec<Option<TabEntry>> = Vec::with_capacity(self.tabs.len());
        for entry in self.tabs.drain(..) {
            if seen.contains(&entry.id) {
                by_id.insert(entry.id, entry);
                slots.push(None);
            } else {
                slots.push(Some(entry));
            }
        }
        let mut incoming = order.into_iter().filter_map(|id| by_id.remove(&id));
        self.tabs = slots
            .into_iter()
            .filter_map(|slot| slot.or_else(|| incoming.next()))
            .collect();
        self.bump_revision();
        self.persist_tab_order();
    }

    fn persist_tab_order(&mut self) {
        let paths: Vec<String> = self
            .tabs
            .iter()
            .filter_map(|t| t.repo_path().map(ToOwned::to_owned))
            .collect();
        if let Some(settings) = self.settings() {
            let _ = settings.set_repo_order(&paths);
        }
    }

    /// Activate `new_tab_id`: hibernate the previous tab, rehydrate or
    /// re-select on the new one so its panels light up immediately. Caller
    /// handles anything fetch-related.
    ///
    /// Activating a tab of another workspace switches to that workspace.
    pub fn activate_tab(&mut self, new_tab_id: TabId) -> Task<Message> {
        let prev_tab_id = self.active_tab_id;
        if prev_tab_id == new_tab_id {
            return Task::none();
        }
        let Some(workspace) = self.entry(new_tab_id).map(|t| t.workspace) else {
            return Task::none();
        };
        if workspace != self.active_workspace {
            self.remember_active_tab();
            self.set_active_workspace(workspace);
        }
        if let Some(prev) = self.screens.get_mut(&prev_tab_id) {
            prev.hibernate();
        }
        if let Some(prev) = self.plugin_screens.get_mut(&prev_tab_id) {
            prev.set_focused(false);
        }
        self.active_tab_id = new_tab_id;
        self.active_tab_persisted_as_recent = false;
        self.bump_revision();
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
            if let Some(settings) = self.settings() {
                let _ = settings.set_most_recent_repo(&repo_path);
            }
            self.remembered_paths
                .insert(self.active_workspace.raw(), repo_path);
            self.persist_remembered_paths();
            self.active_tab_persisted_as_recent = true;
        }
    }

    // ── Workspaces ──────────────────────────────────────────────────────

    /// Switch to `workspace`, landing on the tab it had active last time (or
    /// its first tab). An empty workspace leaves no tab active.
    pub fn select_workspace(&mut self, workspace: WorkspaceId) -> Task<Message> {
        if workspace == self.active_workspace || !self.workspaces.contains(workspace) {
            return Task::none();
        }
        match self.remembered_tab_for(workspace) {
            // `activate_tab` remembers the current tab and switches workspace.
            Some(tab_id) => self.activate_tab(tab_id),
            None => {
                self.remember_active_tab();
                self.deactivate_current();
                self.set_active_workspace(workspace);
                Task::none()
            }
        }
    }

    /// Create a workspace at the end of the sort order. It is not selected.
    pub fn create_workspace(&mut self, name: String, icon: String) -> WorkspaceId {
        let stored = self
            .settings()
            .and_then(|s| s.create_workspace(&name, &icon).ok())
            .map(WorkspaceId);
        let id = stored
            .filter(|id| !self.workspaces.contains(*id))
            .unwrap_or_else(|| self.workspaces.next_local_id());
        self.workspaces.insert(Workspace { id, name, icon });
        self.bump_revision();
        id
    }

    pub fn update_workspace(&mut self, id: WorkspaceId, name: String, icon: String) -> bool {
        if !self.workspaces.update(id, name.clone(), icon.clone()) {
            return false;
        }
        if let Some(settings) = self.settings() {
            let _ = settings.update_workspace(id.raw(), &name, &icon);
        }
        self.bump_revision();
        true
    }

    /// Delete a user-created workspace. Its tabs move to the end of the
    /// default workspace. Deleting the active workspace makes the default
    /// one active, keeping the current tab (which moved along) in front.
    pub fn delete_workspace(&mut self, id: WorkspaceId) -> Task<Message> {
        if id.is_default() || !self.workspaces.contains(id) {
            return Task::none();
        }
        let (moved, kept): (Vec<TabEntry>, Vec<TabEntry>) =
            self.tabs.drain(..).partition(|t| t.workspace == id);
        self.tabs = kept;
        let moved_any = !moved.is_empty();
        for mut entry in moved {
            entry.workspace = WorkspaceId::DEFAULT;
            self.tabs.push(entry);
        }
        self.workspaces.remove(id);
        self.workspace_active_tab.remove(&id);
        self.remembered_paths.remove(&id.raw());
        if let Some(settings) = self.settings() {
            let _ = settings.delete_workspace(id.raw());
        }
        self.persist_remembered_paths();
        if moved_any {
            self.persist_tab_order();
        }
        self.bump_revision();

        if self.active_workspace == id {
            self.set_active_workspace(WorkspaceId::DEFAULT);
        }
        // The moved tabs may have landed in an active workspace that had no
        // tab in front (the deleted one itself, or an empty default).
        if self.active_tab_id != NO_TAB {
            return Task::none();
        }
        match self.remembered_tab_for(self.active_workspace) {
            Some(tab_id) => {
                self.active_tab_id = tab_id;
                self.active_tab_persisted_as_recent = false;
                self.wake_tab(tab_id)
            }
            None => Task::none(),
        }
    }

    pub fn reorder_workspaces(&mut self, order: &[WorkspaceId]) {
        if !self.workspaces.reorder(order) {
            return;
        }
        let ids: Vec<u64> = self
            .workspaces
            .custom()
            .iter()
            .map(|w| w.id.raw())
            .collect();
        if let Some(settings) = self.settings() {
            let _ = settings.set_workspace_order(&ids);
        }
        self.bump_revision();
    }

    /// Move a tab to the end of `workspace`'s tabs. With `follow`, the
    /// target workspace becomes active with the tab in front. Without it,
    /// the active workspace stays put and, if the moved tab was active, its
    /// neighbour takes over.
    pub fn move_tab_to_workspace(
        &mut self,
        tab_id: TabId,
        workspace: WorkspaceId,
        follow: bool,
    ) -> Task<Message> {
        if !self.workspaces.contains(workspace) {
            return Task::none();
        }
        let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return Task::none();
        };
        if self.tabs[idx].workspace == workspace {
            return if follow {
                self.activate_tab(tab_id)
            } else {
                Task::none()
            };
        }
        let neighbor = self.neighbor_in_workspace(tab_id);
        let was_active = self.active_tab_id == tab_id;

        let mut entry = self.tabs.remove(idx);
        let from = entry.workspace;
        entry.workspace = workspace;
        let repo_path = entry.repo_path().map(ToOwned::to_owned);
        let key = entry.path_key().to_string();
        self.tabs.push(entry);

        if self.workspace_active_tab.get(&from) == Some(&tab_id) {
            self.workspace_active_tab.remove(&from);
        }
        if self.remembered_paths.get(&from.raw()) == Some(&key) {
            self.remembered_paths.remove(&from.raw());
        }
        self.workspace_active_tab.entry(workspace).or_insert(tab_id);
        if let Some(path) = repo_path {
            if let Some(settings) = self.settings() {
                let _ = settings.set_repo_workspace(&path, workspace.raw());
            }
            self.persist_tab_order();
        }
        self.bump_revision();

        if follow {
            if !was_active {
                return self.activate_tab(tab_id);
            }
            // The tab keeps the screen; only the workspace around it
            // changes. The old workspace resumes on the neighbour.
            if let Some(next) = neighbor {
                self.workspace_active_tab.insert(from, next);
                if let Some(next_key) = self.entry(next).map(|t| t.path_key().to_string()) {
                    self.remembered_paths.insert(from.raw(), next_key);
                }
            }
            self.workspace_active_tab.insert(workspace, tab_id);
            self.remembered_paths.insert(workspace.raw(), key);
            self.persist_remembered_paths();
            self.set_active_workspace(workspace);
            return Task::none();
        }
        if !was_active {
            return Task::none();
        }
        match neighbor {
            Some(next) => self.activate_tab(next),
            None => {
                self.deactivate_current();
                Task::none()
            }
        }
    }

    /// The tab `workspace` should land on when entered: the one it had active
    /// last (this session, else as persisted), else its first tab.
    fn remembered_tab_for(&self, workspace: WorkspaceId) -> Option<TabId> {
        let in_workspace = |id: TabId| self.entry(id).is_some_and(|t| t.workspace == workspace);
        self.workspace_active_tab
            .get(&workspace)
            .copied()
            .filter(|id| in_workspace(*id))
            .or_else(|| self.remembered_path_tab(workspace))
            .or_else(|| {
                self.tabs
                    .iter()
                    .find(|t| t.workspace == workspace)
                    .map(|t| t.id)
            })
    }

    /// Record the active tab as the one to return to in the active workspace.
    fn remember_active_tab(&mut self) {
        let Some(key) = self.active_entry().map(|t| t.path_key().to_string()) else {
            return;
        };
        self.workspace_active_tab
            .insert(self.active_workspace, self.active_tab_id);
        self.remembered_paths
            .insert(self.active_workspace.raw(), key);
        self.persist_remembered_paths();
    }

    fn set_active_workspace(&mut self, workspace: WorkspaceId) {
        if self.active_workspace == workspace {
            return;
        }
        self.active_workspace = workspace;
        self.bump_revision();
        if let Some(settings) = self.settings() {
            let _ = settings.set_active_workspace(workspace.raw());
        }
    }

    fn persist_remembered_paths(&mut self) {
        let paths = self.remembered_paths.clone();
        if let Some(settings) = self.settings() {
            let _ = settings.save_workspace_active_tabs(&paths);
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
        TabManager::new(Arc::new(DefaultPresenter::new())).without_persistence()
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
        push_tab_in(m, id, path, WorkspaceId::DEFAULT);
    }

    /// Screen-less tab entry: enough for ordering / workspace bookkeeping.
    fn push_tab_in(m: &mut TabManager, id: u64, path: &str, workspace: WorkspaceId) {
        m.tabs.push(TabEntry {
            id: TabId(id),
            name: path.to_string(),
            kind: TabKind::Repository {
                path: path.to_string(),
            },
            workspace,
        });
        if m.active_tab_id == NO_TAB && workspace == m.active_workspace {
            m.active_tab_id = TabId(id);
        }
    }

    fn visible(m: &TabManager) -> Vec<u64> {
        m.visible_tabs().map(|t| t.id.0).collect()
    }

    fn global(m: &TabManager) -> Vec<u64> {
        m.tabs.iter().map(|t| t.id.0).collect()
    }

    /// Default workspace with tabs 1, 2; workspace A with 3, 4 (interleaved
    /// in global order as 1, 3, 2, 4).
    fn two_workspaces() -> (TabManager, WorkspaceId) {
        let mut m = make();
        let a = m.create_workspace("A".into(), "rocket".into());
        push_tab_in(&mut m, 1, "/1", WorkspaceId::DEFAULT);
        push_tab_in(&mut m, 3, "/3", a);
        push_tab_in(&mut m, 2, "/2", WorkspaceId::DEFAULT);
        push_tab_in(&mut m, 4, "/4", a);
        (m, a)
    }

    #[test]
    fn only_active_workspace_tabs_are_visible() {
        let (mut m, a) = two_workspaces();
        assert_eq!(visible(&m), vec![1, 2]);
        assert_eq!(m.active_tab_id(), TabId(1));
        let _ = m.select_workspace(a);
        assert_eq!(m.active_workspace(), a);
        assert_eq!(visible(&m), vec![3, 4]);
        assert_eq!(m.active_tab_id(), TabId(3));
        assert_eq!(m.tab_count(a), 2);
        assert_eq!(m.tab_count(WorkspaceId::DEFAULT), 2);
    }

    #[test]
    fn switching_back_restores_each_workspaces_active_tab() {
        let (mut m, a) = two_workspaces();
        let _ = m.activate_tab(TabId(2));
        let _ = m.select_workspace(a);
        let _ = m.activate_tab(TabId(4));
        let _ = m.select_workspace(WorkspaceId::DEFAULT);
        assert_eq!(m.active_tab_id(), TabId(2));
        let _ = m.select_workspace(a);
        assert_eq!(m.active_tab_id(), TabId(4));
    }

    #[test]
    fn empty_workspace_has_no_active_tab() {
        let (mut m, _) = two_workspaces();
        let empty = m.create_workspace("Empty".into(), "bug".into());
        let _ = m.select_workspace(empty);
        assert!(m.is_empty());
        assert!(m.has_any_tabs());
        assert_eq!(m.active_tab_id(), NO_TAB);
        assert!(m.active_entry().is_none());
        assert_eq!(m.next_tab_id_circular(), None);
    }

    #[test]
    fn selecting_unknown_workspace_is_noop() {
        let (mut m, _) = two_workspaces();
        let _ = m.select_workspace(WorkspaceId(999));
        assert_eq!(m.active_workspace(), WorkspaceId::DEFAULT);
    }

    #[test]
    fn activating_hidden_tab_switches_workspace() {
        let (mut m, a) = two_workspaces();
        let _ = m.activate_tab(TabId(4));
        assert_eq!(m.active_workspace(), a);
        assert_eq!(m.active_tab_id(), TabId(4));
        // The default workspace remembers where it was left.
        let _ = m.select_workspace(WorkspaceId::DEFAULT);
        assert_eq!(m.active_tab_id(), TabId(1));
    }

    #[test]
    fn circular_nav_stays_inside_workspace() {
        let (mut m, a) = two_workspaces();
        assert_eq!(m.next_tab_id_circular(), Some(TabId(2)));
        assert_eq!(m.prev_tab_id_circular(), Some(TabId(2)));
        let _ = m.select_workspace(a);
        assert_eq!(m.next_tab_id_circular(), Some(TabId(4)));
    }

    #[test]
    fn reorder_within_workspace_keeps_other_workspace_slots() {
        let (mut m, _) = two_workspaces();
        m.reorder(vec![TabId(2), TabId(1)]);
        assert_eq!(global(&m), vec![2, 3, 1, 4]);
        assert_eq!(visible(&m), vec![2, 1]);
    }

    #[test]
    fn closing_active_tab_picks_neighbor_in_same_workspace() {
        let (mut m, a) = two_workspaces();
        let _ = m.select_workspace(a);
        // Tab 3 is first in A: its right neighbour 4 takes over even though
        // tab 2 (default workspace) sits next to it globally.
        let _ = m.remove_tab_inner(TabId(3), m.tabs.iter().position(|t| t.id == TabId(3)));
        assert_eq!(m.active_tab_id(), TabId(4));
        let _ = m.remove_tab_inner(TabId(4), m.tabs.iter().position(|t| t.id == TabId(4)));
        assert_eq!(m.active_tab_id(), NO_TAB);
        assert!(m.is_empty());
        assert_eq!(m.active_workspace(), a);
    }

    #[test]
    fn moving_active_tab_keeps_workspace_and_activates_neighbor() {
        let (mut m, a) = two_workspaces();
        let _ = m.activate_tab(TabId(2));
        let _ = m.move_tab_to_workspace(TabId(2), a, false);
        assert_eq!(m.active_workspace(), WorkspaceId::DEFAULT);
        assert_eq!(m.active_tab_id(), TabId(1));
        assert_eq!(visible(&m), vec![1]);
        let _ = m.select_workspace(a);
        // Moved tab lands at the end of the target workspace.
        assert_eq!(visible(&m), vec![3, 4, 2]);
    }

    #[test]
    fn moving_last_tab_out_empties_workspace() {
        let mut m = make();
        let a = m.create_workspace("A".into(), "rocket".into());
        push_tab_in(&mut m, 1, "/1", WorkspaceId::DEFAULT);
        let _ = m.move_tab_to_workspace(TabId(1), a, false);
        assert!(m.is_empty());
        assert_eq!(m.active_tab_id(), NO_TAB);
        let _ = m.select_workspace(a);
        assert_eq!(m.active_tab_id(), TabId(1));
    }

    #[test]
    fn moving_to_same_or_unknown_workspace_is_noop() {
        let (mut m, _) = two_workspaces();
        let before = m.revision();
        let _ = m.move_tab_to_workspace(TabId(1), WorkspaceId::DEFAULT, false);
        let _ = m.move_tab_to_workspace(TabId(1), WorkspaceId(999), false);
        assert_eq!(m.revision(), before);
    }

    #[test]
    fn moving_active_tab_with_follow_switches_workspace_keeping_tab() {
        let (mut m, a) = two_workspaces();
        let _ = m.activate_tab(TabId(2));
        let _ = m.move_tab_to_workspace(TabId(2), a, true);
        assert_eq!(m.active_workspace(), a);
        assert_eq!(m.active_tab_id(), TabId(2));
        assert_eq!(visible(&m), vec![3, 4, 2]);
        // The default workspace resumes on the moved tab's neighbour.
        let _ = m.select_workspace(WorkspaceId::DEFAULT);
        assert_eq!(m.active_tab_id(), TabId(1));
    }

    #[test]
    fn moving_inactive_tab_with_follow_activates_it_there() {
        let (mut m, a) = two_workspaces();
        let _ = m.move_tab_to_workspace(TabId(2), a, true);
        assert_eq!(m.active_workspace(), a);
        assert_eq!(m.active_tab_id(), TabId(2));
    }

    #[test]
    fn deleting_inactive_workspace_moves_tabs_to_default_end() {
        let (mut m, a) = two_workspaces();
        let _ = m.delete_workspace(a);
        assert!(!m.workspaces().contains(a));
        assert_eq!(visible(&m), vec![1, 2, 3, 4]);
        assert_eq!(m.active_tab_id(), TabId(1));
    }

    #[test]
    fn deleting_active_workspace_keeps_current_tab() {
        let (mut m, a) = two_workspaces();
        let _ = m.activate_tab(TabId(4));
        let _ = m.delete_workspace(a);
        assert_eq!(m.active_workspace(), WorkspaceId::DEFAULT);
        assert_eq!(m.active_tab_id(), TabId(4));
        assert_eq!(visible(&m), vec![1, 2, 3, 4]);
    }

    #[test]
    fn deleting_active_empty_workspace_lands_on_default_tab() {
        let (mut m, _) = two_workspaces();
        let empty = m.create_workspace("Empty".into(), "bug".into());
        let _ = m.activate_tab(TabId(2));
        let _ = m.select_workspace(empty);
        let _ = m.delete_workspace(empty);
        assert_eq!(m.active_workspace(), WorkspaceId::DEFAULT);
        assert_eq!(m.active_tab_id(), TabId(2));
    }

    #[test]
    fn deleting_workspace_into_empty_active_default_activates_a_tab() {
        let mut m = make();
        let a = m.create_workspace("A".into(), "rocket".into());
        push_tab_in(&mut m, 1, "/1", WorkspaceId::DEFAULT);
        let _ = m.move_tab_to_workspace(TabId(1), a, false);
        assert_eq!(m.active_tab_id(), NO_TAB);
        let _ = m.delete_workspace(a);
        assert_eq!(m.active_workspace(), WorkspaceId::DEFAULT);
        assert_eq!(m.active_tab_id(), TabId(1));
        assert!(!m.is_empty());
    }

    #[test]
    fn default_workspace_cannot_be_deleted() {
        let (mut m, _) = two_workspaces();
        let before = m.revision();
        let _ = m.delete_workspace(WorkspaceId::DEFAULT);
        assert_eq!(m.revision(), before);
    }

    #[test]
    fn reorder_workspaces_updates_order() {
        let mut m = make();
        let a = m.create_workspace("A".into(), "rocket".into());
        let b = m.create_workspace("B".into(), "bug".into());
        m.reorder_workspaces(&[b, a]);
        let order: Vec<WorkspaceId> = m.workspaces().custom().iter().map(|w| w.id).collect();
        assert_eq!(order, vec![b, a]);
    }

    #[test]
    fn update_workspace_renames() {
        let mut m = make();
        let a = m.create_workspace("A".into(), "rocket".into());
        assert!(m.update_workspace(a, "Alpha".into(), "code".into()));
        assert_eq!(m.workspaces().name(a), "Alpha");
        assert!(!m.update_workspace(WorkspaceId::DEFAULT, "X".into(), "code".into()));
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

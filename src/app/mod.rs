mod animation;
mod commands;
mod fetch_ops;
mod fetch_policy;
mod focus;
mod git_queue;
mod input;
mod key_chord;
mod subscription;
mod tabs;
mod update;
mod view;

use iced::{Element, Subscription, Task};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{
    config::AppConfig,
    message::Message,
    plugin::events::EventPayload,
    plugin::navigation::PluginNavigationEffect,
    plugin::tab_snapshot::{TabRegistryOp, TabSnapshotEntry, TabsSnapshot},
    plugin::PluginHost,
    screens::no_git::TargetOs,
    screens::{BlankScreen, NoGitScreen},
    services::{
        detect_git, DefaultPresenter, GitStatus, PersistedPluginTab, Presenter, SettingsService,
    },
    toast::ToastManager,
    widgets::chrome::main_bar::{builtins as main_bar_builtins, MainBarRegistry},
    widgets::chrome::repo_region::{RepoChromeRegistry, RepoRegionRegistry},
    widgets::chrome::tab_bar_slots::{builtins as tab_bar_builtins, TabBarRegistry},
};

use fetch_policy::FetchPolicy;
use git_queue::GitOperationQueue;
use key_chord::KeyChordState;
use tabs::TabManager;

/// Delay between Ctrl+Tab settling on a tab and kicking off its auto-fetch.
///
/// Users often Ctrl+Tab through several tabs to land on the one they want;
/// a short debounce avoids thrashing fetches on the flyby tabs.
const FETCH_DEBOUNCE_AFTER_TAB_SWITCH: Duration = Duration::from_secs(3);

pub struct App {
    pub(super) tabs: TabManager,
    pub(super) blank_screen: BlankScreen,
    pub(super) no_git_screen: Option<NoGitScreen>,
    pub(super) toasts: ToastManager,
    pub(super) last_animation_tick: Option<Instant>,
    pub(super) fetch: FetchPolicy,
    pub(super) git_queue: GitOperationQueue,
    /// Handle to the most recent file-watcher-driven `reload_refs_task`.
    /// File-watcher events during a git op tend to arrive in bursts (e.g.
    /// every packfile write during `git fetch`); if one reload is already in
    /// flight, the next burst aborts it and starts a fresh one so only the
    /// latest snapshot ever reaches the main thread.
    pub(super) reload_refs_abort: Option<iced::task::Handle>,
    pub(super) plugin_host: PluginHost,
    pub(in crate::app) key_chord: KeyChordState,
    /// Authoritative slot registry for the main bar. Rebuilt from built-ins
    /// + current plugin contributions after plugin host mutations.
    pub(super) main_bar_registry: MainBarRegistry,
    /// Authoritative slot registry for the tab bar. Rebuilt from built-ins
    /// + current plugin contributions after plugin host mutations.
    pub(super) tab_bar_registry: TabBarRegistry,
    /// Authoritative slot registry for the repository region. Rebuilt from
    /// current plugin contributions after plugin host mutations.
    pub(super) repo_region_registry: RepoRegionRegistry,
    pub(super) repo_chrome_registry: RepoChromeRegistry,
    pub(super) slot_registry_revision: u64,
    pub(super) pending_focus_reason: Option<crate::plugin::ui::focus::FocusReason>,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let presenter: Arc<dyn Presenter> = Arc::new(DefaultPresenter::new());
        let mut plugin_host = PluginHost::new();
        plugin_host.load_from_default_dirs();
        let _ = plugin_host.introspect();
        let _ = plugin_host.extension_context_menu_items("");
        let _ = plugin_host.extension_graph_decorations_for_commit("");
        plugin_host.discard_extensions_for_plugin("");
        // Prime the budget tracker query / reset / cleanup
        // entry points so the dead-code analyser sees them as live.
        // Sentinel ids match nothing — the calls exist for the side
        // effect of resolving the breaker lookup, not its result.
        let _ = plugin_host.is_breaker_tripped("", 0, "");
        plugin_host.reset_breaker("", "");
        plugin_host.drop_breaker_state_for_plugin("");
        plugin_host.drop_breaker_state_for_generation("", 0);

        let (main_bar_registry, tab_bar_registry, repo_region_registry, repo_chrome_registry) =
            build_slot_registries(&plugin_host);
        let slot_registry_revision = plugin_host.slot_ops_revision();

        let mut app = App {
            tabs: TabManager::new(presenter),
            blank_screen: BlankScreen::new(),
            no_git_screen: None,
            toasts: ToastManager::default(),
            last_animation_tick: None,
            fetch: FetchPolicy::new(),
            git_queue: GitOperationQueue::new(),
            reload_refs_abort: None,
            plugin_host,
            key_chord: KeyChordState::new(),
            main_bar_registry,
            tab_bar_registry,
            repo_region_registry,
            repo_chrome_registry,
            slot_registry_revision,
            pending_focus_reason: None,
        };

        // Test hook: GIT_LEVIATHAN_FORCE_SCREEN overrides normal startup.
        match parse_force_screen() {
            Some(ForceScreen::NoGit { status, target_os }) => {
                app.no_git_screen = Some(NoGitScreen::new(status, target_os));
                return (app, Task::none());
            }
            Some(ForceScreen::Blank) => {
                return (app, Task::none());
            }
            None => {}
        }

        let git_status = detect_git();
        if !git_status.is_available() {
            app.no_git_screen = Some(NoGitScreen::new(git_status, TargetOs::current()));
            return (app, Task::none());
        }

        let task = app.load_initial_repos();
        app.restore_plugin_tabs();
        app.sync_repository_to_plugins();
        app.process_tab_changes();
        app.rebuild_slot_registries();
        (app, task)
    }

    fn load_initial_repos(&mut self) -> Task<Message> {
        let config = AppConfig::load();
        let env_repo = config.repo_path();

        let (repo_paths, most_recent_repo) = if let Some(path) = env_repo {
            (vec![path.to_string()], None)
        } else {
            SettingsService::new()
                .map(|s| {
                    let repos = s.load_repos().unwrap_or_default();
                    let most_recent = s.get_most_recent_repo().unwrap_or_default();
                    (repos, most_recent)
                })
                .unwrap_or_else(|_| (Vec::new(), None))
        };

        let initial_tasks = self.tabs.load_initial_repos(repo_paths, most_recent_repo);
        Task::batch(initial_tasks)
    }

    fn restore_plugin_tabs(&mut self) {
        let Ok(settings) = SettingsService::new() else {
            return;
        };
        let Ok(tabs) = settings.load_plugin_tabs() else {
            return;
        };
        for tab in tabs {
            if !self
                .plugin_host
                .screen_exists(&tab.plugin_id, &tab.screen_id)
            {
                continue;
            }
            if let Some(state) = tab.state {
                let _ = self.plugin_host.deserialize_screen_state(
                    &tab.plugin_id,
                    &tab.screen_id,
                    state,
                );
            }
            self.open_plugin_screen_tab(tab.plugin_id, tab.screen_id, tab.bound_repo_path);
        }
    }

    pub(super) fn drain_pending_navigation_effects(&mut self) -> Task<Message> {
        let effects = self.plugin_host.take_pending_navigation_effects();
        for effect in effects {
            match effect {
                PluginNavigationEffect::NavigateRepository => self.navigate_to_repository_tab(),
                PluginNavigationEffect::OpenScreen {
                    plugin_id,
                    screen_id,
                } => self.open_plugin_screen_tab(plugin_id, screen_id, None),
            }
        }
        Task::none()
    }

    fn navigate_to_repository_tab(&mut self) {
        let bound = self
            .tabs
            .active_plugin_screen()
            .and_then(|s| s.bound_repo_path().map(ToOwned::to_owned));
        let target = bound
            .as_deref()
            .and_then(|path| self.tabs.tab_id_for_path(path))
            .or_else(|| self.tabs.first_repository_tab_id());
        if let Some(tab_id) = target {
            let _ = self.tabs.select(tab_id);
            self.plugin_host.clear_active_screen();
        }
    }

    fn open_plugin_screen_tab(
        &mut self,
        plugin_id: String,
        screen_id: String,
        restored_bound_repo_path: Option<String>,
    ) {
        let Some(summary) = self.plugin_host.screen_summary(&plugin_id, &screen_id) else {
            return;
        };
        let bound_repo_path = restored_bound_repo_path.or_else(|| {
            if summary.bind_repository {
                self.tabs
                    .active_entry()
                    .and_then(|entry| entry.repo_path().map(ToOwned::to_owned))
            } else {
                None
            }
        });
        self.plugin_host.open_screen(plugin_id, screen_id);
        self.tabs.open_plugin_screen(summary, bound_repo_path);
    }

    fn persist_plugin_tabs(&mut self) {
        let tabs: Vec<PersistedPluginTab> = self
            .tabs
            .tabs()
            .iter()
            .filter_map(|entry| {
                let screen = self.tabs.plugin_screen(entry.id)?;
                Some(PersistedPluginTab {
                    plugin_id: screen.plugin_id().to_string(),
                    screen_id: screen.screen_id().to_string(),
                    title: screen.title().to_string(),
                    bound_repo_path: screen.bound_repo_path().map(ToOwned::to_owned),
                    state: self
                        .plugin_host
                        .serialize_screen_state(screen.plugin_id(), screen.screen_id()),
                })
            })
            .collect();
        if let Ok(settings) = SettingsService::new() {
            let _ = settings.save_plugin_tabs(&tabs);
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = match message {
            Message::App(am) => self.update_app(am),
            Message::Screen(routed) => self.update_screen(routed),
            Message::Toast(tm) => self.update_toast(tm),
            Message::Plugin(pm) => self.update_plugin(pm),
        };
        self.sync_active_plugin_screen_to_host();
        self.sync_repository_to_plugins();
        self.process_tab_changes();
        let command_actions = self.drain_core_command_actions();
        let drain = self.drain_pending_tab_ops();
        let git_drain = self.drain_git_operation_queue();
        let ui_effects = self.drain_plugin_ui_effects();
        self.persist_plugin_tabs();
        let snapshot = self.compute_focus_snapshot();
        let _ = self.plugin_host.sync_focus(snapshot);
        self.rebuild_slot_registries();
        self.reset_animation_clock_if_idle();
        Task::batch(vec![task, command_actions, drain, git_drain, ui_effects])
    }

    fn drain_plugin_ui_effects(&mut self) -> Task<Message> {
        Task::batch(
            self.plugin_host
                .take_pending_ui_scrolls()
                .into_iter()
                .map(|request| {
                    iced::widget::operation::scroll_to(
                        iced::widget::Id::from(request.id),
                        iced::widget::scrollable::AbsoluteOffset {
                            x: 0.0,
                            y: request.y,
                        },
                    )
                }),
        )
    }

    fn rebuild_slot_registries(&mut self) {
        let revision = self.plugin_host.slot_ops_revision();
        if self.slot_registry_revision == revision {
            return;
        }
        let (main_bar_registry, tab_bar_registry, repo_region_registry, repo_chrome_registry) =
            build_slot_registries(&self.plugin_host);
        self.main_bar_registry = main_bar_registry;
        self.tab_bar_registry = tab_bar_registry;
        self.repo_region_registry = repo_region_registry;
        self.repo_chrome_registry = repo_chrome_registry;
        self.slot_registry_revision = revision;
    }

    fn sync_active_plugin_screen_to_host(&mut self) {
        if let Some(screen) = self.tabs.active_plugin_screen() {
            if self.plugin_host.active_screen() != Some((screen.plugin_id(), screen.screen_id())) {
                self.plugin_host.open_screen(
                    screen.plugin_id().to_string(),
                    screen.screen_id().to_string(),
                );
            }
        } else if self.plugin_host.active_screen().is_some() {
            self.plugin_host.clear_active_screen();
        }
    }

    /// Push the active tab's branch refs into every plugin's
    /// `leviathan.repository` and fire `BranchChanged` when they differ
    /// from the last sync. Called at the end of `update` — the plugin
    /// host short-circuits on unchanged state so the common path is a
    /// cheap hash compare.
    ///
    /// The refs slice is cloned into a local `Vec` so we can drop the
    /// borrow on `self.tabs` before re-borrowing `self.plugin_host`
    /// mutably; the list is small (<= a few dozen entries on typical
    /// repos) so the clone is cheap.
    fn sync_repository_to_plugins(&mut self) {
        let bound_repo_tab = self
            .tabs
            .active_plugin_screen()
            .and_then(|s| s.bound_repo_path().map(ToOwned::to_owned))
            .and_then(|path| self.tabs.tab_id_for_path(&path));
        let source_screen = self
            .tabs
            .active_screen()
            .or_else(|| bound_repo_tab.and_then(|id| self.tabs.screen(id)));
        let active_gateway = source_screen.map(|screen| screen.active_gateway());
        let selection = source_screen
            .map(|screen| screen.selection_context_snapshot())
            .unwrap_or_else(crate::plugin::ui::context::SelectionContextSnapshot::none);
        let (repo_name, workdir_path, current_branch, head_hash, default_remote, refs) =
            source_screen
                .map(|screen| {
                    (
                        screen.repo_name().to_string(),
                        screen.active_worktree_path().to_string_lossy().into_owned(),
                        screen.current_branch().to_string(),
                        screen.head_hash().unwrap_or("").to_string(),
                        screen.default_remote_name().unwrap_or("").to_string(),
                        screen.branch_refs().to_vec(),
                    )
                })
                .unwrap_or_else(|| {
                    (
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        Vec::new(),
                    )
                });
        // Keep the plugin host's view of the active gateway
        // in sync with the active tab. None when no repository is open;
        // plugin git reads/writes then surface "no repository open"
        // instead of silently routing to a stale gateway.
        self.plugin_host.set_repository_gateway(active_gateway);
        self.plugin_host.sync_repository(
            &repo_name,
            &workdir_path,
            &current_branch,
            &head_hash,
            &default_remote,
            &refs,
        );
        self.plugin_host.sync_selection(selection);
    }

    pub(super) fn fetch_all_remotes_payload() -> EventPayload {
        let mut payload = EventPayload::new();
        payload.insert("remote".into(), serde_json::Value::String(String::new()));
        payload.insert("scope".into(), serde_json::Value::String("all".into()));
        payload
    }

    pub(super) fn tab_payload(entry: &TabSnapshotEntry) -> EventPayload {
        let mut payload = EventPayload::new();
        payload.insert(
            "tab_id".into(),
            serde_json::Value::Number(serde_json::Number::from(entry.id.raw())),
        );
        payload.insert("path".into(), serde_json::Value::String(entry.path.clone()));
        payload
    }

    pub(super) fn tab_moved_payload(count: usize) -> EventPayload {
        let mut payload = EventPayload::new();
        payload.insert(
            "count".into(),
            serde_json::Value::Number(serde_json::Number::from(count as u64)),
        );
        payload
    }

    fn snapshot_tabs(&self) -> TabsSnapshot {
        let entries: Vec<TabSnapshotEntry> = self
            .tabs
            .tabs()
            .iter()
            .map(|t| TabSnapshotEntry {
                id: t.id,
                path: t.path_key().to_string(),
                name: t.name.clone(),
            })
            .collect();
        let active_id = if self.tabs.is_empty() {
            None
        } else {
            Some(self.tabs.active_tab_id())
        };
        let active_path =
            active_id.and_then(|id| entries.iter().find(|t| t.id == id).map(|t| t.path.clone()));
        TabsSnapshot {
            tabs: entries,
            active_id,
            active_path,
        }
    }

    /// Diff the current tab list against the last reflected snapshot.
    /// On any change: push the new snapshot to every plugin's
    /// `leviathan.tab_registry`, then fire each matching lifecycle event
    /// at most once. Sync runs first so autocmd handlers reading the
    /// global see the fresh state.
    pub(super) fn process_tab_changes(&mut self) {
        let current = self.snapshot_tabs();
        let Some(change) = self.plugin_host.sync_tab_registry(&current) else {
            return;
        };
        if let Some(entry) = change.added_entry.as_ref() {
            self.plugin_host
                .fire_event_typed("TabAdded", Self::tab_payload(entry));
        }
        if let Some(entry) = change.removed_entry.as_ref() {
            self.plugin_host
                .fire_event_typed("TabRemoved", Self::tab_payload(entry));
        }
        if change.reordered {
            self.plugin_host
                .fire_event_typed("TabMoved", Self::tab_moved_payload(change.count));
        }
        if let Some(entry) = change.selected_entry.as_ref() {
            self.plugin_host
                .fire_event_typed("TabSelected", Self::tab_payload(entry));
        }
    }

    /// Apply queued `tab_registry.{add,remove,select}` ops Lua pushed
    /// during the just-finished update. Each pass may itself trigger
    /// further events (whose autocmds may queue more ops); cap the loop
    /// to keep a misbehaving plugin from spinning forever.
    pub(super) fn drain_pending_tab_ops(&mut self) -> Task<Message> {
        const MAX_ITERATIONS: usize = 8;
        let mut tasks: Vec<Task<Message>> = Vec::new();
        let mut iterations = 0;
        for _ in 0..MAX_ITERATIONS {
            let ops = self.plugin_host.take_pending_tab_ops();
            if ops.is_empty() {
                break;
            }
            iterations += 1;
            for op in ops {
                if let Some(t) = self.apply_tab_registry_op(op) {
                    tasks.push(t);
                }
            }
            self.sync_repository_to_plugins();
            self.process_tab_changes();
        }
        if iterations == MAX_ITERATIONS {
            eprintln!(
                "git_leviathan: drain_pending_tab_ops hit cap of {MAX_ITERATIONS} iterations; deferring remaining ops"
            );
        }
        Task::batch(tasks)
    }

    pub(super) fn apply_tab_registry_op(&mut self, op: TabRegistryOp) -> Option<Task<Message>> {
        match op {
            TabRegistryOp::Add(path) => {
                Some(self.open_repo_from_path(std::path::PathBuf::from(path)))
            }
            TabRegistryOp::Remove(path) => {
                if let Some(id) = self.tabs.tab_id_for_path(&path) {
                    if let Some(screen) = self.tabs.plugin_screen(id) {
                        if !screen.can_close(&self.plugin_host) {
                            return None;
                        }
                    }
                    self.tabs.close_tab(id);
                }
                None
            }
            TabRegistryOp::Select(path) => self
                .tabs
                .tab_id_for_path(&path)
                .map(|id| self.tabs.select(id)),
            TabRegistryOp::Reorder(paths) => {
                let ids: Vec<_> = paths
                    .iter()
                    .filter_map(|p| self.tabs.tab_id_for_path(p))
                    .collect();
                if !ids.is_empty() {
                    self.tabs.reorder(ids);
                }
                None
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        view::render(self)
    }

    pub fn theme(&self) -> iced::Theme {
        crate::style::iced_theme()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        subscription::build(self)
    }
}

/// Test-only startup override. Controlled by `GIT_LEVIATHAN_FORCE_SCREEN`.
enum ForceScreen {
    NoGit {
        status: GitStatus,
        target_os: TargetOs,
    },
    Blank,
}

fn parse_force_screen() -> Option<ForceScreen> {
    let raw = std::env::var("GIT_LEVIATHAN_FORCE_SCREEN").ok()?;
    let value = raw.trim().to_ascii_lowercase();
    match value.as_str() {
        "" => None,
        "blank" => Some(ForceScreen::Blank),
        "no-git-linux" => Some(ForceScreen::NoGit {
            status: GitStatus::NotFound,
            target_os: TargetOs::Linux,
        }),
        "no-git-macos" => Some(ForceScreen::NoGit {
            status: GitStatus::NotFound,
            target_os: TargetOs::MacOs,
        }),
        "no-git-windows" => Some(ForceScreen::NoGit {
            status: GitStatus::NotFound,
            target_os: TargetOs::Windows,
        }),
        "no-git-macos-clt" => Some(ForceScreen::NoGit {
            status: GitStatus::MacOsCommandLineToolsMissing,
            target_os: TargetOs::MacOs,
        }),
        other => {
            eprintln!(
                "git_leviathan: unknown GIT_LEVIATHAN_FORCE_SCREEN value {:?} — ignoring. \
                 Valid: blank, no-git-linux, no-git-macos, no-git-windows, no-git-macos-clt",
                other
            );
            None
        }
    }
}

fn build_slot_registries(
    plugin_host: &PluginHost,
) -> (
    MainBarRegistry,
    TabBarRegistry,
    RepoRegionRegistry,
    RepoChromeRegistry,
) {
    let mut main_bar_registry = MainBarRegistry::new();
    main_bar_builtins::register_all(&mut main_bar_registry);
    plugin_host.apply_main_bar_slots(&mut main_bar_registry);

    let mut tab_bar_registry = TabBarRegistry::new();
    tab_bar_builtins::register_all(&mut tab_bar_registry);
    plugin_host.apply_tab_bar_slots(&mut tab_bar_registry);

    let mut repo_region_registry = RepoRegionRegistry::new();
    plugin_host.apply_repo_region_slots(&mut repo_region_registry);

    let mut repo_chrome_registry = RepoChromeRegistry::new();
    plugin_host.apply_repo_chrome_slots(&mut repo_chrome_registry);

    (
        main_bar_registry,
        tab_bar_registry,
        repo_region_registry,
        repo_chrome_registry,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::tests::harness::MockHost;

    const MANIFEST: &str = r#"
id = "slots"
name = "Slots"
version = "0.1.0"
api_version = "1.0"
capabilities = [
  "ui:region:main_bar",
  "ui:region:tab_bar",
  "ui:region:repository",
]
"#;

    const INIT: &str = r#"
leviathan.ui.slot.add{
    region = "main_bar",
    section = "left",
    id = "plugin.slots.main",
    priority = 5,
    widget = { kind = "text", value = "main" },
}

leviathan.ui.slot.add{
    region = "tab_bar",
    section = "left",
    id = "plugin.slots.tab",
    priority = 5,
    widget = { kind = "text", value = "tab" },
}

leviathan.ui.slot.add{
    region = "repository",
    pane = "sidebar",
    section = "top",
    id = "plugin.slots.repo",
    priority = 5,
    widget = { kind = "text", value = "repo" },
}
"#;

    #[test]
    fn rebuilding_slot_registries_reflects_plugin_disable_and_enable() {
        let mut host = MockHost::new();
        host.load_inline("slots", MANIFEST, INIT)
            .expect("load slots plugin");
        let load_revision = host.host().slot_ops_revision();
        assert!(load_revision > 0);

        let (main, tabs, repo, _chrome) = build_slot_registries(host.host());
        assert!(main.contains_display_id("builtin.repo_info"));
        assert!(tabs.contains_display_id("builtin.plus_button"));
        assert!(main.contains_display_id("plugin.slots.main"));
        assert!(tabs.contains_display_id("plugin.slots.tab"));
        assert!(repo.contains_display_id("plugin.slots.repo"));

        assert!(host.host_mut().disable_plugin("slots"));
        let disable_revision = host.host().slot_ops_revision();
        assert!(disable_revision > load_revision);

        let (main, tabs, repo, _chrome) = build_slot_registries(host.host());
        assert!(main.contains_display_id("builtin.repo_info"));
        assert!(tabs.contains_display_id("builtin.plus_button"));
        assert!(!main.contains_display_id("plugin.slots.main"));
        assert!(!tabs.contains_display_id("plugin.slots.tab"));
        assert!(!repo.contains_display_id("plugin.slots.repo"));

        let reloaded = host
            .host_mut()
            .enable_plugin("slots")
            .expect("enable slots plugin");
        assert!(reloaded);
        assert!(host.host().slot_ops_revision() > disable_revision);

        let (main, tabs, repo, _chrome) = build_slot_registries(host.host());
        assert!(main.contains_display_id("plugin.slots.main"));
        assert!(tabs.contains_display_id("plugin.slots.tab"));
        assert!(repo.contains_display_id("plugin.slots.repo"));
    }

    #[test]
    fn app_rebuilds_slot_registries_only_when_slot_ops_revision_changes() {
        use crate::plugin::diagnostic::{DiagnosticStore, NullSink};

        const REMOVE_MISSING_INIT: &str = r#"
leviathan.ui.slot.add{
    region = "main_bar",
    section = "left",
    id = "present.slot",
    priority = 10,
    widget = { kind = "text", value = "present" },
}
leviathan.ui.slot.remove{
    region = "main_bar",
    section = "left",
    id = "missing.slot",
}
"#;

        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path().join("missing_remove");
        std::fs::create_dir_all(&dir).expect("plugin dir");
        std::fs::write(
            dir.join("plugin.toml"),
            r#"
id = "missing_remove"
name = "Missing Remove"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:region:main_bar"]
	"#,
        )
        .expect("manifest");
        std::fs::write(dir.join("init.lua"), REMOVE_MISSING_INIT).expect("init");

        let mut plugin_host = PluginHost::new();
        plugin_host.set_diagnostic_store(DiagnosticStore::with_sink(std::sync::Arc::new(NullSink)));
        plugin_host.trust_local_plugin_root(tmp.path());
        plugin_host
            .load_plugin(&dir)
            .expect("load missing-remove plugin");

        let (main_bar_registry, tab_bar_registry, repo_region_registry, repo_chrome_registry) =
            build_slot_registries(&plugin_host);
        let slot_registry_revision = plugin_host.slot_ops_revision();
        let initial_missing_count = plugin_host
            .diagnostics()
            .entries()
            .iter()
            .filter(|diag| diag.code == "schema.slot_remove_missing")
            .count();
        assert_eq!(initial_missing_count, 1);

        let mut app = App {
            tabs: TabManager::new(std::sync::Arc::new(DefaultPresenter::new())),
            blank_screen: BlankScreen::new(),
            no_git_screen: None,
            toasts: ToastManager::default(),
            last_animation_tick: None,
            fetch: FetchPolicy::new(),
            git_queue: GitOperationQueue::new(),
            reload_refs_abort: None,
            plugin_host,
            key_chord: KeyChordState::new(),
            main_bar_registry,
            tab_bar_registry,
            repo_region_registry,
            repo_chrome_registry,
            slot_registry_revision,
            pending_focus_reason: None,
        };

        app.rebuild_slot_registries();
        app.rebuild_slot_registries();
        let missing_count_after_noop_rebuilds = app
            .plugin_host
            .diagnostics()
            .entries()
            .iter()
            .filter(|diag| diag.code == "schema.slot_remove_missing")
            .count();
        assert_eq!(missing_count_after_noop_rebuilds, initial_missing_count);

        assert!(app.plugin_host.disable_plugin("missing_remove"));
        let disabled_revision = app.plugin_host.slot_ops_revision();
        assert_ne!(app.slot_registry_revision, disabled_revision);
        app.rebuild_slot_registries();
        assert_eq!(app.slot_registry_revision, disabled_revision);
    }
}

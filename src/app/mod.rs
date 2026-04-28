mod animation;
mod commands;
mod fetch_ops;
mod fetch_policy;
mod input;
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
    plugin::PluginHost,
    screens::no_git::TargetOs,
    screens::{BlankScreen, NoGitScreen},
    services::{detect_git, DefaultPresenter, GitStatus, Presenter, SettingsService},
    toast::ToastManager,
    widgets::chrome::main_bar::{builtins as main_bar_builtins, MainBarRegistry},
};

use fetch_policy::FetchPolicy;
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
    /// Handle to the most recent file-watcher-driven `reload_refs_task`.
    /// File-watcher events during a git op tend to arrive in bursts (e.g.
    /// every packfile write during `git fetch`); if one reload is already in
    /// flight, the next burst aborts it and starts a fresh one so only the
    /// latest snapshot ever reaches the main thread.
    pub(super) reload_refs_abort: Option<iced::task::Handle>,
    pub(super) plugin_host: PluginHost,
    /// Authoritative slot registry for the main bar. Built once at startup
    /// from built-ins + plugin contributions; `view` walks it each frame.
    pub(super) main_bar_registry: MainBarRegistry,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let presenter: Arc<dyn Presenter> = Arc::new(DefaultPresenter::new());
        let mut plugin_host = PluginHost::new();
        plugin_host.load_from_default_dirs();

        let mut main_bar_registry = MainBarRegistry::new();
        main_bar_builtins::register_all(&mut main_bar_registry);
        // Plugin contributions run after built-ins so plugins can remove
        // or replace built-in slots. Legacy `add_main_bar_button` buttons
        // land on the right; new `main_bar.{add,remove,replace}` ops run
        // in source order across all plugins.
        plugin_host.register_main_bar_slots(&mut main_bar_registry);

        let mut app = App {
            tabs: TabManager::new(presenter),
            blank_screen: BlankScreen::new(),
            no_git_screen: None,
            toasts: ToastManager::default(),
            last_animation_tick: None,
            fetch: FetchPolicy::new(),
            reload_refs_abort: None,
            plugin_host,
            main_bar_registry,
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
        app.sync_repository_to_plugins();
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

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = match message {
            Message::App(am) => self.update_app(am),
            Message::Screen(routed) => self.update_screen(routed),
            Message::Toast(tm) => self.update_toast(tm),
            Message::Plugin(pm) => self.update_plugin(pm),
        };
        self.sync_repository_to_plugins();
        self.reset_animation_clock_if_idle();
        task
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
        let (repo_name, workdir_path, current_branch, head_hash, default_remote, refs) = self
            .tabs
            .active_screen()
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
        self.plugin_host.sync_repository(
            &repo_name,
            &workdir_path,
            &current_branch,
            &head_hash,
            &default_remote,
            &refs,
        );
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

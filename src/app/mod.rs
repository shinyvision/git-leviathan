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
    screens::no_git::TargetOs,
    screens::{BlankScreen, NoGitScreen},
    services::{detect_git, DefaultPresenter, GitStatus, Presenter, SettingsService},
    toast::ToastManager,
};

use fetch_policy::FetchPolicy;
use tabs::TabManager;

/// Delay between Ctrl+Tab settling on a tab and kicking off its auto-fetch.
///
/// Users often Ctrl+Tab through several tabs to land on the one they want;
/// a short debounce avoids thrashing fetches on the flyby tabs.
const FETCH_DEBOUNCE_AFTER_TAB_SWITCH: Duration = Duration::from_secs(3);

pub struct App {
    tabs: TabManager,
    blank_screen: BlankScreen,
    no_git_screen: Option<NoGitScreen>,
    toasts: ToastManager,
    last_animation_tick: Option<Instant>,
    fetch: FetchPolicy,
    /// Handle to the most recent file-watcher-driven `reload_refs_task`.
    /// File-watcher events during a git op tend to arrive in bursts (e.g.
    /// every packfile write during `git fetch`); if one reload is already in
    /// flight, the next burst aborts it and starts a fresh one so only the
    /// latest snapshot ever reaches the main thread.
    reload_refs_abort: Option<iced::task::Handle>,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let presenter: Arc<dyn Presenter> = Arc::new(DefaultPresenter::new());
        let mut app = App {
            tabs: TabManager::new(presenter),
            blank_screen: BlankScreen::new(),
            no_git_screen: None,
            toasts: ToastManager::default(),
            last_animation_tick: None,
            fetch: FetchPolicy::new(),
            reload_refs_abort: None,
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
        };
        self.reset_animation_clock_if_idle();
        task
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

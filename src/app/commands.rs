//! App-level command handlers: opening repos, showing toasts, spawning the
//! OS file picker, opening URLs, and finalising a tab-load result.
//! Extracted from `update.rs` in keymaps.

use std::path::PathBuf;

use iced::Task;

use crate::core::TabId;
use crate::message::{AppMessage, Message, SyntaxGrammarCommandOutcome};
use crate::plugin::core_commands::CoreCommandAction;
use crate::plugin::tab_snapshot::TabRegistryOp;
use crate::screens::repository::panel_messages::DiffPanelAction;
use crate::screens::repository::RepositoryMessage;
use crate::services::GitError;
use crate::toast::{auto_dismiss_task, ToastData};
use crate::view_model::LoadedRepo;
use crate::work::presentation_work;

use super::{tabs, App};

impl App {
    pub(super) fn handle_tab_opened(
        &mut self,
        tab_id: TabId,
        result: Result<Box<LoadedRepo>, GitError>,
    ) -> Task<Message> {
        match result {
            Ok(snapshot) => {
                let active_id = self.tabs.active_tab_id();
                if let Some(screen) = self.tabs.screen_mut(tab_id) {
                    let task = screen.update(RepositoryMessage::RepoLoaded(Ok(*snapshot)));
                    let fetch_task = self.try_start_fetch_for_tab(tab_id);
                    if tab_id == active_id {
                        return Task::batch(vec![task, fetch_task]);
                    }
                    return fetch_task;
                }
                Task::none()
            }
            Err(e) => {
                eprintln!(
                    "git_leviathan: failed to open repo in tab {}: {}",
                    tab_id, e
                );
                self.tabs.drop_failed_tab(tab_id);
                self.show_toast(ToastData::error("Failed to Open Repository", e.to_string()))
            }
        }
    }

    pub(super) fn open_repo_dialog(&self) -> Task<Message> {
        Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .set_title("Open Git Repository")
                    .pick_folder()
                    .await
                    .map(|handle| handle.path().to_path_buf())
            },
            |path| Message::App(AppMessage::RepoPathChosen(path)),
        )
    }

    pub(super) fn open_repo_from_path(&mut self, path: PathBuf) -> Task<Message> {
        match self.tabs.open_path(path) {
            Ok(task) => task,
            Err(tabs::OpenRepoError::NotARepo(path)) => {
                eprintln!(
                    "git_leviathan: open_repo_from_path: {} is not a git repository",
                    path.display()
                );
                self.show_toast(ToastData::error(
                    "Not a Git Repository",
                    format!("{} is not a git repository", path.display()),
                ))
            }
        }
    }

    pub(super) fn show_toast(&mut self, toast: ToastData) -> Task<Message> {
        let toast_id = self.toasts.push(toast);
        self.reset_animation_clock_if_idle();
        auto_dismiss_task(toast_id)
    }

    pub(super) fn drain_core_command_actions(&mut self) -> Task<Message> {
        let actions = self.plugin_host.take_core_command_actions();
        Task::batch(
            actions
                .into_iter()
                .map(|action| self.apply_core_command_action(action))
                .collect::<Vec<_>>(),
        )
    }

    fn apply_core_command_action(&mut self, action: CoreCommandAction) -> Task<Message> {
        match action {
            CoreCommandAction::App(message) => self.update_app(message),
            CoreCommandAction::Repository(message) => Task::done(Message::repo(*message)),
            CoreCommandAction::OpenRepositoryPath(path) => {
                self.open_repo_from_path(std::path::PathBuf::from(path))
            }
            CoreCommandAction::CloseTab { path } => {
                let path = path.or_else(|| self.active_tab_path());
                if let Some(path) = path {
                    let _ = self.apply_tab_registry_op(TabRegistryOp::Remove(path));
                }
                Task::none()
            }
            CoreCommandAction::SelectTab { path } => {
                let task = self
                    .apply_tab_registry_op(TabRegistryOp::Select(path))
                    .unwrap_or_else(Task::none);
                Task::batch(vec![task, self.try_start_fetch()])
            }
            CoreCommandAction::ReorderTabs { paths } => {
                let _ = self.apply_tab_registry_op(TabRegistryOp::Reorder(paths));
                Task::none()
            }
            CoreCommandAction::RefreshGrammarRegistry { path } => {
                self.refresh_grammar_registry(path)
            }
            CoreCommandAction::InstallGrammar { language } => self.install_grammar(language),
            CoreCommandAction::UpdateGrammar { language } => self.update_grammar(language),
            CoreCommandAction::UninstallGrammar { language } => self.uninstall_grammar(language),
        }
    }

    fn active_tab_path(&self) -> Option<String> {
        let active = self.tabs.active_tab_id();
        self.tabs
            .tabs()
            .iter()
            .find(|tab| tab.id == active)
            .map(|tab| tab.path_key().to_string())
    }

    fn refresh_grammar_registry(&mut self, path: String) -> Task<Message> {
        syntax_grammar_command_task(
            "Grammar Registry Refreshed",
            "Grammar Registry Failed",
            false,
            move || {
                crate::services::refresh_runtime_grammar_registry_from_path(&path)
                    .map(|outcome| {
                        format!("Loaded {} grammar entries", outcome.registry.grammars.len())
                    })
                    .map_err(|err| err.message)
            },
        )
    }

    fn install_grammar(&mut self, language: String) -> Task<Message> {
        syntax_grammar_command_task(
            "Grammar Installed",
            "Grammar Install Failed",
            false,
            move || {
                crate::services::install_runtime_grammar(&language)
                    .map(|status| grammar_status_body(&status.language, status.status))
                    .map_err(|err| err.message)
            },
        )
    }

    fn update_grammar(&mut self, language: String) -> Task<Message> {
        syntax_grammar_command_task(
            "Grammar Updated",
            "Grammar Update Failed",
            false,
            move || {
                crate::services::update_runtime_grammar(&language)
                    .map(|status| grammar_status_body(&status.language, status.status))
                    .map_err(|err| err.message)
            },
        )
    }

    fn uninstall_grammar(&mut self, language: String) -> Task<Message> {
        syntax_grammar_command_task(
            "Grammar Removed",
            "Grammar Remove Failed",
            false,
            move || {
                crate::services::uninstall_runtime_grammar(&language)
                    .map(|status| grammar_status_body(&status.language, status.status))
                    .map_err(|err| err.message)
            },
        )
    }

    pub(super) fn eager_install_grammar(&mut self, language: String) -> Task<Message> {
        syntax_grammar_command_task(
            "Grammar Installed",
            "Grammar Install Failed",
            true,
            move || {
                crate::services::install_runtime_grammar(&language)
                    .map(|status| grammar_status_body(&status.language, status.status))
                    .map_err(|err| err.message)
            },
        )
    }

    pub(super) fn handle_syntax_grammar_command_finished(
        &mut self,
        outcome: SyntaxGrammarCommandOutcome,
    ) -> Task<Message> {
        let succeeded = outcome.result.is_ok();
        let changed_assets = outcome.changed_assets && succeeded;
        let suppress_toast = succeeded && outcome.silent_on_success;
        let toast_task = if suppress_toast {
            Task::none()
        } else {
            let toast = match outcome.result {
                Ok(body) => ToastData::success(outcome.success_title, body),
                Err(body) => ToastData::error(outcome.error_title, body),
            };
            self.show_toast(toast)
        };
        let diff_task = if changed_assets {
            self.tabs
                .active_screen_mut()
                .map(|screen| {
                    screen.update(RepositoryMessage::DiffPanel(
                        DiffPanelAction::SyntaxGrammarAssetsChanged,
                    ))
                })
                .unwrap_or_else(Task::none)
        } else {
            Task::none()
        };
        Task::batch(vec![toast_task, diff_task])
    }
}

fn syntax_grammar_command_task(
    success_title: &'static str,
    error_title: &'static str,
    silent_on_success: bool,
    run: impl FnOnce() -> Result<String, String> + Send + 'static,
) -> Task<Message> {
    Task::perform(presentation_work(run), move |result| {
        Message::App(AppMessage::SyntaxGrammarCommandFinished(
            SyntaxGrammarCommandOutcome {
                success_title: success_title.to_string(),
                error_title: error_title.to_string(),
                result,
                changed_assets: true,
                silent_on_success,
            },
        ))
    })
}

fn grammar_status_body(
    language: &str,
    status: crate::services::syntax_highlight::installation::GrammarInstallStatus,
) -> String {
    let status = match status {
        crate::services::syntax_highlight::installation::GrammarInstallStatus::Missing => "missing",
        crate::services::syntax_highlight::installation::GrammarInstallStatus::Queued => "queued",
        crate::services::syntax_highlight::installation::GrammarInstallStatus::Installing => {
            "installing"
        }
        crate::services::syntax_highlight::installation::GrammarInstallStatus::Installed => {
            "installed"
        }
        crate::services::syntax_highlight::installation::GrammarInstallStatus::Failed => "failed",
    };
    format!("{language} is {status}")
}

pub(super) fn open_url(url: &str) {
    use std::process::{Command, Stdio};

    #[cfg(target_os = "windows")]
    let result = {
        let mut command = Command::new("cmd");
        crate::utils::configure_background_command(&mut command)
            .args(["/C", "start", "", url])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    };

    #[cfg(target_os = "macos")]
    let result = Command::new("/usr/bin/open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    if let Err(e) = result {
        eprintln!("git_leviathan: failed to open URL {url}: {e}");
    }
}

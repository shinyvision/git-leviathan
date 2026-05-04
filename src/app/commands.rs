//! App-level command handlers: opening repos, showing toasts, spawning the
//! OS file picker, opening URLs, and finalising a tab-load result.
//! Extracted from `update.rs` in keymaps.

use std::path::PathBuf;

use iced::Task;

use crate::core::TabId;
use crate::message::{AppMessage, Message};
use crate::plugin::core_commands::CoreCommandAction;
use crate::plugin::tab_snapshot::TabRegistryOp;
use crate::screens::repository::panel_messages::{CenterAction, DetailAction};
use crate::screens::repository::RepositoryMessage;
use crate::services::GitError;
use crate::toast::{auto_dismiss_task, ToastData};
use crate::view_model::LoadedRepo;

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
            CoreCommandAction::Repository(message) => Task::done(Message::repo(message)),
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
            CoreCommandAction::Refresh => self
                .tabs
                .active_screen()
                .map(|screen| screen.reload_refs_task())
                .unwrap_or_else(Task::none),
            CoreCommandAction::Fetch => self.try_start_fetch(),
            CoreCommandAction::CreateBranchAtSelected { commit_idx, hash } => {
                self.create_branch_at_selected(commit_idx, hash)
            }
            CoreCommandAction::CopyCommitHash { hash } => self.copy_commit_hash(hash),
            CoreCommandAction::OpenSelectedDiff => self.open_selected_diff(),
            CoreCommandAction::StartRewordSelected => self.start_reword_selected(),
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

    fn create_branch_at_selected(
        &self,
        commit_idx: Option<usize>,
        hash: Option<String>,
    ) -> Task<Message> {
        let Some(screen) = self.tabs.active_screen() else {
            return Task::none();
        };
        let Some((idx, commit_hash)) = commit_idx
            .zip(hash)
            .or_else(|| screen.selected_commit_hash().map(|(idx, hash)| (idx, hash)))
        else {
            return Task::none();
        };
        Task::done(Message::repo(RepositoryMessage::Center(
            CenterAction::CreateBranchHereRequested {
                commit_idx: idx,
                commit_hash,
            },
        )))
    }

    fn copy_commit_hash(&self, hash: Option<String>) -> Task<Message> {
        let hash = hash.or_else(|| {
            self.tabs
                .active_screen()
                .and_then(|screen| screen.selected_commit_hash().map(|(_, hash)| hash))
        });
        match hash {
            Some(hash) => Task::done(Message::repo(RepositoryMessage::Detail(
                DetailAction::CopyCommitShaRequested(hash),
            ))),
            None => Task::none(),
        }
    }

    fn open_selected_diff(&self) -> Task<Message> {
        let Some(screen) = self.tabs.active_screen() else {
            return Task::none();
        };
        match screen.selected_diff_target() {
            Some(crate::screens::repository::SelectedDiffTarget::Dirty { path, is_staged }) => {
                Task::done(Message::repo(RepositoryMessage::Detail(
                    DetailAction::DirtyFileClicked { path, is_staged },
                )))
            }
            Some(crate::screens::repository::SelectedDiffTarget::Commit { commit_idx, path }) => {
                Task::done(Message::repo(RepositoryMessage::Detail(
                    DetailAction::CommitFileClicked { commit_idx, path },
                )))
            }
            None => Task::none(),
        }
    }

    fn start_reword_selected(&self) -> Task<Message> {
        let Some(screen) = self.tabs.active_screen() else {
            return Task::none();
        };
        let Some((hash, message)) = screen.selected_commit_reword_seed() else {
            return Task::none();
        };
        Task::done(Message::repo(RepositoryMessage::Detail(
            DetailAction::RewordStarted {
                hash,
                original_message: message,
            },
        )))
    }
}

pub(super) fn open_url(url: &str) {
    use std::process::{Command, Stdio};

    #[cfg(target_os = "windows")]
    let result = {
        use std::os::windows::process::CommandExt;
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
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

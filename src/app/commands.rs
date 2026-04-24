//! App-level command handlers: opening repos, showing toasts, spawning the
//! OS file picker, opening URLs, and finalising a tab-load result.
//! Extracted from `update.rs` in Phase 9.

use std::path::PathBuf;

use iced::Task;

use crate::core::TabId;
use crate::message::{AppMessage, Message};
use crate::screens::repository::RepositoryMessage;
use crate::services::GitError;
use crate::toast::{auto_dismiss_task, ToastData};
use crate::view_model::LoadedRepo;

use super::{tabs, App};

impl App {
    pub(super) fn handle_tab_opened(
        &mut self,
        tab_id: TabId,
        result: Result<LoadedRepo, GitError>,
    ) -> Task<Message> {
        match result {
            Ok(snapshot) => {
                let active_id = self.tabs.active_tab_id();
                if let Some(screen) = self.tabs.screen_mut(tab_id) {
                    let task = screen.update(RepositoryMessage::RepoLoaded(Ok(snapshot)));
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
            Err(tabs::OpenRepoError::NotARepo(path)) => self.show_toast(ToastData::error(
                "Not a Git Repository",
                format!("{} is not a git repository", path.display()),
            )),
        }
    }

    pub(super) fn show_toast(&mut self, toast: ToastData) -> Task<Message> {
        let toast_id = self.toasts.push(toast);
        self.reset_animation_clock_if_idle();
        auto_dismiss_task(toast_id)
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

use iced::Task;

use crate::{
    message::Message,
    services::GitError,
    toast::ToastData,
    view_model::{LoadedCherryPickOutcome, LoadedRepo},
};

use super::super::overlays::ActiveDialog;
use super::super::{CenterViewMode, RepositoryScreen};

pub(super) fn on_dirty_commit_created(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            screen.panels.detail.clear_commit_message();
            super::helpers::handle_repo_loaded(screen, loaded)
        }
        Err(e) => {
            eprintln!("git_leviathan: commit failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Commit Failed",
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_dirty_merge_aborted(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            screen.panels.detail.clear_commit_message();
            super::helpers::handle_repo_loaded(screen, loaded)
        }
        Err(e) => {
            eprintln!("git_leviathan: abort merge failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Abort Merge Failed",
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_dirty_index_changed(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::Discard(_))
            ) {
                screen.overlay_manager.close();
            }
            super::helpers::handle_repo_loaded(screen, loaded)
        }
        Err(e) => {
            eprintln!("git_leviathan: index update failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Index Update Failed",
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_cherry_pick_completed(
    screen: &mut RepositoryScreen,
    result: Result<LoadedCherryPickOutcome, GitError>,
) -> Task<Message> {
    match result {
        Ok(LoadedCherryPickOutcome::Committed(loaded)) => super::helpers::handle_repo_loaded(screen, loaded),
        Ok(LoadedCherryPickOutcome::Conflicted(loaded)) => {
            let load_task = super::helpers::handle_repo_loaded(screen, loaded);
            let toast_task = Task::done(Message::show_toast(ToastData::error(
                "Cherry pick Failed",
                "There are merge conflicts that need to be resolved",
            )));
            Task::batch(vec![load_task, toast_task])
        }
        Err(e) => {
            eprintln!("git_leviathan: cherry-pick failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Cherry pick Failed",
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_conflict_resolution_saved(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            screen.panels.diff.conflict_file_resolution = None;
            screen.panels.diff.center_view_mode = CenterViewMode::Graph;
            let scroll_task = screen.panels.center.restore_center_list_scroll();
            let load_task = super::helpers::handle_repo_loaded(screen, loaded);
            Task::batch(vec![scroll_task, load_task])
        }
        Err(e) => {
            eprintln!("git_leviathan: conflict resolution save failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Save Conflict Resolution Failed",
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_squash_completed(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            screen.data.selection.select_commit(0);
            super::helpers::handle_repo_loaded(screen, loaded)
        }
        Err(e) => {
            eprintln!("git_leviathan: squash failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Squash Failed",
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_reword_completed(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            screen.panels.detail.cancel_reword();
            super::helpers::handle_repo_loaded(screen, loaded)
        }
        Err(e) => {
            eprintln!("git_leviathan: reword failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Reword Failed",
                e.to_string(),
            )))
        }
    }
}

use iced::Task;

use crate::{
    message::Message,
    services::GitError,
    toast::ToastData,
    view_model::{LoadedCherryPickOutcome, LoadedDirtyIndex, LoadedRepo, LoadedRevertOutcome},
};

use super::super::overlays::ActiveDialog;
use super::super::state::OperationId;
use super::super::{CenterViewMode, RepositoryScreen};

pub(super) fn on_dirty_commit_created(
    screen: &mut RepositoryScreen,
    operation_id: OperationId,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if !screen.finish_git_write(operation_id) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            screen.panels.detail.clear_commit_message();
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: commit failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    "Commit Failed",
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_dirty_merge_aborted(
    screen: &mut RepositoryScreen,
    operation_id: OperationId,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if !screen.finish_git_write(operation_id) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            screen.panels.detail.clear_commit_message();
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: abort merge failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    "Abort Merge Failed",
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_dirty_index_changed(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::Discard(_))
            ) {
                screen.overlay_manager.close();
            }
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: index update failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    "Index Update Failed",
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_dirty_index_reloaded(
    screen: &mut RepositoryScreen,
    operation_id: OperationId,
    result: Result<LoadedDirtyIndex, GitError>,
) -> Task<Message> {
    if !screen.finish_git_write(operation_id) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::Discard(_))
            ) {
                screen.overlay_manager.close();
            }
            if !screen.data.apply_dirty_index_update(loaded) {
                let task = screen.reload_task();
                return super::helpers::pending_reload_task_after_write(screen, task);
            }
            let task = super::helpers::sync_dirty_diff_after_reload(screen);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: index update failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    "Index Update Failed",
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_cherry_pick_completed(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    result: Result<LoadedCherryPickOutcome, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    let task = match result {
        Ok(LoadedCherryPickOutcome::Committed(loaded)) => {
            super::helpers::handle_repo_loaded(screen, loaded)
        }
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
    };
    super::helpers::pending_reload_task_after_write(screen, task)
}

pub(super) fn on_revert_completed(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    result: Result<LoadedRevertOutcome, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    let task = match result {
        Ok(LoadedRevertOutcome::Committed(loaded)) | Ok(LoadedRevertOutcome::Applied(loaded)) => {
            super::helpers::handle_repo_loaded(screen, loaded)
        }
        Ok(LoadedRevertOutcome::RevertConflicted(loaded)) => {
            let load_task = super::helpers::handle_repo_loaded(screen, loaded);
            let toast_task = Task::done(Message::show_toast(ToastData::error(
                "Revert Failed",
                "There are merge conflicts that need to be resolved",
            )));
            Task::batch(vec![load_task, toast_task])
        }
        Ok(LoadedRevertOutcome::StashRestoreConflicted(loaded)) => {
            let load_task = super::helpers::handle_repo_loaded(screen, loaded);
            let toast_task = Task::done(Message::show_toast(ToastData::error(
                "Revert Completed With Conflicts",
                "Restoring stashed changes caused conflicts that need to be resolved",
            )));
            Task::batch(vec![load_task, toast_task])
        }
        Err(e) => {
            eprintln!("git_leviathan: revert failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Revert Failed",
                e.to_string(),
            )))
        }
    };
    super::helpers::pending_reload_task_after_write(screen, task)
}

pub(super) fn on_conflict_resolution_saved(
    screen: &mut RepositoryScreen,
    operation_id: OperationId,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if !screen.finish_git_write(operation_id) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            screen.panels.diff.conflict_file_resolution = None;
            screen.panels.diff.center_view_mode = CenterViewMode::Graph;
            let scroll_task = screen.panels.center.restore_center_list_scroll();
            let load_task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::batch(vec![scroll_task, load_task]),
            )
        }
        Err(e) => {
            eprintln!("git_leviathan: conflict resolution save failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    "Save Conflict Resolution Failed",
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_squash_completed(
    screen: &mut RepositoryScreen,
    operation_id: OperationId,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if !screen.finish_git_write(operation_id) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            screen.data.selection.select_commit(0);
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: squash failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    "Squash Failed",
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_reword_completed(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            screen.panels.detail.cancel_reword();
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: reword failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    "Reword Failed",
                    e.to_string(),
                ))),
            )
        }
    }
}

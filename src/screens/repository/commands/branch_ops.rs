use iced::Task;

use crate::{
    message::Message,
    services::GitError,
    toast::ToastData,
    view_model::{LoadedBranchMergeOutcome, LoadedRemoteCheckoutOutcome, LoadedRepo},
};

use super::super::overlays::{validation, ActiveDialog};
use super::super::state::OperationId;
use super::super::RepositoryScreen;

pub(super) fn on_remote_checkout_completed(
    screen: &mut RepositoryScreen,
    operation_id: OperationId,
    result: Result<LoadedRemoteCheckoutOutcome, GitError>,
) -> Task<Message> {
    if !screen.finish_git_write(operation_id) {
        return Task::none();
    }
    match result {
        Ok(outcome) => {
            let task = super::helpers::handle_remote_checkout(screen, outcome);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: remote branch checkout failed: {}", e);
            super::helpers::pending_reload_task_after_write(screen, Task::none())
        }
    }
}

pub(super) fn on_branch_deleted(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    branch_name: String,
    is_remote: bool,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::DeleteBranch(_))
            ) {
                screen.overlay_manager.close();
            }
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: branch delete failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::delete_failed(
                    &branch_name,
                    is_remote,
                    validation::humanize_git_error(&e),
                ))),
            )
        }
    }
}

pub(super) fn on_branch_renamed(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    old_name: String,
    new_name: String,
    is_remote: bool,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::RenameBranch(_))
            ) {
                screen.overlay_manager.close();
            }
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: branch rename failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::rename_failed(
                    &old_name,
                    &new_name,
                    is_remote,
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_branch_created(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    branch_name: String,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::CreateBranchHere(_))
            ) {
                screen.overlay_manager.close();
            }
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: branch creation failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::create_failed(
                    &branch_name,
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_branch_merged(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    source_branch: String,
    target_branch: String,
    result: Result<LoadedBranchMergeOutcome, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    match result {
        Ok(outcome) => {
            let task = super::helpers::handle_branch_merge_outcome(
                screen,
                source_branch,
                target_branch,
                outcome,
            );
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: branch merge failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    format!("Merge Failed: {} into {}", source_branch, target_branch),
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_branch_rebased(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    source_branch: String,
    target_display: String,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: branch rebase failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    format!("Rebase Failed: {} onto {}", source_branch, target_display),
                    e.to_string(),
                ))),
            )
        }
    }
}

use iced::Task;

use crate::{
    message::Message,
    services::GitError,
    toast::ToastData,
    view_model::{LoadedBranchMergeOutcome, LoadedRemoteCheckoutOutcome, LoadedRepo},
    work::git_write_work,
};

use super::super::overlays::validation;
use super::super::state::OperationId;
use super::super::{RepositoryMessage, RepositoryScreen};

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
            if screen.overlay_manager.is_delete_branch_dialog_open() {
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
            if screen.overlay_manager.is_rename_branch_dialog_open() {
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
            if screen.overlay_manager.is_create_branch_dialog_open() {
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

pub(in crate::screens::repository) fn delete_branch_direct(
    screen: &mut RepositoryScreen,
    branch_name: String,
    is_remote: bool,
    remote_ref: Option<String>,
) -> Task<Message> {
    let branch_ref = if is_remote {
        remote_ref.unwrap_or_else(|| branch_name.clone())
    } else {
        branch_name.clone()
    };
    let Some(operation_id) = screen.begin_git_write() else {
        return Task::none();
    };
    let repo = screen.fleet.active().clone();
    let presenter = screen.presenter.clone();
    let tab_id = screen.tab_id;
    Task::perform(
        git_write_work(move || {
            repo.delete_branch(&branch_ref, is_remote)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::BranchDeleted {
                    operation_id: Some(operation_id),
                    branch_name: branch_name.clone(),
                    is_remote,
                    result,
                },
            )
        },
    )
}

pub(in crate::screens::repository) fn delete_branch_local_and_remote(
    screen: &mut RepositoryScreen,
    branch_name: String,
) -> Task<Message> {
    let Some(operation_id) = screen.begin_git_write() else {
        return Task::none();
    };
    let repo = screen.fleet.active().clone();
    let presenter = screen.presenter.clone();
    let tab_id = screen.tab_id;
    let delete_branch_name = branch_name.clone();
    Task::perform(
        git_write_work(move || {
            repo.delete_branch_all(&delete_branch_name)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::BranchDeleted {
                    operation_id: Some(operation_id),
                    branch_name: branch_name.clone(),
                    is_remote: false,
                    result,
                },
            )
        },
    )
}

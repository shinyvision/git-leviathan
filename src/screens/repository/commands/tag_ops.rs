use iced::Task;

use crate::{
    message::Message, services::GitError, toast::ToastData, view_model::LoadedRepo,
    work::git_write_work,
};

use super::super::state::OperationId;
use super::super::{RepositoryMessage, RepositoryScreen};

pub(super) fn on_tag_created(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    tag_name: String,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            if screen.overlay_manager.is_create_tag_dialog_open() {
                screen.overlay_manager.close();
            }
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: create tag failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    format!("Create Tag Failed: {}", tag_name),
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_tag_deleted(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    tag_name: String,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            let remote_names = screen.overlay_manager.delete_tag_remote_names();
            if screen.overlay_manager.is_delete_tag_dialog_open() {
                screen.overlay_manager.close();
            }
            let reload_task = super::helpers::handle_repo_loaded(screen, loaded);
            let tab_id = screen.tab_id;
            let mut tasks = vec![reload_task];
            for remote in remote_names {
                let repo = screen.fleet.active().clone();
                let presenter = screen.presenter.clone();
                let tag_clone = tag_name.clone();
                let remote_clone = remote.clone();
                let tag_for_msg = tag_name.clone();
                let remote_for_msg = remote.clone();
                tasks.push(Task::perform(
                    git_write_work(move || {
                        repo.delete_remote_tag(&remote_clone, &tag_clone)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(
                            tab_id,
                            RepositoryMessage::TagDeletedFromRemote {
                                operation_id: None,
                                tag_name: tag_for_msg.clone(),
                                remote_name: remote_for_msg.clone(),
                                result,
                            },
                        )
                    },
                ));
            }
            super::helpers::pending_reload_task_after_write(screen, Task::batch(tasks))
        }
        Err(e) => {
            eprintln!("git_leviathan: delete tag failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    format!("Delete Tag Failed: {}", tag_name),
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_tag_pushed(
    screen: &mut RepositoryScreen,
    operation_id: OperationId,
    tag_name: String,
    remote_name: String,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if !screen.finish_git_write(operation_id) {
        return Task::none();
    }
    let task = match result {
        Ok(loaded) => {
            let toast = Task::done(Message::show_toast(ToastData::success(
                format!("Tag pushed: {}", tag_name),
                format!("pushed to {}", remote_name),
            )));
            let reload = super::helpers::handle_repo_loaded(screen, loaded);
            Task::batch(vec![reload, toast])
        }
        Err(e) => {
            eprintln!("git_leviathan: push tag failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                format!("Push Tag Failed: {}", tag_name),
                e.to_string(),
            )))
        }
    };
    super::helpers::pending_reload_task_after_write(screen, task)
}

pub(super) fn on_tag_deleted_from_remote(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    tag_name: String,
    remote_name: String,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            let toast = Task::done(Message::show_toast(ToastData::success(
                format!("Tag removed from remote: {}", tag_name),
                format!("deleted from {}", remote_name),
            )));
            let reload = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::batch(vec![reload, toast]),
            )
        }
        Err(e) => {
            eprintln!("git_leviathan: delete remote tag failed: {}", e);
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    format!("Delete Remote Tag Failed: {}", tag_name),
                    e.to_string(),
                ))),
            )
        }
    }
}

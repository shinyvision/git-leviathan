use iced::Task;

use crate::{message::Message, services::GitError, toast::ToastData, view_model::LoadedRepo};

use super::super::gateway_work;
use super::super::overlays::ActiveDialog;
use super::super::{RepositoryMessage, RepositoryScreen};

pub(super) fn on_tag_created(
    screen: &mut RepositoryScreen,
    tag_name: String,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::CreateTagHere(_))
            ) {
                screen.overlay_manager.close();
            }
            super::helpers::handle_repo_loaded(screen, loaded)
        }
        Err(e) => {
            eprintln!("git_leviathan: create tag failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                format!("Create Tag Failed: {}", tag_name),
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_tag_deleted(
    screen: &mut RepositoryScreen,
    tag_name: String,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            let remote_names = match screen.overlay_manager.active() {
                Some(ActiveDialog::DeleteTag(s)) => s.tag_remote_names.clone(),
                _ => Vec::new(),
            };
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::DeleteTag(_))
            ) {
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
                    gateway_work(move || {
                        repo.delete_remote_tag(&remote_clone, &tag_clone)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(
                            tab_id,
                            RepositoryMessage::TagDeletedFromRemote {
                                tag_name: tag_for_msg.clone(),
                                remote_name: remote_for_msg.clone(),
                                result,
                            },
                        )
                    },
                ));
            }
            Task::batch(tasks)
        }
        Err(e) => {
            eprintln!("git_leviathan: delete tag failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                format!("Delete Tag Failed: {}", tag_name),
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_tag_pushed(
    screen: &mut RepositoryScreen,
    tag_name: String,
    remote_name: String,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
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
    }
}

pub(super) fn on_tag_deleted_from_remote(
    screen: &mut RepositoryScreen,
    tag_name: String,
    remote_name: String,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            let toast = Task::done(Message::show_toast(ToastData::success(
                format!("Tag removed from remote: {}", tag_name),
                format!("deleted from {}", remote_name),
            )));
            let reload = super::helpers::handle_repo_loaded(screen, loaded);
            Task::batch(vec![reload, toast])
        }
        Err(e) => {
            eprintln!("git_leviathan: delete remote tag failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                format!("Delete Remote Tag Failed: {}", tag_name),
                e.to_string(),
            )))
        }
    }
}

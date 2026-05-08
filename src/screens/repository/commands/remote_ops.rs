use iced::Task;

use crate::{
    message::Message,
    services::GitError,
    toast::ToastData,
    view_model::{LoadedPushOutcome, LoadedRepo},
    work::git_write_work,
};

use super::super::overlays::{push_behind, set_upstream, ActiveDialog};
use super::super::state::OperationId;
use super::super::{RepositoryMessage, RepositoryScreen};

pub(super) fn on_remote_added(
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
                Some(ActiveDialog::AddRemote(_))
            ) {
                screen.overlay_manager.close();
            }
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: add remote failed: {}", e);
            if let Some(state) = screen.overlay_manager.as_add_remote_mut() {
                state.submitting = false;
            }
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    "Add Remote Failed",
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_push_requested(screen: &mut RepositoryScreen) -> Task<Message> {
    if screen.data.animation.network_op_in_flight() {
        return Task::none();
    }
    let Some(operation_id) = screen.begin_git_write() else {
        return Task::none();
    };
    screen.data.animation.mark_push_started();
    let repo = screen.fleet.active().clone();
    let presenter = screen.presenter.clone();
    let tab_id = screen.tab_id;
    Task::perform(
        git_write_work(move || {
            repo.push_current_branch()
                .map(|o| presenter.project_push(o))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::PushCompleted {
                    operation_id,
                    result,
                },
            )
        },
    )
}

pub(super) fn on_push_completed(
    screen: &mut RepositoryScreen,
    operation_id: OperationId,
    result: Result<LoadedPushOutcome, GitError>,
) -> Task<Message> {
    if !screen.finish_git_write(operation_id) {
        return Task::none();
    }
    screen.data.animation.clear_push();
    let task = match result {
        Ok(LoadedPushOutcome::Pushed(loaded)) => {
            let branch_name = screen.data.snapshot.current_branch().to_string();
            let task = super::helpers::handle_repo_loaded(screen, *loaded);
            Task::batch(vec![
                task,
                Task::done(Message::show_toast(ToastData::push_succeeded(&branch_name))),
            ])
        }
        Ok(LoadedPushOutcome::NeedsUpstream {
            branch_name,
            remote_name,
        }) => {
            screen
                .overlay_manager
                .open(ActiveDialog::SetUpstream(set_upstream_state(
                    screen,
                    branch_name,
                    remote_name,
                )));
            Task::none()
        }
        Ok(LoadedPushOutcome::BehindRemote {
            branch_name,
            remote_name,
        }) => {
            screen
                .overlay_manager
                .open(ActiveDialog::PushBehind(push_behind::State {
                    branch_name,
                    remote_name,
                }));
            screen.panels.center.restore_center_list_scroll()
        }
        Err(e) => {
            eprintln!("git_leviathan: push failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Push Failed",
                e.to_string(),
            )))
        }
    };
    super::helpers::pending_reload_task_after_write(screen, task)
}

pub(super) fn on_set_upstream_push_completed(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    match result {
        Ok(loaded) => {
            let branch_name = screen.data.snapshot.current_branch().to_string();
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::SetUpstream(_))
            ) {
                screen.overlay_manager.close();
            }
            let task = super::helpers::handle_repo_loaded(screen, loaded);
            let task = Task::batch(vec![
                task,
                Task::done(Message::show_toast(ToastData::push_succeeded(&branch_name))),
            ]);
            super::helpers::pending_reload_task_after_write(screen, task)
        }
        Err(e) => {
            eprintln!("git_leviathan: push failed: {}", e);
            if let Some(state) = screen.overlay_manager.as_set_upstream_mut() {
                state.submitting = false;
            }
            super::helpers::pending_reload_task_after_write(
                screen,
                Task::done(Message::show_toast(ToastData::error(
                    "Push Failed",
                    e.to_string(),
                ))),
            )
        }
    }
}

pub(super) fn on_force_push_completed(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    result: Result<LoadedPushOutcome, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    let task = match result {
        Ok(LoadedPushOutcome::Pushed(loaded)) => {
            let branch_name = screen.data.snapshot.current_branch().to_string();
            let task = super::helpers::handle_repo_loaded(screen, *loaded);
            Task::batch(vec![
                task,
                Task::done(Message::show_toast(ToastData::push_succeeded(&branch_name))),
            ])
        }
        Ok(LoadedPushOutcome::NeedsUpstream {
            branch_name,
            remote_name,
        }) => {
            screen
                .overlay_manager
                .open(ActiveDialog::SetUpstream(set_upstream_state(
                    screen,
                    branch_name,
                    remote_name,
                )));
            Task::none()
        }
        Ok(LoadedPushOutcome::BehindRemote { .. }) => {
            Task::done(Message::show_toast(ToastData::error(
                "Force Push Failed",
                "Branch is still behind remote".to_string(),
            )))
        }
        Err(e) => {
            eprintln!("git_leviathan: force push failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Force Push Failed",
                e.to_string(),
            )))
        }
    };
    super::helpers::pending_reload_task_after_write(screen, task)
}

fn set_upstream_state(
    screen: &RepositoryScreen,
    branch_name: String,
    proposed_remote_name: String,
) -> set_upstream::State {
    set_upstream::State::new(
        branch_name,
        proposed_remote_name.clone(),
        available_remote_names(screen, &proposed_remote_name),
    )
}

fn available_remote_names(screen: &RepositoryScreen, proposed_remote_name: &str) -> Vec<String> {
    let mut remotes = Vec::new();

    push_unique_remote(&mut remotes, proposed_remote_name);
    for remote_name in screen.remote_names() {
        push_unique_remote(&mut remotes, remote_name);
    }
    if let Some(default_remote_name) = screen.default_remote_name() {
        push_unique_remote(&mut remotes, default_remote_name);
    }

    remotes
}

fn push_unique_remote(remotes: &mut Vec<String>, remote_name: &str) {
    let remote_name = remote_name.trim();
    if remote_name.is_empty() || remotes.iter().any(|remote| remote == remote_name) {
        return;
    }
    remotes.push(remote_name.to_string());
}

pub(super) fn on_pull_requested(screen: &mut RepositoryScreen) -> Task<Message> {
    if screen.data.animation.network_op_in_flight() {
        return Task::none();
    }
    let Some(operation_id) = screen.begin_git_write() else {
        return Task::none();
    };
    screen.data.animation.mark_pull_started();
    let repo = screen.fleet.active().clone();
    let presenter = screen.presenter.clone();
    let tab_id = screen.tab_id;
    Task::perform(
        git_write_work(move || {
            repo.pull_current_branch()
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::PullCompleted {
                    operation_id: Some(operation_id),
                    result,
                },
            )
        },
    )
}

pub(super) fn on_pull_completed(
    screen: &mut RepositoryScreen,
    operation_id: Option<OperationId>,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    if operation_id.is_some_and(|id| !screen.finish_git_write(id)) {
        return Task::none();
    }
    screen.data.animation.clear_pull();
    let task = match result {
        Ok(loaded) => super::helpers::handle_repo_loaded(screen, loaded),
        Err(e) => {
            eprintln!("git_leviathan: pull failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Pull Failed",
                e.to_string(),
            )))
        }
    };
    super::helpers::pending_reload_task_after_write(screen, task)
}

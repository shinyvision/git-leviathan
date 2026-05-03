use iced::Task;

use crate::{
    message::Message,
    services::GitError,
    toast::ToastData,
    view_model::{LoadedPushOutcome, LoadedRepo},
};

use super::super::gateway_work;
use super::super::overlays::{push_behind, set_upstream, ActiveDialog};
use super::super::{RepositoryMessage, RepositoryScreen};

pub(super) fn on_remote_added(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::AddRemote(_))
            ) {
                screen.overlay_manager.close();
            }
            super::helpers::handle_repo_loaded(screen, loaded)
        }
        Err(e) => {
            eprintln!("git_leviathan: add remote failed: {}", e);
            if let Some(state) = screen.overlay_manager.as_add_remote_mut() {
                state.submitting = false;
            }
            Task::done(Message::show_toast(ToastData::error(
                "Add Remote Failed",
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_push_requested(screen: &mut RepositoryScreen) -> Task<Message> {
    if screen.data.animation.network_op_in_flight() {
        return Task::none();
    }
    screen.data.animation.mark_push_started();
    let repo = screen.fleet.active().clone();
    let presenter = screen.presenter.clone();
    let tab_id = screen.tab_id;
    Task::perform(
        gateway_work(move || {
            repo.push_current_branch()
                .map(|o| presenter.project_push(o))
        }),
        move |result| Message::tab(tab_id, RepositoryMessage::PushCompleted(result)),
    )
}

pub(super) fn on_push_completed(
    screen: &mut RepositoryScreen,
    result: Result<LoadedPushOutcome, GitError>,
) -> Task<Message> {
    screen.data.animation.clear_push();
    match result {
        Ok(LoadedPushOutcome::Pushed(loaded)) => {
            let branch_name = screen.data.snapshot.current_branch().to_string();
            let task = super::helpers::handle_repo_loaded(screen, loaded);
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
                .open(ActiveDialog::SetUpstream(set_upstream::State {
                    branch_name: branch_name.clone(),
                    remote_name,
                    new_branch_input: branch_name,
                    needs_focus: true,
                    submitting: false,
                }));
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
    }
}

pub(super) fn on_set_upstream_push_completed(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
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
            Task::batch(vec![
                task,
                Task::done(Message::show_toast(ToastData::push_succeeded(&branch_name))),
            ])
        }
        Err(e) => {
            eprintln!("git_leviathan: push failed: {}", e);
            if let Some(state) = screen.overlay_manager.as_set_upstream_mut() {
                state.submitting = false;
            }
            Task::done(Message::show_toast(ToastData::error(
                "Push Failed",
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_force_push_completed(
    screen: &mut RepositoryScreen,
    result: Result<LoadedPushOutcome, GitError>,
) -> Task<Message> {
    match result {
        Ok(LoadedPushOutcome::Pushed(loaded)) => {
            let branch_name = screen.data.snapshot.current_branch().to_string();
            let task = super::helpers::handle_repo_loaded(screen, loaded);
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
                .open(ActiveDialog::SetUpstream(set_upstream::State {
                    branch_name: branch_name.clone(),
                    remote_name,
                    new_branch_input: branch_name,
                    needs_focus: true,
                    submitting: false,
                }));
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
    }
}

pub(super) fn on_pull_requested(screen: &mut RepositoryScreen) -> Task<Message> {
    if screen.data.animation.network_op_in_flight() {
        return Task::none();
    }
    screen.data.animation.mark_pull_started();
    let repo = screen.fleet.active().clone();
    let presenter = screen.presenter.clone();
    let tab_id = screen.tab_id;
    Task::perform(
        gateway_work(move || {
            repo.pull_current_branch()
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| Message::tab(tab_id, RepositoryMessage::PullCompleted(result)),
    )
}

pub(super) fn on_pull_completed(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    screen.data.animation.clear_pull();
    match result {
        Ok(loaded) => super::helpers::handle_repo_loaded(screen, loaded),
        Err(e) => {
            eprintln!("git_leviathan: pull failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Pull Failed",
                e.to_string(),
            )))
        }
    }
}

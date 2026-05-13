use iced::Task;

use crate::{
    message::Message,
    screens::repository::{panel_messages::OverlayPanelAction, RepositoryMessage},
    work::git_write_work,
};

use super::{
    add_remote, create_worktree, remove_worktree, DialogCtx, DialogDispatch, OverlayManager,
    SidePanelOverlay,
};

pub(super) fn dispatch(
    manager: &mut OverlayManager,
    action: OverlayPanelAction,
    ctx: DialogCtx<'_>,
) -> DialogDispatch {
    match action {
        OverlayPanelAction::AddRemoteOpen => {
            manager.open_side_panel(SidePanelOverlay::AddRemote(add_remote::State::new()));
            DialogDispatch::RestoreCenterListScroll
        }
        OverlayPanelAction::AddRemoteClose => {
            if let Some(state) = manager.as_add_remote_mut() {
                state.start_close();
            }
            DialogDispatch::Task(Task::none())
        }
        OverlayPanelAction::AddRemoteNameChanged(value) => {
            if let Some(state) = manager.as_add_remote_mut() {
                state.name = value;
            }
            DialogDispatch::Task(Task::none())
        }
        OverlayPanelAction::AddRemotePullUrlChanged(value) => {
            if let Some(state) = manager.as_add_remote_mut() {
                state.pull_url = value;
            }
            DialogDispatch::Task(Task::none())
        }
        OverlayPanelAction::AddRemotePushUrlChanged(value) => {
            if let Some(state) = manager.as_add_remote_mut() {
                state.push_url = value;
            }
            DialogDispatch::Task(Task::none())
        }
        OverlayPanelAction::AddRemoteConfirmed => {
            let Some(state) = manager.as_add_remote_mut() else {
                return DialogDispatch::Task(Task::none());
            };
            let name = state.name.trim().to_string();
            let pull_url = state.pull_url.trim().to_string();
            let push_url = state.push_url.trim().to_string();
            if name.is_empty() || pull_url.is_empty() {
                return DialogDispatch::Task(Task::none());
            }
            let Some(operation_id) = ctx.operations.begin_write() else {
                return DialogDispatch::Task(Task::none());
            };
            state.submitting = true;
            let DialogCtx {
                repository,
                presenter,
                tab_id,
                ..
            } = ctx;
            let task = Task::perform(
                git_write_work(move || {
                    repository
                        .add_remote(&name, &pull_url, &push_url)
                        .map(|s| presenter.project_loaded(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::RemoteAdded {
                            operation_id: Some(operation_id),
                            result,
                        },
                    )
                },
            );
            DialogDispatch::Task(task)
        }
        OverlayPanelAction::CreateWorktreeOpen {
            available_refs,
            default_dir_prefix,
        } => {
            manager.open_side_panel(SidePanelOverlay::CreateWorktree(
                create_worktree::State::new(available_refs, default_dir_prefix),
            ));
            DialogDispatch::RestoreCenterListScroll
        }
        OverlayPanelAction::CreateWorktreeClose => {
            if let Some(state) = manager.as_create_worktree_mut() {
                state.start_close();
            }
            DialogDispatch::Task(Task::none())
        }
        OverlayPanelAction::CreateWorktreeReferenceChanged(choice) => {
            if let Some(state) = manager.as_create_worktree_mut() {
                state.set_reference(choice);
                state.dropdown_open = false;
            }
            DialogDispatch::Task(Task::none())
        }
        OverlayPanelAction::CreateWorktreeDropdownToggled => {
            if let Some(state) = manager.as_create_worktree_mut() {
                state.dropdown_open = !state.dropdown_open;
            }
            DialogDispatch::Task(Task::none())
        }
        OverlayPanelAction::CreateWorktreeBranchNameChanged(value) => {
            if let Some(state) = manager.as_create_worktree_mut() {
                state.set_branch_name(value);
            }
            DialogDispatch::Task(Task::none())
        }
        OverlayPanelAction::CreateWorktreeWorkingDirChanged(value) => {
            if let Some(state) = manager.as_create_worktree_mut() {
                state.set_working_dir(value);
            }
            DialogDispatch::Task(Task::none())
        }
        OverlayPanelAction::CreateWorktreeBrowseRequested => {
            let task = Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Choose worktree directory")
                        .pick_folder()
                        .await
                        .map(|handle| handle.path().to_path_buf())
                },
                |chosen| {
                    Message::repo(RepositoryMessage::OverlayPanel(
                        OverlayPanelAction::CreateWorktreeBrowseResolved(chosen),
                    ))
                },
            );
            DialogDispatch::Task(task)
        }
        OverlayPanelAction::CreateWorktreeBrowseResolved(chosen) => {
            if let (Some(state), Some(path)) = (manager.as_create_worktree_mut(), chosen) {
                state.set_working_dir(path.to_string_lossy().to_string());
            }
            DialogDispatch::Task(Task::none())
        }
        OverlayPanelAction::CreateWorktreeConfirmed => {
            let Some(state) = manager.as_create_worktree_mut() else {
                return DialogDispatch::Task(Task::none());
            };
            if !state.can_submit() {
                return DialogDispatch::Task(Task::none());
            }
            let ref_git = state
                .reference
                .as_ref()
                .map(|r| r.git_ref())
                .unwrap_or_default();
            let branch_name = state.branch_name.trim().to_string();
            let working_dir = std::path::PathBuf::from(state.working_dir.trim());
            let Some(operation_id) = ctx.operations.begin_write() else {
                return DialogDispatch::Task(Task::none());
            };
            state.submitting = true;
            state.error = None;

            let DialogCtx {
                primary_repository,
                presenter,
                tab_id,
                ..
            } = ctx;
            let task = Task::perform(
                git_write_work(move || {
                    primary_repository
                        .add_worktree(working_dir, Some(branch_name), ref_git)
                        .map(|s| presenter.project_loaded(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::WorktreeCreated {
                            operation_id: Some(operation_id),
                            result,
                        },
                    )
                },
            );
            DialogDispatch::Task(task)
        }
        OverlayPanelAction::WorktreeRemoveRequested {
            path,
            branch_name,
            is_active,
        } => {
            manager.open_toolbar_dialog(remove_worktree::dialog(remove_worktree::State {
                path,
                branch_name,
                is_active,
            }));
            DialogDispatch::RestoreCenterListScroll
        }
        _ => unreachable!("non-side-panel action routed to side-panel dispatch"),
    }
}

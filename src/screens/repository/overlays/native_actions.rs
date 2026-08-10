use iced::Task;

use crate::{
    message::Message, services::ModifyDeleteConflictChoice, toast::ToastData, work::git_write_work,
};

use super::super::{state::OperationKind, RepositoryMessage};
use super::{
    cherry_pick_confirm, conflict_checkout, create_branch, create_tag, delete_branch, delete_tag,
    discard, force_push, modify_delete_conflict, push_behind, remove_worktree, rename_branch,
    revert_confirm, set_upstream, stash_delete, DialogCtx, DialogDispatch, OverlayManager,
};

pub(super) fn resolve_modify_delete_conflict(
    manager: &mut OverlayManager,
    choice: ModifyDeleteConflictChoice,
    ctx: DialogCtx<'_>,
) -> DialogDispatch {
    let Some(path) = manager
        .toolbar_dialog
        .as_ref()
        .and_then(modify_delete_conflict::path)
    else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(operation_id) = ctx
        .operations
        .begin_write_kind(OperationKind::ResolveConflict)
    else {
        return DialogDispatch::Task(Task::none());
    };
    manager.close();
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let task = Task::perform(
        git_write_work(move || {
            repository
                .resolve_modify_delete_conflict(&path, choice)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::DirtyIndexChanged {
                    operation_id: Some(operation_id),
                    result,
                },
            )
        },
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_force_push(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    manager.close();
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let task = Task::perform(
        git_write_work(move || {
            repository
                .force_push_current_branch()
                .map(|o| presenter.project_push(o))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::ForcePushCompleted {
                    operation_id: Some(operation_id),
                    result,
                },
            )
        },
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_stash_delete(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(stash_index) = manager
        .toolbar_dialog
        .as_ref()
        .and_then(stash_delete::stash_index)
    else {
        return DialogDispatch::Task(Task::none());
    };
    // Resolve the stash's stable commit hash from the current sections so a
    // shifted index (auto-stash pushed/popped since the dialog opened) can't
    // drop the wrong stash.
    let Some(stash_hash) = ctx
        .sidebar_sections
        .iter()
        .flat_map(|section| section.stashes.iter())
        .find(|stash| stash.stash_index == stash_index)
        .map(|stash| stash.hash.clone())
    else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    manager.close();
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let task = Task::perform(
        git_write_work(move || {
            repository
                .drop_stash(&stash_hash)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::DirtyIndexChanged {
                    operation_id: Some(operation_id),
                    result,
                },
            )
        },
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_delete_tag(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(tag_name) = manager
        .toolbar_dialog
        .as_ref()
        .and_then(delete_tag::tag_name)
    else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let tag_clone = tag_name.clone();
    let task = Task::perform(
        git_write_work(move || {
            repository
                .delete_tag(&tag_clone)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::TagDeleted {
                    operation_id: Some(operation_id),
                    tag_name: tag_name.clone(),
                    result,
                },
            )
        },
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_cherry_pick(
    manager: &mut OverlayManager,
    commit_now: bool,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(commit_hash) = manager
        .toolbar_dialog
        .as_ref()
        .and_then(cherry_pick_confirm::commit_hash)
    else {
        return DialogDispatch::Task(Task::none());
    };
    manager.close();
    DialogDispatch::Task(spawn_cherry_pick_task(commit_hash, commit_now, ctx))
}

pub(super) fn confirm_revert(
    manager: &mut OverlayManager,
    commit_now: bool,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(commit_hash) = manager
        .toolbar_dialog
        .as_ref()
        .and_then(revert_confirm::commit_hash)
    else {
        return DialogDispatch::Task(Task::none());
    };
    manager.close();
    DialogDispatch::Task(spawn_revert_task(commit_hash, commit_now, ctx))
}

pub(super) fn confirm_remove_worktree(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(state) = manager
        .toolbar_dialog
        .as_ref()
        .and_then(remove_worktree::state)
    else {
        return DialogDispatch::Task(Task::none());
    };
    if ctx.active_path == state.path {
        let branch_name = state.branch_name;
        manager.close();
        return DialogDispatch::Task(Task::done(Message::show_toast(ToastData::error(
            "Cannot remove focused worktree",
            format!("Switch away from '{branch_name}' first."),
        ))));
    }
    let path = state.path;
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    manager.close();
    let DialogCtx {
        primary_repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let task = Task::perform(
        git_write_work(move || {
            primary_repository
                .remove_worktree(path)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::WorktreeRemoved {
                    operation_id: Some(operation_id),
                    result,
                },
            )
        },
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_discard_dialog(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(target) = manager.toolbar_dialog.as_ref().and_then(discard::target) else {
        return DialogDispatch::Task(Task::none());
    };
    DialogDispatch::Task(spawn_discard_task(target, ctx))
}

pub(super) fn confirm_delete_branch(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(dialog) = manager.toolbar_dialog.as_ref() else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(branch_name) = delete_branch::branch_name(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(is_remote) = delete_branch::is_remote(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(branch_ref) = delete_branch::branch_ref(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let task = Task::perform(
        git_write_work(move || {
            repository
                .delete_branch(&branch_ref, is_remote, true)
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
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_delete_branch_all(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(branch_name) = manager
        .toolbar_dialog
        .as_ref()
        .and_then(delete_branch::branch_name)
    else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let delete_branch_name = branch_name.clone();
    let task = Task::perform(
        git_write_work(move || {
            repository
                .delete_branch_all(&delete_branch_name)
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
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_rename_branch(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(dialog) = manager.toolbar_dialog.as_ref() else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(old_name) = rename_branch::old_name(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(old_ref) = rename_branch::old_ref(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(is_remote) = rename_branch::is_remote(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    let new_name = rename_branch::new_name_input(dialog)
        .unwrap_or_default()
        .trim()
        .to_string();
    if new_name.is_empty() || new_name == old_name {
        manager.close();
        return DialogDispatch::Task(Task::none());
    }
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let new_name_clone = new_name.clone();
    let task = Task::perform(
        git_write_work(move || {
            repository
                .rename_branch(&old_ref, &new_name_clone, is_remote)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::BranchRenamed {
                    operation_id: Some(operation_id),
                    old_name: old_name.clone(),
                    new_name: new_name.clone(),
                    is_remote,
                    result,
                },
            )
        },
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_create_branch(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(dialog) = manager.toolbar_dialog.as_ref() else {
        return DialogDispatch::Task(Task::none());
    };
    let branch_name = create_branch::branch_name_input(dialog)
        .unwrap_or_default()
        .trim()
        .to_string();
    let Some(commit_hash) = create_branch::commit_hash(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    if branch_name.is_empty() {
        manager.close();
        return DialogDispatch::Task(Task::none());
    }
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let branch_name_clone = branch_name.clone();
    let task = Task::perform(
        git_write_work(move || {
            repository
                .create_branch_at_commit(&branch_name_clone, &commit_hash)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::BranchCreated {
                    operation_id: Some(operation_id),
                    branch_name: branch_name.clone(),
                    result,
                },
            )
        },
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_conflict_create_branch(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(dialog) = manager.toolbar_dialog.as_ref() else {
        return DialogDispatch::Task(Task::none());
    };
    let new_name = conflict_checkout::new_branch_input(dialog)
        .unwrap_or_default()
        .trim()
        .to_string();
    if new_name.is_empty() {
        return DialogDispatch::Task(Task::none());
    }
    let Some(remote_ref) = conflict_checkout::remote_ref(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let task = Task::perform(
        git_write_work(move || {
            repository
                .create_branch_from_remote(&new_name, &remote_ref)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::WriteRepoLoaded {
                    operation_id,
                    result,
                },
            )
        },
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_conflict_reset_local(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(dialog) = manager.toolbar_dialog.as_ref() else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(branch_name) = conflict_checkout::branch_name(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(remote_ref) = conflict_checkout::remote_ref(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let task = Task::perform(
        git_write_work(move || {
            repository
                .reset_branch_to_remote(&branch_name, &remote_ref)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::WriteRepoLoaded {
                    operation_id,
                    result,
                },
            )
        },
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_set_upstream(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(dialog) = manager.toolbar_dialog.as_ref() else {
        return DialogDispatch::Task(Task::none());
    };
    let remote_branch = set_upstream::remote_branch_input(dialog)
        .unwrap_or_default()
        .trim()
        .to_string();
    let remote_name = set_upstream::selected_remote_name(dialog)
        .unwrap_or_default()
        .trim()
        .to_string();
    if remote_branch.is_empty() || remote_name.is_empty() {
        return DialogDispatch::Task(Task::none());
    }
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    if let Some(dialog) = manager.toolbar_dialog.as_mut() {
        set_upstream::set_submitting(dialog, true);
        set_upstream::refresh_enabled(dialog);
    }
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let task = Task::perform(
        git_write_work(move || {
            repository
                .push_and_set_upstream(&remote_name, &remote_branch)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::SetUpstreamPushCompleted {
                    operation_id: Some(operation_id),
                    result,
                },
            )
        },
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_push_behind_pull(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    manager.close();
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let task = Task::perform(
        git_write_work(move || {
            repository
                .pull_current_branch()
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
    );
    DialogDispatch::Task(task)
}

pub(super) fn confirm_push_behind_force_push(manager: &mut OverlayManager) -> DialogDispatch {
    let Some(dialog) = manager.toolbar_dialog.as_ref() else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(branch_name) = push_behind::branch_name(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(remote_name) = push_behind::remote_name(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    manager.open_toolbar_dialog(force_push::dialog(force_push::State {
        branch_name,
        remote_name,
    }));
    DialogDispatch::RestoreCenterListScroll
}

pub(super) fn confirm_create_tag(
    manager: &mut OverlayManager,
    ctx: Option<DialogCtx<'_>>,
) -> DialogDispatch {
    let Some(ctx) = ctx else {
        return DialogDispatch::Task(Task::none());
    };
    let Some(dialog) = manager.toolbar_dialog.as_ref() else {
        return DialogDispatch::Task(Task::none());
    };
    let tag_name = create_tag::tag_name_input(dialog)
        .unwrap_or_default()
        .trim()
        .to_string();
    let Some(commit_hash) = create_tag::commit_hash(dialog) else {
        return DialogDispatch::Task(Task::none());
    };
    if tag_name.is_empty() {
        manager.close();
        return DialogDispatch::Task(Task::none());
    }
    let Some(operation_id) = ctx.operations.begin_write() else {
        return DialogDispatch::Task(Task::none());
    };
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    let tag_clone = tag_name.clone();
    let task = Task::perform(
        git_write_work(move || {
            repository
                .create_tag(&tag_clone, &commit_hash)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::TagCreated {
                    operation_id: Some(operation_id),
                    tag_name: tag_name.clone(),
                    result,
                },
            )
        },
    );
    DialogDispatch::Task(task)
}

fn spawn_discard_task(target: discard::Target, ctx: DialogCtx<'_>) -> Task<Message> {
    let Some(operation_id) = ctx.operations.begin_write_kind(OperationKind::Discard) else {
        return Task::none();
    };
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    Task::perform(
        git_write_work(move || {
            let snapshot = match target {
                discard::Target::All => repository.discard_all_dirty_changes(),
                discard::Target::File(path) => repository.discard_file(&path),
                discard::Target::Files { paths, .. } => repository.discard_files(&paths),
            }?;
            Ok(presenter.project_loaded(snapshot))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::DirtyIndexChanged {
                    operation_id: Some(operation_id),
                    result,
                },
            )
        },
    )
}

fn spawn_cherry_pick_task(
    commit_hash: String,
    commit_now: bool,
    ctx: DialogCtx<'_>,
) -> Task<Message> {
    let Some(operation_id) = ctx.operations.begin_write() else {
        return Task::none();
    };
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    Task::perform(
        git_write_work(move || {
            repository
                .cherry_pick_commit(commit_hash, commit_now)
                .map(|o| presenter.project_cherry_pick(o))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::CherryPickCompleted {
                    operation_id: Some(operation_id),
                    result,
                },
            )
        },
    )
}

fn spawn_revert_task(commit_hash: String, commit_now: bool, ctx: DialogCtx<'_>) -> Task<Message> {
    let Some(operation_id) = ctx.operations.begin_write() else {
        return Task::none();
    };
    let DialogCtx {
        repository,
        presenter,
        tab_id,
        ..
    } = ctx;
    Task::perform(
        git_write_work(move || {
            repository
                .revert_commit(commit_hash, commit_now)
                .map(|o| presenter.project_revert(o))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::RevertCompleted {
                    operation_id: Some(operation_id),
                    result,
                },
            )
        },
    )
}

//! Detail panel action handler. Migrated out of `RepositoryScreen` as part of
//! the package-layout extraction.

use iced::{clipboard, Point, Task};

use crate::{
    core::CommitKind,
    message::Message,
    toast::ToastData,
    work::{git_write_work, timer_work},
};

use super::super::super::{
    overlays::{discard, modify_delete_conflict, ActiveDialog, DialogCtx},
    panel_messages::DetailAction,
    state::{FocusedPanel, OperationKind, PendingFocus},
    RepositoryMessage,
};
use super::super::center::CenterPanel;
use super::super::diff::{update_diff, DiffPanel};
use super::super::sidebar::try_resolve_pending_focus;
use super::super::ScreenCtx;
use super::{dirty_commit_message_text, split_commit_message, DetailPanel};

pub(in crate::screens::repository) fn update(
    panel: &mut DetailPanel,
    action: DetailAction,
    ctx: &mut ScreenCtx<'_>,
    repository_panel: &mut CenterPanel,
    diff_panel: &mut DiffPanel,
) -> Task<Message> {
    match action {
        DetailAction::FileViewChanged(view) => {
            ctx.data.selection.set_file_view(view);
            Task::none()
        }
        DetailAction::DirtyFileClicked { path, is_staged } => {
            if diff_panel.is_active() {
                let already_shown = diff_panel
                    .dirty_file_diff
                    .as_ref()
                    .is_some_and(|d| d.file_path == path && d.is_staged == is_staged)
                    || diff_panel
                        .conflict_file_resolution
                        .as_ref()
                        .is_some_and(|d| d.file_path == path);
                if already_shown {
                    diff_panel.close();
                    return repository_panel.restore_center_list_scroll();
                }
            }
            let Some(dirty_commit) = ctx.data.snapshot.commits().first() else {
                return Task::none();
            };
            if dirty_commit.kind != CommitKind::Dirty {
                return Task::none();
            }

            let all_files = dirty_commit
                .conflicted_files
                .iter()
                .chain(dirty_commit.unstaged_files.iter())
                .chain(dirty_commit.staged_files.iter());
            let selected_idx = all_files
                .enumerate()
                .find(|(_, f)| f.path == path)
                .map(|(i, _)| i)
                .unwrap_or(0);

            let is_conflicted = dirty_commit
                .conflicted_files
                .iter()
                .any(|file| file.path == path);

            if is_conflicted {
                match ctx.repository.load_modify_delete_conflict(&path) {
                    Ok(Some(_)) => {
                        ctx.data.commit_search = None;
                        diff_panel.close();
                        ctx.overlay_manager.open(ActiveDialog::ModifyDeleteConflict(
                            modify_delete_conflict::State { path },
                        ));
                        return repository_panel.restore_center_list_scroll();
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("git_leviathan: conflict inspection failed: {}", e);
                        return Task::done(Message::show_toast(ToastData::error(
                            "Open Conflict Failed",
                            e.to_string(),
                        )));
                    }
                }
            }

            ctx.data.commit_search = None;
            let follow_up =
                diff_panel.open_dirty_file_view(path, is_staged, selected_idx, is_conflicted);
            update_diff(diff_panel, follow_up, ctx)
        }
        DetailAction::DirtyFileRightClicked(path) => {
            if matches!(
                ctx.overlay_manager.active(),
                Some(
                    ActiveDialog::ConflictCheckout(_)
                        | ActiveDialog::DeleteBranch(_)
                        | ActiveDialog::RenameBranch(_)
                        | ActiveDialog::CreateBranchHere(_)
                )
            ) {
                ctx.overlay_manager.close();
            }
            let position = ctx.input.last_pointer_position.unwrap_or(Point::ORIGIN);
            ctx.data
                .branch_popout
                .open_dirty_file_context_menu(path, position);
            Task::none()
        }
        DetailAction::StageFile(path) => {
            let Some(operation_id) = ctx
                .data
                .operations
                .begin_write_kind(OperationKind::StageFile)
            else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            let repo_path = ctx.fleet.active_path().display().to_string();
            let path_for_log = path.clone();
            Task::perform(
                git_write_work(move || {
                    let _span = crate::perf::Span::new("git.stage")
                        .field("tab", tab_id)
                        .field("repo", &repo_path)
                        .field("path", &path_for_log);
                    repo.stage_file_and_load_dirty(&path)
                        .map(|s| presenter.project_dirty_index(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DirtyIndexReloaded {
                            operation_id,
                            result,
                        },
                    )
                },
            )
        }
        DetailAction::StageAll => {
            let Some(operation_id) = ctx
                .data
                .operations
                .begin_write_kind(OperationKind::StageAll)
            else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            let repo_path = ctx.fleet.active_path().display().to_string();
            Task::perform(
                git_write_work(move || {
                    let _span = crate::perf::Span::new("git.stage")
                        .field("tab", tab_id)
                        .field("repo", &repo_path)
                        .field("path", "*");
                    repo.stage_all_dirty_changes_and_load_dirty()
                        .map(|s| presenter.project_dirty_index(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DirtyIndexReloaded {
                            operation_id,
                            result,
                        },
                    )
                },
            )
        }
        DetailAction::UnstageFile(path) => {
            let Some(operation_id) = ctx
                .data
                .operations
                .begin_write_kind(OperationKind::UnstageFile)
            else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            let repo_path = ctx.fleet.active_path().display().to_string();
            let path_for_log = path.clone();
            Task::perform(
                git_write_work(move || {
                    let _span = crate::perf::Span::new("git.unstage")
                        .field("tab", tab_id)
                        .field("repo", &repo_path)
                        .field("path", &path_for_log);
                    repo.unstage_file_and_load_dirty(&path)
                        .map(|s| presenter.project_dirty_index(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DirtyIndexReloaded {
                            operation_id,
                            result,
                        },
                    )
                },
            )
        }
        DetailAction::UnstageAll => {
            let Some(operation_id) = ctx
                .data
                .operations
                .begin_write_kind(OperationKind::UnstageAll)
            else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            let repo_path = ctx.fleet.active_path().display().to_string();
            Task::perform(
                git_write_work(move || {
                    let _span = crate::perf::Span::new("git.unstage")
                        .field("tab", tab_id)
                        .field("repo", &repo_path)
                        .field("path", "*");
                    repo.unstage_all_dirty_changes_and_load_dirty()
                        .map(|s| presenter.project_dirty_index(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DirtyIndexReloaded {
                            operation_id,
                            result,
                        },
                    )
                },
            )
        }
        DetailAction::MarkConflictResolved(path) => {
            let Some(operation_id) = ctx
                .data
                .operations
                .begin_write_kind(OperationKind::ResolveConflict)
            else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                git_write_work(move || {
                    repo.mark_conflict_resolved(&path)
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
            )
        }
        DetailAction::MarkAllConflictsResolved => {
            let Some(operation_id) = ctx
                .data
                .operations
                .begin_write_kind(OperationKind::ResolveConflict)
            else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                git_write_work(move || {
                    repo.mark_all_conflicts_resolved()
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
            )
        }
        DetailAction::DiscardAllRequested => {
            if ctx.data.operations.is_writing() {
                return Task::none();
            }
            ctx.data.branch_popout.close_context_menu();
            ctx.overlay_manager
                .open(ActiveDialog::Discard(discard::State {
                    target: discard::Target::All,
                }));
            Task::none()
        }
        DetailAction::DiscardFileRequested(path) => {
            if ctx.data.operations.is_writing() {
                return Task::none();
            }
            ctx.data.branch_popout.close_context_menu();
            ctx.overlay_manager
                .open(ActiveDialog::Discard(discard::State {
                    target: discard::Target::File(path),
                }));
            Task::none()
        }
        DetailAction::DiscardConfirmed => {
            let dctx = DialogCtx {
                repository: ctx.repository.clone(),
                primary_repository: ctx.fleet.primary().clone(),
                presenter: ctx.presenter.clone(),
                tab_id: ctx.tab_id,
                active_path: ctx.fleet.active_path().to_path_buf(),
                operations: &mut ctx.data.operations,
            };
            ctx.overlay_manager.confirm_discard(dctx)
        }
        DetailAction::DiscardCanceled => {
            ctx.overlay_manager.close();
            ctx.input.focused_panel = FocusedPanel::Center;
            Task::none()
        }
        DetailAction::CommitConfirmed => {
            let message = dirty_commit_message_text(&panel.dirty_commit_message);
            if message.is_empty() {
                return Task::none();
            }

            let (summary, description) = split_commit_message(&message);
            if summary.is_empty() {
                return Task::none();
            }
            let Some(operation_id) = ctx.data.operations.begin_write_kind(OperationKind::Commit)
            else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            let repo_path = ctx.fleet.active_path().display().to_string();
            Task::perform(
                git_write_work(move || {
                    let _span = crate::perf::Span::new("git.commit")
                        .field("tab", tab_id)
                        .field("repo", &repo_path)
                        .field("summary_len", summary.len())
                        .field("description_len", description.len());
                    repo.commit_dirty_changes(&summary, &description)
                        .map(|s| presenter.project_loaded(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DirtyCommitCreated {
                            operation_id,
                            result,
                        },
                    )
                },
            )
        }
        DetailAction::AbortMergeConfirmed => {
            let Some(operation_id) = ctx
                .data
                .operations
                .begin_write_kind(OperationKind::AbortMerge)
            else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                git_write_work(move || repo.abort_merge().map(|s| presenter.project_loaded(s))),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DirtyMergeAborted {
                            operation_id,
                            result,
                        },
                    )
                },
            )
        }
        DetailAction::CommitFileClicked { commit_idx, path } => {
            if diff_panel.is_active() {
                let already_shown = diff_panel
                    .commit_file_diff
                    .as_ref()
                    .is_some_and(|d| d.commit_idx == commit_idx && d.file_path == path);
                if already_shown {
                    diff_panel.close();
                    return repository_panel.restore_center_list_scroll();
                }
            }
            let Some(commit) = ctx.data.snapshot.commits().get(commit_idx) else {
                return Task::none();
            };
            let commit_files = ctx
                .data
                .cache
                .state(commit_idx)
                .map(|d| d.files.as_slice())
                .unwrap_or_default();

            let selected_idx = commit_files
                .iter()
                .enumerate()
                .find(|(_, f)| f.path == path)
                .map(|(i, _)| i)
                .unwrap_or(0);

            let commit_hash = commit.hash.clone();
            ctx.data.commit_search = None;
            let follow_up =
                diff_panel.open_commit_file(commit_idx, commit_hash, path, selected_idx);
            update_diff(diff_panel, follow_up, ctx)
        }
        DetailAction::CloseDirtyFileDiff => {
            diff_panel.close();
            repository_panel.restore_center_list_scroll()
        }
        DetailAction::MergedFileClicked { path } => {
            if diff_panel.is_active() {
                let already_shown = diff_panel
                    .merged_file_diff
                    .as_ref()
                    .is_some_and(|d| d.file_path == path);
                if already_shown {
                    diff_panel.close();
                    return repository_panel.restore_center_list_scroll();
                }
            }
            let Some(merged) = ctx.merged_diff.result() else {
                return Task::none();
            };
            let selected_idx = merged
                .files
                .iter()
                .enumerate()
                .find(|(_, f)| f.path == path)
                .map(|(i, _)| i)
                .unwrap_or(0);

            let hashes = merged.hashes.clone();
            ctx.data.commit_search = None;
            let follow_up = diff_panel.open_merged_file(hashes, path, selected_idx);
            update_diff(diff_panel, follow_up, ctx)
        }
        DetailAction::CommitMessageAction(action) => {
            panel.dirty_commit_message.perform(action);
            Task::none()
        }
        DetailAction::RewordStarted {
            hash,
            original_message,
        } => {
            panel.start_reword(hash, original_message);
            Task::none()
        }
        DetailAction::RewordCanceled => {
            panel.cancel_reword();
            Task::none()
        }
        DetailAction::RewordMessageAction(action) => {
            panel.perform_reword_action(action);
            Task::none()
        }
        DetailAction::RewordConfirmed => {
            let Some((hash, message)) = panel.reword_active() else {
                return Task::none();
            };
            if message.is_empty() {
                return Task::none();
            }
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let hash = hash.to_string();
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                git_write_work(move || {
                    repo.reword_commit(hash, message)
                        .map(|s| presenter.project_loaded(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::RewordCompleted {
                            operation_id: Some(operation_id),
                            result,
                        },
                    )
                },
            )
        }
        DetailAction::CopyCommitShaRequested(hash) => {
            panel.set_copied_sha_flash();
            let tab_id = ctx.tab_id;
            let clipboard_task = clipboard::write::<Message>(hash);
            let delay = std::time::Duration::from_millis(1000);
            let clear_task = Task::perform(timer_work(delay), move |_| {
                Message::tab(
                    tab_id,
                    RepositoryMessage::Detail(DetailAction::ClearCopyShaFlash),
                )
            });
            Task::batch(vec![clipboard_task, clear_task])
        }
        DetailAction::ClearCopyShaFlash => {
            panel.clear_copied_sha_flash();
            Task::none()
        }
        DetailAction::ParentCommitPressed(hash) => {
            ctx.data.pending_focus = Some(PendingFocus::Commit { hash });
            try_resolve_pending_focus(ctx, repository_panel).unwrap_or(Task::none())
        }
        DetailAction::None => Task::none(),
    }
}

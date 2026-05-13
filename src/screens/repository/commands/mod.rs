//! Git-operation result dispatch. Every non-panel `RepositoryMessage` variant
//! lands here. Each family of operations has its own submodule (mirrors the
//! segregated gateway traits; this file owns the
//! top-level routing that maps an incoming `RepositoryMessage` to the right
//! handler.
//!
//! Panel-scoped messages (`Sidebar`, `Center`, `Detail`, `DiffPanel`,
//! `CommitSearch`, `OverlayPanel`) are intentionally NOT routed through here —
//! those go straight to their panel's `update.rs` from `mod.rs`, keeping
//! panel lifecycles self-contained.

use iced::Task;

use crate::{message::Message, work::git_read_work};

use super::RepositoryMessage;
use super::RepositoryScreen;

mod branch_ops;
mod commit_ops;
pub(in crate::screens::repository) mod helpers;
mod loaders;
mod remote_ops;
mod stash_ops;
mod tag_ops;
mod worktree_ops;

/// Swap the active gateway in the fleet to the given worktree path and load
/// the repo from it. Called from both the sidebar double-click handler and the
/// graph branch-label click handler when the label belongs to a non-primary
/// worktree branch.
pub(in crate::screens::repository) fn focus_swap_to_worktree(
    ctx: &mut crate::screens::repository::panels::ScreenCtx<'_>,
    path: std::path::PathBuf,
) -> Task<Message> {
    use crate::services::COMMIT_LOAD_LIMIT;

    if let Err(e) = ctx.fleet.ensure(path.clone()) {
        eprintln!("git_leviathan: ensure gateway failed: {e}");
        return Task::done(Message::show_toast(crate::toast::ToastData::error(
            "Focus swap failed",
            format!("ensure gateway failed: {e}"),
        )));
    }
    if let Err(e) = ctx.fleet.swap_active(path.clone()) {
        eprintln!("git_leviathan: swap active failed: {e}");
        return Task::done(Message::show_toast(crate::toast::ToastData::error(
            "Focus swap failed",
            format!("swap active failed: {e}"),
        )));
    }
    let repo = ctx.fleet.active().clone();
    let presenter = ctx.presenter.clone();
    let tab_id = ctx.tab_id;
    let repo_path = ctx.fleet.active_path().display().to_string();
    Task::perform(
        git_read_work(move || {
            let _span = crate::perf::Span::new("git.full_repo_load")
                .field("tab", tab_id)
                .field("repo", &repo_path)
                .field("limit", COMMIT_LOAD_LIMIT);
            repo.load_repo(COMMIT_LOAD_LIMIT)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| Message::tab(tab_id, RepositoryMessage::WorktreeFocusSwapped(result)),
    )
}

/// Dispatches a non-panel `RepositoryMessage` to its category handler.
pub(in crate::screens::repository) fn dispatch_result(
    screen: &mut RepositoryScreen,
    msg: RepositoryMessage,
) -> Task<Message> {
    match msg {
        // Panel messages are routed in mod.rs and never reach here.
        RepositoryMessage::Sidebar(_)
        | RepositoryMessage::Center(_)
        | RepositoryMessage::Detail(_)
        | RepositoryMessage::DiffPanel(_)
        | RepositoryMessage::CommitSearch(_)
        | RepositoryMessage::OpenCommitSearch
        | RepositoryMessage::OverlayPanel(_)
        | RepositoryMessage::FetchRequested
        | RepositoryMessage::RefreshRequested
        | RepositoryMessage::CreateBranchAtSelected { .. }
        | RepositoryMessage::CopyCommitHash { .. }
        | RepositoryMessage::OpenSelectedDiff
        | RepositoryMessage::MoveSelection { .. }
        | RepositoryMessage::ClearMultiSelection
        | RepositoryMessage::EscapePressed
        | RepositoryMessage::FocusCommitMessage
        | RepositoryMessage::StartRewordSelected
        | RepositoryMessage::FocusRewordMessage
        | RepositoryMessage::FocusPanel(_)
        | RepositoryMessage::StageSelectedDirtyFile
        | RepositoryMessage::UnstageSelectedDirtyFile
        | RepositoryMessage::DiscardSelectedDirtyFile
        | RepositoryMessage::DeleteBranchDirect { .. }
        | RepositoryMessage::DeleteBranchLocalAndRemote { .. } => {
            unreachable!("repository UI messages routed in mod.rs")
        }

        RepositoryMessage::RepoLoaded(result) => loaders::on_repo_loaded(screen, result),
        RepositoryMessage::WriteRepoLoaded {
            operation_id,
            result,
        } => loaders::on_write_repo_loaded(screen, operation_id, result),
        RepositoryMessage::RefsReloaded(result) => loaders::on_refs_reloaded(screen, result),
        RepositoryMessage::FetchFinished {
            operation_id,
            result,
        } => loaders::on_fetch_finished(screen, operation_id, result),
        RepositoryMessage::GraphAndRefsReloaded(result) => {
            loaders::on_graph_and_refs_reloaded(screen, result)
        }
        RepositoryMessage::MoreCommitsLoaded {
            repo_version,
            result,
        } => loaders::on_more_commits_loaded(screen, repo_version, result),
        RepositoryMessage::CommitDiffLoaded(result) => {
            loaders::on_commit_diff_loaded(screen, result)
        }
        RepositoryMessage::MergedCommitDiffLoaded { version, result } => {
            loaders::on_merged_commit_diff_loaded(screen, version, result)
        }
        RepositoryMessage::MergedCommitFileDiffLoaded { generation, result } => {
            loaders::on_merged_commit_file_diff_loaded(screen, generation, result)
        }
        RepositoryMessage::CommitFileDiffLoaded { generation, result } => {
            loaders::on_commit_file_diff_loaded(screen, generation, result)
        }
        RepositoryMessage::DirtyFileDiffLoaded { generation, result } => {
            loaders::on_dirty_file_diff_loaded(screen, generation, result)
        }
        RepositoryMessage::DirtyDiffSyncChecked(result) => {
            loaders::on_dirty_diff_sync_checked(screen, result)
        }
        RepositoryMessage::ConflictResolutionLoaded { generation, result } => {
            loaders::on_conflict_resolution_loaded(screen, generation, result)
        }

        RepositoryMessage::RemoteCheckoutCompleted {
            operation_id,
            result,
        } => branch_ops::on_remote_checkout_completed(screen, operation_id, result),
        RepositoryMessage::BranchDeleted {
            operation_id,
            branch_name,
            is_remote,
            result,
        } => branch_ops::on_branch_deleted(screen, operation_id, branch_name, is_remote, result),
        RepositoryMessage::BranchRenamed {
            operation_id,
            old_name,
            new_name,
            is_remote,
            result,
        } => branch_ops::on_branch_renamed(
            screen,
            operation_id,
            old_name,
            new_name,
            is_remote,
            result,
        ),
        RepositoryMessage::BranchCreated {
            operation_id,
            branch_name,
            result,
        } => branch_ops::on_branch_created(screen, operation_id, branch_name, result),
        RepositoryMessage::BranchMerged {
            operation_id,
            source_branch,
            target_branch,
            result,
        } => {
            branch_ops::on_branch_merged(screen, operation_id, source_branch, target_branch, result)
        }
        RepositoryMessage::BranchRebased {
            operation_id,
            source_branch,
            target_display,
            result,
        } => branch_ops::on_branch_rebased(
            screen,
            operation_id,
            source_branch,
            target_display,
            result,
        ),

        RepositoryMessage::DirtyCommitCreated {
            operation_id,
            result,
        } => commit_ops::on_dirty_commit_created(screen, operation_id, result),
        RepositoryMessage::DirtyMergeAborted {
            operation_id,
            result,
        } => commit_ops::on_dirty_merge_aborted(screen, operation_id, result),
        RepositoryMessage::DirtyIndexChanged {
            operation_id,
            result,
        } => commit_ops::on_dirty_index_changed(screen, operation_id, result),
        RepositoryMessage::DirtyIndexReloaded {
            operation_id,
            result,
        } => commit_ops::on_dirty_index_reloaded(screen, operation_id, result),
        RepositoryMessage::CherryPickCompleted {
            operation_id,
            result,
        } => commit_ops::on_cherry_pick_completed(screen, operation_id, result),
        RepositoryMessage::RevertCompleted {
            operation_id,
            result,
        } => commit_ops::on_revert_completed(screen, operation_id, result),
        RepositoryMessage::ConflictResolutionSaved {
            operation_id,
            result,
        } => commit_ops::on_conflict_resolution_saved(screen, operation_id, result),
        RepositoryMessage::SquashCompleted {
            operation_id,
            result,
        } => commit_ops::on_squash_completed(screen, operation_id, result),
        RepositoryMessage::RewordCompleted {
            operation_id,
            result,
        } => commit_ops::on_reword_completed(screen, operation_id, result),

        RepositoryMessage::StashApplyCompleted {
            operation_id,
            result,
        } => stash_ops::on_stash_apply_completed(screen, operation_id, result),
        RepositoryMessage::StashPopCompleted {
            operation_id,
            result,
        } => stash_ops::on_stash_pop_completed(screen, operation_id, result),

        RepositoryMessage::TagCreated {
            operation_id,
            tag_name,
            result,
        } => tag_ops::on_tag_created(screen, operation_id, tag_name, result),
        RepositoryMessage::TagDeleted {
            operation_id,
            tag_name,
            result,
        } => tag_ops::on_tag_deleted(screen, operation_id, tag_name, result),
        RepositoryMessage::TagPushed {
            operation_id,
            tag_name,
            remote_name,
            result,
        } => tag_ops::on_tag_pushed(screen, operation_id, tag_name, remote_name, result),
        RepositoryMessage::TagDeletedFromRemote {
            operation_id,
            tag_name,
            remote_name,
            result,
        } => {
            tag_ops::on_tag_deleted_from_remote(screen, operation_id, tag_name, remote_name, result)
        }

        RepositoryMessage::RemoteAdded {
            operation_id,
            result,
        } => remote_ops::on_remote_added(screen, operation_id, result),
        RepositoryMessage::WorktreeCreated {
            operation_id,
            result,
        } => worktree_ops::on_worktree_created(screen, operation_id, result),
        RepositoryMessage::WorktreeFocusSwapped(result) => {
            on_worktree_focus_swapped(screen, result)
        }
        RepositoryMessage::WorktreeRemoved {
            operation_id,
            result,
        } => worktree_ops::on_worktree_removed(screen, operation_id, result),
        RepositoryMessage::PushRequested => remote_ops::on_push_requested(screen),
        RepositoryMessage::PushCompleted {
            operation_id,
            result,
        } => remote_ops::on_push_completed(screen, operation_id, result),
        RepositoryMessage::SetUpstreamPushCompleted {
            operation_id,
            result,
        } => remote_ops::on_set_upstream_push_completed(screen, operation_id, result),
        RepositoryMessage::ForcePushCompleted {
            operation_id,
            result,
        } => remote_ops::on_force_push_completed(screen, operation_id, result),
        RepositoryMessage::PullRequested => remote_ops::on_pull_requested(screen),
        RepositoryMessage::PullCompleted {
            operation_id,
            result,
        } => remote_ops::on_pull_completed(screen, operation_id, result),
    }
}

fn on_worktree_focus_swapped(
    screen: &mut RepositoryScreen,
    result: Result<crate::view_model::LoadedRepo, crate::services::GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            let apply_task = helpers::handle_repo_loaded(screen, loaded);
            let toast_task = Task::done(Message::show_toast(crate::toast::ToastData::success(
                "Focused worktree",
                String::new(),
            )));
            Task::batch(vec![apply_task, toast_task])
        }
        Err(e) => {
            eprintln!("git_leviathan: focus swap failed: {e}");
            Task::done(Message::show_toast(crate::toast::ToastData::error(
                "Failed to focus worktree",
                e.to_string(),
            )))
        }
    }
}

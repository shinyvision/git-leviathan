//! Git-operation result dispatch. Every non-panel `RepositoryMessage` variant
//! lands here; each family of operations has its own submodule mirroring the
//! segregated gateway traits.
//!
//! Panel-scoped messages (`Sidebar`, `Center`, `Detail`, `DiffPanel`,
//! `CommitSearch`, `OverlayPanel`) are intentionally NOT routed through here —
//! those go straight to their panel's `update.rs` from `mod.rs`, keeping
//! panel lifecycles self-contained.

use iced::Task;

use crate::message::Message;

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
        return Task::done(Message::show_toast(
            crate::toast::ToastData::error("Focus swap failed", format!("ensure gateway failed: {e}")),
        ));
    }
    if let Err(e) = ctx.fleet.swap_active(path.clone()) {
        eprintln!("git_leviathan: swap active failed: {e}");
        return Task::done(Message::show_toast(
            crate::toast::ToastData::error("Focus swap failed", format!("swap active failed: {e}")),
        ));
    }
    let repo = ctx.fleet.active().clone();
    let presenter = ctx.presenter.clone();
    let tab_id = ctx.tab_id;
    Task::perform(
        super::gateway_work(move || {
            repo.load_repo(COMMIT_LOAD_LIMIT)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| Message::tab(tab_id, RepositoryMessage::WorktreeFocusSwapped(result)),
    )
}

pub(in crate::screens::repository) fn dispatch_result(
    screen: &mut RepositoryScreen,
    msg: RepositoryMessage,
) -> Task<Message> {
    match msg {
        RepositoryMessage::Sidebar(_)
        | RepositoryMessage::Center(_)
        | RepositoryMessage::Detail(_)
        | RepositoryMessage::DiffPanel(_)
        | RepositoryMessage::CommitSearch(_)
        | RepositoryMessage::OpenCommitSearch
        | RepositoryMessage::OverlayPanel(_) => unreachable!("panel messages routed in mod.rs"),

        RepositoryMessage::RepoLoaded(result) => loaders::on_repo_loaded(screen, result),
        RepositoryMessage::RefsReloaded(result) => loaders::on_refs_reloaded(screen, result),
        RepositoryMessage::FetchFinished(result) => loaders::on_fetch_finished(screen, result),
        RepositoryMessage::GraphAndRefsReloaded(result) => {
            loaders::on_graph_and_refs_reloaded(screen, result)
        }
        RepositoryMessage::MoreCommitsLoaded {
            repo_version,
            result,
        } => loaders::on_more_commits_loaded(screen, repo_version, result),
        RepositoryMessage::CommitDiffLoaded(result) => loaders::on_commit_diff_loaded(screen, result),
        RepositoryMessage::MergedCommitDiffLoaded { version, result } => {
            loaders::on_merged_commit_diff_loaded(screen, version, result)
        }
        RepositoryMessage::MergedCommitFileDiffLoaded(result) => {
            loaders::on_merged_commit_file_diff_loaded(screen, result)
        }
        RepositoryMessage::CommitFileDiffLoaded(result) => {
            loaders::on_commit_file_diff_loaded(screen, result)
        }
        RepositoryMessage::DirtyFileDiffLoaded(result) => {
            loaders::on_dirty_file_diff_loaded(screen, result)
        }
        RepositoryMessage::ConflictResolutionLoaded(result) => {
            loaders::on_conflict_resolution_loaded(screen, result)
        }

        RepositoryMessage::RemoteCheckoutCompleted(result) => {
            branch_ops::on_remote_checkout_completed(screen, result)
        }
        RepositoryMessage::BranchDeleted {
            branch_name,
            is_remote,
            result,
        } => branch_ops::on_branch_deleted(screen, branch_name, is_remote, result),
        RepositoryMessage::BranchRenamed {
            old_name,
            new_name,
            is_remote,
            result,
        } => branch_ops::on_branch_renamed(screen, old_name, new_name, is_remote, result),
        RepositoryMessage::BranchCreated {
            branch_name,
            result,
        } => branch_ops::on_branch_created(screen, branch_name, result),
        RepositoryMessage::BranchMerged {
            source_branch,
            target_branch,
            result,
        } => branch_ops::on_branch_merged(screen, source_branch, target_branch, result),
        RepositoryMessage::BranchRebased {
            source_branch,
            target_display,
            result,
        } => branch_ops::on_branch_rebased(screen, source_branch, target_display, result),

        RepositoryMessage::DirtyCommitCreated(result) => {
            commit_ops::on_dirty_commit_created(screen, result)
        }
        RepositoryMessage::DirtyMergeAborted(result) => {
            commit_ops::on_dirty_merge_aborted(screen, result)
        }
        RepositoryMessage::DirtyIndexChanged(result) => {
            commit_ops::on_dirty_index_changed(screen, result)
        }
        RepositoryMessage::CherryPickCompleted(result) => {
            commit_ops::on_cherry_pick_completed(screen, result)
        }
        RepositoryMessage::ConflictResolutionSaved(result) => {
            commit_ops::on_conflict_resolution_saved(screen, result)
        }
        RepositoryMessage::SquashCompleted(result) => {
            commit_ops::on_squash_completed(screen, result)
        }
        RepositoryMessage::RewordCompleted(result) => {
            commit_ops::on_reword_completed(screen, result)
        }

        RepositoryMessage::StashApplyCompleted(result) => {
            stash_ops::on_stash_apply_completed(screen, result)
        }
        RepositoryMessage::StashPopCompleted(result) => {
            stash_ops::on_stash_pop_completed(screen, result)
        }

        RepositoryMessage::TagCreated { tag_name, result } => {
            tag_ops::on_tag_created(screen, tag_name, result)
        }
        RepositoryMessage::TagDeleted { tag_name, result } => {
            tag_ops::on_tag_deleted(screen, tag_name, result)
        }
        RepositoryMessage::TagPushed {
            tag_name,
            remote_name,
            result,
        } => tag_ops::on_tag_pushed(screen, tag_name, remote_name, result),
        RepositoryMessage::TagDeletedFromRemote {
            tag_name,
            remote_name,
            result,
        } => tag_ops::on_tag_deleted_from_remote(screen, tag_name, remote_name, result),

        RepositoryMessage::RemoteAdded(result) => remote_ops::on_remote_added(screen, result),
        RepositoryMessage::WorktreeCreated(result) => {
            worktree_ops::on_worktree_created(screen, result)
        }
        RepositoryMessage::WorktreeFocusSwapped(result) => {
            on_worktree_focus_swapped(screen, result)
        }
        RepositoryMessage::WorktreeRemoved(result) => {
            worktree_ops::on_worktree_removed(screen, result)
        }
        RepositoryMessage::PushRequested => remote_ops::on_push_requested(screen),
        RepositoryMessage::PushCompleted(result) => remote_ops::on_push_completed(screen, result),
        RepositoryMessage::SetUpstreamPushCompleted(result) => {
            remote_ops::on_set_upstream_push_completed(screen, result)
        }
        RepositoryMessage::ForcePushCompleted(result) => {
            remote_ops::on_force_push_completed(screen, result)
        }
        RepositoryMessage::PullRequested => remote_ops::on_pull_requested(screen),
        RepositoryMessage::PullCompleted(result) => remote_ops::on_pull_completed(screen, result),
    }
}

fn on_worktree_focus_swapped(
    screen: &mut RepositoryScreen,
    result: Result<crate::view_model::LoadedRepo, crate::services::GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            let apply_task = helpers::handle_repo_loaded(screen, loaded);
            let toast_task = Task::done(Message::show_toast(
                crate::toast::ToastData::success("Focused worktree", String::new()),
            ));
            Task::batch(vec![apply_task, toast_task])
        }
        Err(e) => {
            eprintln!("git_leviathan: focus swap failed: {e}");
            Task::done(Message::show_toast(
                crate::toast::ToastData::error("Failed to focus worktree", e.to_string()),
            ))
        }
    }
}

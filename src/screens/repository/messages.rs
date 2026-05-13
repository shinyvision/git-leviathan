//! `RepositoryMessage` — the top-level message variant for the repository
//! screen. Panel messages wrap per-panel action enums; the remaining arms
//! carry git-operation results that flow back from the async task graph.

use crate::{
    core::RepoVersion,
    services::{
        CommitDiffResult, ConflictResolutionResult, GitError, MergedCommitDiffResult,
        WorkingTreeDiffResult,
    },
    view_model::{
        LoadedBranchMergeOutcome, LoadedCherryPickOutcome, LoadedDirtyIndex, LoadedPushOutcome,
        LoadedRefs, LoadedRemoteCheckoutOutcome, LoadedRepo, LoadedRevertOutcome,
        LoadedStashApplyOutcome,
    },
};

use super::commit_search::CommitSearchMessage;
use super::panel_messages::{CenterAction, DetailAction, DiffPanelAction, OverlayPanelAction};
use super::panels::diff::DirtyDiffSyncResult;
use super::panels::sidebar::SidebarAction;
use super::state::{FocusedPanel, OperationId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitWriteIntent {
    FastLocal,
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryFocusTarget {
    Sidebar,
    Center,
    Graph,
    Details,
    Diff,
}

impl RepositoryFocusTarget {
    pub(crate) fn resolve(self, diff_active: bool) -> Result<FocusedPanel, &'static str> {
        match self {
            Self::Sidebar => Ok(FocusedPanel::Sidebar),
            Self::Details => Ok(FocusedPanel::Detail),
            Self::Center => Ok(FocusedPanel::Center),
            Self::Graph => {
                if diff_active {
                    Err("graph is not visible while diff is active")
                } else {
                    Ok(FocusedPanel::Center)
                }
            }
            Self::Diff => {
                if diff_active {
                    Ok(FocusedPanel::Center)
                } else {
                    Err("diff is not visible")
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum RepositoryMessage {
    // Panel actions
    Sidebar(SidebarAction),
    Center(CenterAction),
    Detail(DetailAction),
    DiffPanel(DiffPanelAction),
    OverlayPanel(OverlayPanelAction),
    FetchRequested,
    RefreshRequested,
    CreateBranchAtSelected {
        commit_idx: Option<usize>,
        hash: Option<String>,
    },
    CopyCommitHash {
        hash: Option<String>,
    },
    OpenSelectedDiff,
    MoveSelection {
        action: CenterAction,
        extend: bool,
    },
    ClearMultiSelection,
    EscapePressed,
    FocusCommitMessage,
    StartRewordSelected,
    FocusRewordMessage,
    FocusPanel(RepositoryFocusTarget),
    StageSelectedDirtyFile,
    UnstageSelectedDirtyFile,
    DiscardSelectedDirtyFile,
    DeleteBranchDirect {
        branch_name: String,
        is_remote: bool,
        remote_ref: Option<String>,
    },
    DeleteBranchLocalAndRemote {
        branch_name: String,
    },

    // Git operation results — all snapshots are pre-projected off the main
    // thread (see `project_loaded`) so the UI can swap without running the
    // presenter under the winit event loop.
    RepoLoaded(Result<LoadedRepo, GitError>),
    WriteRepoLoaded {
        operation_id: OperationId,
        result: Result<LoadedRepo, GitError>,
    },
    RefsReloaded(Result<LoadedRepo, GitError>),
    /// Network fetch finished. Carries no repository data; the handler decides
    /// which components need fresh data and dispatches scoped reload tasks.
    FetchFinished {
        operation_id: OperationId,
        result: Result<(), GitError>,
    },
    /// Narrow refs-only payload destined for the graph + sidebar components.
    /// Addressed to those components by construction — the message name and
    /// payload type intentionally exclude everything else.
    GraphAndRefsReloaded(Result<LoadedRefs, GitError>),
    MoreCommitsLoaded {
        repo_version: RepoVersion,
        result: Result<LoadedRepo, GitError>,
    },
    CommitDiffLoaded(Result<CommitDiffResult, GitError>),
    MergedCommitDiffLoaded {
        version: RepoVersion,
        result: Result<MergedCommitDiffResult, GitError>,
    },
    MergedCommitFileDiffLoaded {
        generation: u64,
        result: Result<WorkingTreeDiffResult, GitError>,
    },
    RemoteCheckoutCompleted {
        operation_id: OperationId,
        result: Result<LoadedRemoteCheckoutOutcome, GitError>,
    },
    BranchDeleted {
        operation_id: Option<OperationId>,
        branch_name: String,
        is_remote: bool,
        result: Result<LoadedRepo, GitError>,
    },
    BranchRenamed {
        operation_id: Option<OperationId>,
        old_name: String,
        new_name: String,
        is_remote: bool,
        result: Result<LoadedRepo, GitError>,
    },
    BranchCreated {
        operation_id: Option<OperationId>,
        branch_name: String,
        result: Result<LoadedRepo, GitError>,
    },
    BranchMerged {
        operation_id: Option<OperationId>,
        source_branch: String,
        target_branch: String,
        result: Result<LoadedBranchMergeOutcome, GitError>,
    },
    BranchRebased {
        operation_id: Option<OperationId>,
        source_branch: String,
        target_display: String,
        result: Result<LoadedRepo, GitError>,
    },
    DirtyCommitCreated {
        operation_id: OperationId,
        result: Result<LoadedRepo, GitError>,
    },
    DirtyMergeAborted {
        operation_id: OperationId,
        result: Result<LoadedRepo, GitError>,
    },
    DirtyIndexChanged {
        operation_id: Option<OperationId>,
        result: Result<LoadedRepo, GitError>,
    },
    DirtyIndexReloaded {
        operation_id: OperationId,
        result: Result<LoadedDirtyIndex, GitError>,
    },
    CherryPickCompleted {
        operation_id: Option<OperationId>,
        result: Result<LoadedCherryPickOutcome, GitError>,
    },
    RevertCompleted {
        operation_id: Option<OperationId>,
        result: Result<LoadedRevertOutcome, GitError>,
    },
    StashApplyCompleted {
        operation_id: Option<OperationId>,
        result: Result<LoadedStashApplyOutcome, GitError>,
    },
    StashPopCompleted {
        operation_id: Option<OperationId>,
        result: Result<LoadedStashApplyOutcome, GitError>,
    },
    ConflictResolutionSaved {
        operation_id: OperationId,
        result: Result<LoadedRepo, GitError>,
    },
    CommitFileDiffLoaded {
        generation: u64,
        result: Result<WorkingTreeDiffResult, GitError>,
    },
    DirtyFileDiffLoaded {
        generation: u64,
        result: Result<WorkingTreeDiffResult, GitError>,
    },
    DirtyDiffSyncChecked(Result<DirtyDiffSyncResult, GitError>),
    ConflictResolutionLoaded {
        generation: u64,
        result: Result<ConflictResolutionResult, GitError>,
    },
    RemoteAdded {
        operation_id: Option<OperationId>,
        result: Result<LoadedRepo, GitError>,
    },
    WorktreeCreated {
        operation_id: Option<OperationId>,
        result: Result<LoadedRepo, GitError>,
    },
    WorktreeFocusSwapped(Result<LoadedRepo, GitError>),
    WorktreeRemoved {
        operation_id: Option<OperationId>,
        result: Result<LoadedRepo, GitError>,
    },
    PushRequested,
    PushCompleted {
        operation_id: OperationId,
        result: Result<LoadedPushOutcome, GitError>,
    },
    SetUpstreamPushCompleted {
        operation_id: Option<OperationId>,
        result: Result<LoadedRepo, GitError>,
    },
    ForcePushCompleted {
        operation_id: Option<OperationId>,
        result: Result<LoadedPushOutcome, GitError>,
    },
    PullRequested,
    PullCompleted {
        operation_id: Option<OperationId>,
        result: Result<LoadedRepo, GitError>,
    },
    SquashCompleted {
        operation_id: OperationId,
        result: Result<LoadedRepo, GitError>,
    },
    RewordCompleted {
        operation_id: Option<OperationId>,
        result: Result<LoadedRepo, GitError>,
    },
    TagCreated {
        operation_id: Option<OperationId>,
        tag_name: String,
        result: Result<LoadedRepo, GitError>,
    },
    TagDeleted {
        operation_id: Option<OperationId>,
        tag_name: String,
        result: Result<LoadedRepo, GitError>,
    },
    TagPushed {
        operation_id: OperationId,
        tag_name: String,
        remote_name: String,
        result: Result<LoadedRepo, GitError>,
    },
    TagDeletedFromRemote {
        operation_id: Option<OperationId>,
        tag_name: String,
        remote_name: String,
        result: Result<LoadedRepo, GitError>,
    },

    /// Commit search overlay (Ctrl+F over the graph view).
    CommitSearch(CommitSearchMessage),
    /// Open commit search overlay (from toolbar search button).
    OpenCommitSearch,
}

impl RepositoryMessage {
    pub(crate) fn git_write_intent(&self) -> Option<GitWriteIntent> {
        match self {
            Self::Sidebar(SidebarAction::BranchPressed { .. }) => Some(GitWriteIntent::Normal),
            Self::Center(action) => center_write_intent(action),
            Self::Detail(action) => detail_write_intent(action),
            Self::DiffPanel(DiffPanelAction::ConflictResolutionSaveRequested) => {
                Some(GitWriteIntent::Normal)
            }
            Self::OverlayPanel(action) => overlay_write_intent(action),
            Self::StageSelectedDirtyFile | Self::UnstageSelectedDirtyFile => {
                Some(GitWriteIntent::FastLocal)
            }
            Self::DeleteBranchDirect { .. } | Self::DeleteBranchLocalAndRemote { .. } => {
                Some(GitWriteIntent::Normal)
            }
            Self::PushRequested | Self::PullRequested => Some(GitWriteIntent::Normal),
            _ => None,
        }
    }
}

fn center_write_intent(action: &CenterAction) -> Option<GitWriteIntent> {
    match action {
        CenterAction::BranchLabelClicked { .. }
        | CenterAction::BranchLabelPressed(_)
        | CenterAction::RemoteBranchLabelPressed { .. }
        | CenterAction::BranchMergeRequested { .. }
        | CenterAction::BranchFastForwardRequested { .. }
        | CenterAction::BranchRebaseRequested { .. }
        | CenterAction::ResetToCommitRequested { .. }
        | CenterAction::StashCreateRequested
        | CenterAction::StashApplyRequested { .. }
        | CenterAction::StashPopRequested { .. }
        | CenterAction::SquashCommitsRequested { .. }
        | CenterAction::PushTagRequested { .. } => Some(GitWriteIntent::Normal),
        _ => None,
    }
}

fn detail_write_intent(action: &DetailAction) -> Option<GitWriteIntent> {
    match action {
        DetailAction::StageFile(_)
        | DetailAction::StageAll
        | DetailAction::StageSelectedFiles
        | DetailAction::UnstageFile(_)
        | DetailAction::UnstageAll
        | DetailAction::UnstageSelectedFiles
        | DetailAction::CommitConfirmed => Some(GitWriteIntent::FastLocal),
        DetailAction::MarkConflictResolved(_)
        | DetailAction::MarkAllConflictsResolved
        | DetailAction::AbortMergeConfirmed
        | DetailAction::RewordConfirmed => Some(GitWriteIntent::Normal),
        _ => None,
    }
}

fn overlay_write_intent(action: &OverlayPanelAction) -> Option<GitWriteIntent> {
    match action {
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::force_push::is_confirm_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::stash_delete::is_confirm_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::delete_tag::is_confirm_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::cherry_pick_confirm::is_write_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::revert_confirm::is_write_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::remove_worktree::is_confirm_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::discard::is_confirm_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::delete_branch::is_write_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::rename_branch::is_confirm_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::create_branch::is_confirm_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::conflict_checkout::is_write_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::set_upstream::is_confirm_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::push_behind::is_write_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::create_tag::is_confirm_button_action(dialog_id, button_id) => {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::DialogButtonPressed {
            dialog_id,
            button_id,
        } if super::overlays::modify_delete_conflict::is_write_button_action(
            dialog_id, button_id,
        ) =>
        {
            Some(GitWriteIntent::Normal)
        }
        OverlayPanelAction::AddRemoteConfirmed | OverlayPanelAction::CreateWorktreeConfirmed => {
            Some(GitWriteIntent::Normal)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::overlays::{
        cherry_pick_confirm, delete_tag,
        dialog::model::{DialogButtonId, DialogId},
        discard, force_push, remove_worktree, revert_confirm, stash_delete,
    };
    use super::*;

    #[test]
    fn selected_dirty_file_writes_use_git_queue() {
        assert_eq!(
            RepositoryMessage::StageSelectedDirtyFile.git_write_intent(),
            Some(GitWriteIntent::FastLocal)
        );
        assert_eq!(
            RepositoryMessage::UnstageSelectedDirtyFile.git_write_intent(),
            Some(GitWriteIntent::FastLocal)
        );
        assert_eq!(
            RepositoryMessage::DiscardSelectedDirtyFile.git_write_intent(),
            None
        );
    }

    #[test]
    fn force_push_dialog_confirm_button_uses_git_queue() {
        assert_eq!(
            RepositoryMessage::OverlayPanel(OverlayPanelAction::DialogButtonPressed {
                dialog_id: DialogId(force_push::DIALOG_ID.into()),
                button_id: DialogButtonId(force_push::CONFIRM_BUTTON_ID.into()),
            })
            .git_write_intent(),
            Some(GitWriteIntent::Normal)
        );
        assert_eq!(
            RepositoryMessage::OverlayPanel(OverlayPanelAction::DialogButtonPressed {
                dialog_id: DialogId(force_push::DIALOG_ID.into()),
                button_id: DialogButtonId(force_push::CANCEL_BUTTON_ID.into()),
            })
            .git_write_intent(),
            None
        );
    }

    #[test]
    fn migrated_dialog_confirm_buttons_use_git_queue() {
        let write_buttons = [
            (stash_delete::DIALOG_ID, stash_delete::CONFIRM_BUTTON_ID),
            (delete_tag::DIALOG_ID, delete_tag::CONFIRM_BUTTON_ID),
            (
                cherry_pick_confirm::DIALOG_ID,
                cherry_pick_confirm::IMMEDIATE_BUTTON_ID,
            ),
            (
                cherry_pick_confirm::DIALOG_ID,
                cherry_pick_confirm::STAGED_BUTTON_ID,
            ),
            (
                revert_confirm::DIALOG_ID,
                revert_confirm::IMMEDIATE_BUTTON_ID,
            ),
            (
                revert_confirm::DIALOG_ID,
                revert_confirm::IN_PLACE_BUTTON_ID,
            ),
            (
                remove_worktree::DIALOG_ID,
                remove_worktree::CONFIRM_BUTTON_ID,
            ),
            (discard::DIALOG_ID, discard::CONFIRM_BUTTON_ID),
        ];

        for (dialog_id, button_id) in write_buttons {
            assert_eq!(
                RepositoryMessage::OverlayPanel(OverlayPanelAction::DialogButtonPressed {
                    dialog_id: DialogId(dialog_id.into()),
                    button_id: DialogButtonId(button_id.into()),
                })
                .git_write_intent(),
                Some(GitWriteIntent::Normal)
            );
        }
    }

    #[test]
    fn repository_focus_target_resolves_against_visible_panels() {
        assert_eq!(
            RepositoryFocusTarget::Sidebar.resolve(false),
            Ok(FocusedPanel::Sidebar)
        );
        assert_eq!(
            RepositoryFocusTarget::Sidebar.resolve(true),
            Ok(FocusedPanel::Sidebar)
        );
        assert_eq!(
            RepositoryFocusTarget::Details.resolve(false),
            Ok(FocusedPanel::Detail)
        );
        assert_eq!(
            RepositoryFocusTarget::Details.resolve(true),
            Ok(FocusedPanel::Detail)
        );
        assert_eq!(
            RepositoryFocusTarget::Center.resolve(false),
            Ok(FocusedPanel::Center)
        );
        assert_eq!(
            RepositoryFocusTarget::Center.resolve(true),
            Ok(FocusedPanel::Center)
        );
        assert_eq!(
            RepositoryFocusTarget::Graph.resolve(false),
            Ok(FocusedPanel::Center)
        );
        assert!(RepositoryFocusTarget::Graph.resolve(true).is_err());
        assert_eq!(
            RepositoryFocusTarget::Diff.resolve(true),
            Ok(FocusedPanel::Center)
        );
        assert!(RepositoryFocusTarget::Diff.resolve(false).is_err());
    }
}

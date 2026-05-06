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
use super::state::OperationId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitWriteIntent {
    FastLocal,
    Normal,
}

#[derive(Debug, Clone)]
pub enum RepositoryMessage {
    // Panel actions
    Sidebar(SidebarAction),
    Center(CenterAction),
    Detail(DetailAction),
    DiffPanel(DiffPanelAction),
    OverlayPanel(OverlayPanelAction),

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
            Self::PushRequested | Self::PullRequested => Some(GitWriteIntent::Normal),
            _ => None,
        }
    }
}

fn center_write_intent(action: &CenterAction) -> Option<GitWriteIntent> {
    match action {
        CenterAction::BranchLabelClicked { .. }
        | CenterAction::BranchLabelPressed(_)
        | CenterAction::RemoteBranchLabelPressed(_)
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
        | DetailAction::UnstageFile(_)
        | DetailAction::UnstageAll
        | DetailAction::CommitConfirmed => Some(GitWriteIntent::FastLocal),
        DetailAction::MarkConflictResolved(_)
        | DetailAction::MarkAllConflictsResolved
        | DetailAction::DiscardConfirmed
        | DetailAction::AbortMergeConfirmed
        | DetailAction::RewordConfirmed => Some(GitWriteIntent::Normal),
        _ => None,
    }
}

fn overlay_write_intent(action: &OverlayPanelAction) -> Option<GitWriteIntent> {
    match action {
        OverlayPanelAction::ConflictCreateBranch
        | OverlayPanelAction::ConflictResetLocal
        | OverlayPanelAction::BranchDeleteConfirmed
        | OverlayPanelAction::BranchDeleteAllConfirmed
        | OverlayPanelAction::StashDeleteConfirmed
        | OverlayPanelAction::BranchRenameConfirmed
        | OverlayPanelAction::CreateBranchHereConfirmed
        | OverlayPanelAction::DiscardConfirmed
        | OverlayPanelAction::AddRemoteConfirmed
        | OverlayPanelAction::SetUpstreamConfirmed
        | OverlayPanelAction::PushBehindPullRequested
        | OverlayPanelAction::ForcePushConfirmed
        | OverlayPanelAction::CreateTagHereConfirmed
        | OverlayPanelAction::DeleteTagConfirmed
        | OverlayPanelAction::CherryPickImmediateConfirmed
        | OverlayPanelAction::CherryPickStagedConfirmed
        | OverlayPanelAction::RevertImmediateConfirmed
        | OverlayPanelAction::RevertInPlaceConfirmed
        | OverlayPanelAction::CreateWorktreeConfirmed
        | OverlayPanelAction::WorktreeRemoveConfirmed => Some(GitWriteIntent::Normal),
        _ => None,
    }
}

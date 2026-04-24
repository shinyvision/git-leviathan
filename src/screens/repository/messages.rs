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
        LoadedBranchMergeOutcome, LoadedCherryPickOutcome, LoadedPushOutcome, LoadedRefs,
        LoadedRemoteCheckoutOutcome, LoadedRepo, LoadedStashApplyOutcome,
    },
};

use super::commit_search::CommitSearchMessage;
use super::panel_messages::{CenterAction, DetailAction, DiffPanelAction, OverlayPanelAction};
use super::panels::sidebar::SidebarAction;

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
    RefsReloaded(Result<LoadedRepo, GitError>),
    /// Network fetch finished. Carries no repository data; the handler decides
    /// which components need fresh data and dispatches scoped reload tasks.
    FetchFinished(Result<(), GitError>),
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
    MergedCommitFileDiffLoaded(Result<WorkingTreeDiffResult, GitError>),
    RemoteCheckoutCompleted(Result<LoadedRemoteCheckoutOutcome, GitError>),
    BranchDeleted {
        branch_name: String,
        is_remote: bool,
        result: Result<LoadedRepo, GitError>,
    },
    BranchRenamed {
        old_name: String,
        new_name: String,
        is_remote: bool,
        result: Result<LoadedRepo, GitError>,
    },
    BranchCreated {
        branch_name: String,
        result: Result<LoadedRepo, GitError>,
    },
    BranchMerged {
        source_branch: String,
        target_branch: String,
        result: Result<LoadedBranchMergeOutcome, GitError>,
    },
    BranchRebased {
        source_branch: String,
        target_display: String,
        result: Result<LoadedRepo, GitError>,
    },
    DirtyCommitCreated(Result<LoadedRepo, GitError>),
    DirtyMergeAborted(Result<LoadedRepo, GitError>),
    DirtyIndexChanged(Result<LoadedRepo, GitError>),
    CherryPickCompleted(Result<LoadedCherryPickOutcome, GitError>),
    StashApplyCompleted(Result<LoadedStashApplyOutcome, GitError>),
    StashPopCompleted(Result<LoadedStashApplyOutcome, GitError>),
    ConflictResolutionSaved(Result<LoadedRepo, GitError>),
    CommitFileDiffLoaded(Result<WorkingTreeDiffResult, GitError>),
    DirtyFileDiffLoaded(Result<WorkingTreeDiffResult, GitError>),
    ConflictResolutionLoaded(Result<ConflictResolutionResult, GitError>),
    RemoteAdded(Result<LoadedRepo, GitError>),
    WorktreeCreated(Result<LoadedRepo, GitError>),
    WorktreeFocusSwapped(Result<LoadedRepo, GitError>),
    WorktreeRemoved(Result<LoadedRepo, GitError>),
    PushRequested,
    PushCompleted(Result<LoadedPushOutcome, GitError>),
    SetUpstreamPushCompleted(Result<LoadedRepo, GitError>),
    ForcePushCompleted(Result<LoadedPushOutcome, GitError>),
    PullRequested,
    PullCompleted(Result<LoadedRepo, GitError>),
    SquashCompleted(Result<LoadedRepo, GitError>),
    RewordCompleted(Result<LoadedRepo, GitError>),
    TagCreated {
        tag_name: String,
        result: Result<LoadedRepo, GitError>,
    },
    TagDeleted {
        tag_name: String,
        result: Result<LoadedRepo, GitError>,
    },
    TagPushed {
        tag_name: String,
        remote_name: String,
        result: Result<LoadedRepo, GitError>,
    },
    TagDeletedFromRemote {
        tag_name: String,
        remote_name: String,
        result: Result<LoadedRepo, GitError>,
    },

    /// Commit search overlay (Ctrl+F over the graph view).
    CommitSearch(CommitSearchMessage),
    /// Open commit search overlay (from toolbar search button).
    OpenCommitSearch,
}

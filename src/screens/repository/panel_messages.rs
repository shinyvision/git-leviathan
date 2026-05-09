//! Panel-specific message types for decoupled architecture.
//!
//! Each panel returns intentions via its message type.
//! `mod.rs` coordinates and creates Tasks based on these intentions.

#[derive(Debug, Clone)]
pub enum CenterAction {
    CommitSelected(usize),
    /// Tracked for multi-select clicks (Ctrl/Shift etc.).
    ModifiersChanged(iced::keyboard::Modifiers),
    NavigateUp,
    NavigateDown,
    /// Single click — double-click gate applies.
    BranchLabelClicked {
        branch_name: String,
        is_remote_only: bool,
        /// Exact remote ref for remote-only labels, e.g. "origin/feature".
        /// Existing service calls may still consume `branch_name` until the
        /// exact-ref methods are wired through.
        remote_ref: Option<String>,
    },
    /// Double-click confirmed — checkout.
    BranchLabelPressed(String),
    /// Double-click confirmed — checkout.
    RemoteBranchLabelPressed {
        branch_name: String,
        /// Exact remote ref selected by the UI, e.g. "origin/feature".
        remote_ref: Option<String>,
    },
    BranchLabelRightClicked {
        branch_name: String,
        is_remote: bool,
        has_remote: bool,
        is_tag: bool,
        remote_name: Option<String>,
        /// Exact remote ref for the remote side of this label, if known.
        remote_ref: Option<String>,
        /// Short branch name on the remote side, if it differs from
        /// `branch_name` (happens when local tracks a renamed remote).
        remote_branch_name: Option<String>,
    },
    BranchMergeRequested {
        source_branch: String,
        target_branch: String,
    },
    BranchFastForwardRequested {
        source_branch: String,
        target_branch: String,
    },
    /// Rebase the checked-out branch onto another ref.
    /// `target_ref` is what gets handed to `git rebase` — a local shorthand
    /// or `"<remote>/<branch>"` for remote targets. `target_display` is the
    /// human-readable label shown in toasts.
    BranchRebaseRequested {
        source_branch: String,
        target_ref: String,
        target_display: String,
        /// Exact remote ref for remote targets. Mirrors `target_ref` when the
        /// target is remote and is kept explicit for exact-ref service wiring.
        target_remote_ref: Option<String>,
    },
    /// "Reset ... to this commit" parent item clicked — opens submenu.
    ResetSubmenuRequested {
        commit_idx: usize,
        commit_hash: String,
    },
    ResetParentHoverChanged {
        commit_hash: String,
        hovered: bool,
    },
    ResetSubmenuHoverChanged(bool),
    /// Hover-open timer fired (opens submenu if parent still hovered).
    ResetOpenTimer(u64),
    /// Hover-close timer fired (closes submenu if neither hovered).
    ResetCloseTimer(u64),
    ResetToCommitRequested {
        commit_hash: String,
        mode: crate::services::ResetMode,
    },
    BranchDeleteRequested {
        branch_name: String,
        is_remote: bool,
        has_remote: bool,
        remote_name: Option<String>,
        /// Exact remote ref for remote deletes, e.g. "origin/feature".
        remote_ref: Option<String>,
    },
    BranchRenameRequested {
        branch_name: String,
        is_remote: bool,
        remote_name: Option<String>,
        /// Exact remote ref for remote renames, e.g. "origin/feature".
        remote_ref: Option<String>,
    },
    CommitRightClicked {
        commit_idx: usize,
    },
    CherryPickRequested {
        commit_hash: String,
    },
    RevertCommitRequested {
        commit_hash: String,
    },
    StashCreateRequested,
    StashApplyRequested {
        stash_index: usize,
    },
    /// Toolbar uses stash_index 0; commit context menu uses the actual index.
    StashPopRequested {
        stash_index: usize,
    },
    StashDeleteRequested {
        stash_index: usize,
        display_name: String,
    },
    CreateBranchHereRequested {
        commit_idx: usize,
        commit_hash: String,
    },
    CenterListScrolled(iced::widget::scrollable::Viewport),
    BranchLabelTriggerEntered(usize),
    BranchLabelTriggerBounds(Option<iced::Rectangle>),
    BranchLabelPopoutBounds(Option<iced::Rectangle>),
    BranchLabelContentBounds(Option<iced::Rectangle>),
    CopyBranchName(String),
    CloseContextMenu,
    PanelFocused(super::state::FocusedPanel),
    LoadMoreCommitsRequested,
    PointerMoved(iced::Point),
    PointerLeftWindow,
    DetailResizeStarted {
        effective_width: f32,
    },
    DetailHeightResizeStarted,
    GraphColumnResizeStarted {
        effective_width: f32,
        window_width: f32,
    },
    WindowResized {
        width: f32,
    },
    ResizeReleased,
    None,
    SquashCommitsRequested {
        indices: Vec<usize>,
        hashes: Vec<String>,
    },
    CreateTagHereRequested {
        commit_hash: String,
    },
    PushTagRequested {
        tag_name: String,
        remote_name: String,
    },
    DeleteTagRequested {
        tag_name: String,
        tag_remote_names: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub enum DetailAction {
    DirtyFileClicked {
        path: String,
        is_staged: bool,
    },
    DirtyFileRightClicked(String),
    StageFile(String),
    StageAll,
    UnstageFile(String),
    UnstageAll,
    MarkConflictResolved(String),
    MarkAllConflictsResolved,
    DiscardAllRequested,
    DiscardFileRequested(String),
    DiscardConfirmed,
    DiscardCanceled,
    CommitConfirmed,
    AbortMergeConfirmed,
    CommitFileClicked {
        commit_idx: usize,
        path: String,
    },
    /// File in the merged (multi-commit) diff.
    MergedFileClicked {
        path: String,
    },
    CloseDirtyFileDiff,
    CommitMessageAction(iced::widget::text_editor::Action),
    RewordStarted {
        hash: String,
        original_message: String,
    },
    RewordCanceled,
    RewordConfirmed,
    RewordMessageAction(iced::widget::text_editor::Action),
    /// Copy full hash to clipboard and flash tooltip.
    CopyCommitShaRequested(String),
    /// Parent-commit hash clicked — select that commit in the graph,
    /// scrolling/loading more commits as needed.
    ParentCommitPressed(String),
    /// Timer expired — remove the "Copied" flash tooltip.
    ClearCopyShaFlash,
    None,
}

#[derive(Debug, Clone)]
pub enum DiffPanelAction {
    /// Begin a text selection in one of the diff canvases. `canvas_id`
    /// identifies which buffer (diff, conflict ours/theirs/output).
    /// `viewport_rect` is the scrollable viewport in window coords —
    /// captured here so auto-scroll ticks can measure off-canvas cursor
    /// distance from the window-level cursor position without relying on
    /// clamped in-widget events.
    DiffSelectionBegin {
        canvas_id: crate::widgets::diff_canvas::DiffCanvasId,
        row: usize,
        col: usize,
        viewport_rect: iced::Rectangle,
        /// Canvas data captured at drag start so the auto-scroll tick can
        /// re-hit-test the cursor against new scroll positions and extend
        /// the selection as the viewport scrolls.
        data: std::sync::Arc<crate::widgets::diff_canvas::DiffCanvasData>,
    },
    DiffSelectionExtend {
        canvas_id: crate::widgets::diff_canvas::DiffCanvasId,
        row: usize,
        col: usize,
    },
    DiffSelectionEnd {
        canvas_id: crate::widgets::diff_canvas::DiffCanvasId,
    },
    /// Clear any active selection (e.g. file change).
    DiffSelectionCleared,
    /// Gutter "hot spot" click (e.g. a conflict hunk checkbox).
    /// `meta` is an opaque u64 provided by the row — for conflict buffers
    /// this is the hunk index.
    DiffGutterClicked {
        canvas_id: crate::widgets::diff_canvas::DiffCanvasId,
        row: usize,
        meta: u64,
    },
    /// Ctrl+C — copy current selection to clipboard.
    DiffCopyRequested,
    /// Keeps the sticky gutter in sync with content scroll.
    DiffContentScrolled {
        viewport: iced::widget::scrollable::Viewport,
    },
    ContinueVisibleHighlighting,
    SyntaxGrammarAssetsChanged,
    /// Shift+wheel over the diff — scroll horizontally by `delta_lines`,
    /// suppressing the default vertical scroll.
    DiffShiftWheel {
        delta_lines: f32,
    },
    NavigateFileUp,
    NavigateFileDown,
    ConflictHunkSideToggled {
        hunk_idx: usize,
        side: super::panels::diff::ConflictSide,
    },
    ConflictSideAllToggled(super::panels::diff::ConflictSide),
    ConflictBufferScrolled {
        side: super::panels::diff::ConflictSide,
        viewport: iced::widget::scrollable::Viewport,
    },
    ConflictOutputScrolled {
        viewport: iced::widget::scrollable::Viewport,
    },
    /// Shift+wheel over a conflict buffer — scroll horizontally, suppressing vertical scroll.
    ConflictShiftWheel {
        target: super::panels::diff::ConflictScrollTarget,
        delta_lines: f32,
    },
    ConflictResolutionSaveRequested,
    ConflictHighlightReady {
        generation: u64,
        ours: Option<std::sync::Arc<crate::services::HighlightedFile>>,
        theirs: Option<std::sync::Arc<crate::services::HighlightedFile>>,
    },
    LoadCommitFileDiff {
        generation: u64,
        commit_hash: String,
        path: String,
    },
    LoadDirtyFileDiff {
        generation: u64,
        path: String,
        is_staged: bool,
    },
    LoadConflictResolution {
        generation: u64,
        path: String,
    },
    LoadMergedFileDiff {
        generation: u64,
        hashes: Vec<String>,
        path: String,
    },
    RunSingleFileRenderBuild {
        generation: u64,
        kind: super::panels::diff::SingleFileDiffKind,
        file_path: String,
        lines: std::sync::Arc<Vec<crate::services::WorkingTreeDiffLine>>,
        fallbacks: crate::services::DiffFallbacks,
        highlight_provider:
            std::sync::Arc<crate::widgets::diff_canvas::CachedDiffHighlightProvider>,
    },
    SingleFileRenderReady {
        generation: u64,
        kind: super::panels::diff::SingleFileDiffKind,
        file_path: String,
        data: std::sync::Arc<crate::widgets::diff_canvas::DiffCanvasData>,
    },
    RunConflictHighlight {
        generation: u64,
        file_path: String,
        ours_content: Option<String>,
        theirs_content: Option<String>,
    },
    /// Text-buffer search message targeting the currently active search bar.
    /// `DiffPanel` tracks which canvas owns the search; screen-layer dispatch
    /// just needs to know a widget message arrived.
    TextSearch(crate::widgets::search_widget::SearchWidgetMessage),
    /// Cursor entered a buffer canvas. Updates the hover tracker so the
    /// shared text-search bar re-renders on top of the new buffer; matches
    /// recompute silently (no scroll or re-select).
    CanvasHoverEntered(crate::widgets::diff_canvas::DiffCanvasId),
    None,
}

#[derive(Debug, Clone)]
pub enum OverlayPanelAction {
    ConflictNewBranchInput(String),
    ConflictCreateBranch,
    ConflictResetLocal,
    ConflictCancel,
    ModifyDeleteKeepModified,
    ModifyDeleteDeleteFile,
    ModifyDeleteKeepBase,
    ModifyDeleteCancel,
    BranchDeleteConfirmed,
    BranchDeleteAllConfirmed,
    BranchDeleteCanceled,
    StashDeleteConfirmed,
    StashDeleteCanceled,
    BranchRenameConfirmed,
    BranchRenameCanceled,
    RenameNewBranchInput(String),
    CreateBranchHereConfirmed,
    CreateBranchHereCanceled,
    CreateBranchHereInput(String),
    DiscardConfirmed,
    DiscardCanceled,
    AddRemoteOpen,
    AddRemoteClose,
    AddRemoteNameChanged(String),
    AddRemotePullUrlChanged(String),
    AddRemotePushUrlChanged(String),
    AddRemoteConfirmed,
    /// Set upstream (first push): remote-branch name input.
    SetUpstreamInput(String),
    /// Set upstream (first push): selected remote.
    SetUpstreamRemoteChanged(String),
    /// Set upstream (first push): remote dropdown visibility.
    SetUpstreamRemoteDropdownToggled,
    SetUpstreamConfirmed,
    SetUpstreamCanceled,
    /// Push behind remote: pull (fast-forward).
    PushBehindPullRequested,
    /// Push behind remote: force push.
    PushBehindForcePushRequested,
    PushBehindCanceled,
    ForcePushConfirmed,
    ForcePushCanceled,
    CreateTagHereInput(String),
    CreateTagHereConfirmed,
    CreateTagHereCanceled,
    /// Also deletes from any remotes holding the tag.
    DeleteTagConfirmed,
    DeleteTagCanceled,
    /// Cherry pick: immediately commit.
    CherryPickImmediateConfirmed,
    /// Cherry pick: apply changes without committing.
    CherryPickStagedConfirmed,
    CherryPickCanceled,
    /// Revert: immediately commit.
    RevertImmediateConfirmed,
    /// Revert: apply changes without committing.
    RevertInPlaceConfirmed,
    RevertCanceled,
    CreateWorktreeOpen {
        available_refs: Vec<super::overlays::create_worktree::RefChoice>,
        default_dir_prefix: String,
    },
    CreateWorktreeClose,
    CreateWorktreeReferenceChanged(super::overlays::create_worktree::RefChoice),
    CreateWorktreeDropdownToggled,
    CreateWorktreeBranchNameChanged(String),
    CreateWorktreeWorkingDirChanged(String),
    CreateWorktreeBrowseRequested,
    /// Folder picker resolved (Some = picked, None = cancelled).
    CreateWorktreeBrowseResolved(Option<std::path::PathBuf>),
    CreateWorktreeConfirmed,
    WorktreeRemoveRequested {
        path: std::path::PathBuf,
        branch_name: String,
        is_active: bool,
    },
    WorktreeRemoveConfirmed,
    WorktreeRemoveCanceled,
    None,
}

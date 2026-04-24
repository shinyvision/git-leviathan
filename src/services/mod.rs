pub mod file_watcher;
pub mod gateway;
pub mod git;
pub mod git_detect;
pub mod git_error;
pub mod migrations;
pub mod presenter;
pub mod settings;
pub mod snapshot;
pub mod syntax_highlight;
#[cfg(test)]
pub(crate) mod test_support;
pub mod text_measurement;

pub use gateway::{GitRepositoryGateway, PushGatewayOutcome, SharedRepositoryGateway};
pub use git::working_tree_diff::{
    DiffLineType, DiffSegment, SegmentKind, WorkingTreeDiffLine, WorkingTreeDiffResult,
};
pub use git::{
    load_commit_diff, load_merged_commit_diff, load_merged_commit_file_diff, BranchMergeOutcome,
    CherryPickOutcome, CommitDiffResult, ConflictBlock, ConflictResolutionResult, GitService,
    MergedCommitDiffResult, PushOutcome, RemoteCheckoutOutcome, ResetMode, StashApplyOutcome,
    COMMIT_LOAD_LIMIT,
};
pub use git_detect::{detect as detect_git, resolve_primary_and_active, GitStatus};
pub use git_error::GitError;
pub use presenter::{DefaultPresenter, Presenter};
pub use settings::SettingsService;
pub use snapshot::{
    CommitSnapshot, DirtySnapshot, RefsSnapshot, RepoRef, RepoRefKind, RepoSnapshot, StashSnapshot,
};
pub use syntax_highlight::{
    file_extension_from_path, highlight_file, HighlightedFile, SyntaxHighlightedSpan, SyntaxStyle,
};
pub use text_measurement::{cached_measure_width, cached_truncate_name, FontFamily};
pub use crate::core::WorktreeInfo;

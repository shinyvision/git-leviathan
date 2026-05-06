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

pub use crate::core::WorktreeInfo;
pub use gateway::{GitRepositoryGateway, PushGatewayOutcome, SharedRepositoryGateway};
pub use git::working_tree_diff::{
    DiffContentSkipReason, DiffFallbacks, DiffLineType, DiffSegment, DiffSide, DirtyDiffSignature,
    SegmentKind, WorkingTreeDiffLine, WorkingTreeDiffResult,
};
pub use git::{
    kill_running_git_processes, load_commit_diff, load_merged_commit_diff,
    load_merged_commit_file_diff, BranchMergeOutcome, CherryPickOutcome, CommitDiffResult,
    ConflictBlock, ConflictResolutionResult, GitService, MergedCommitDiffResult,
    ModifyDeleteConflict, ModifyDeleteConflictChoice, PushOutcome, RemoteCheckoutOutcome,
    ResetMode, RevertOutcome, StashApplyOutcome, COMMIT_LOAD_LIMIT,
};
pub use git_detect::{detect as detect_git, resolve_primary_and_active, GitStatus};
pub use git_error::GitError;
pub use presenter::{DefaultPresenter, Presenter};
pub use settings::{PersistedPluginTab, SettingsService};
pub use snapshot::{
    CommitSnapshot, DirtySnapshot, RefsSnapshot, RepoRef, RepoRefKind, RepoSnapshot, StashSnapshot,
};
pub use syntax_highlight::{
    file_extension_from_path, highlight_document, install_runtime_grammar,
    refresh_runtime_grammar_registry_from_path, release_syntax_caches, uninstall_runtime_grammar,
    update_runtime_grammar, HighlightDocument, HighlightedFile, SyntaxHighlightedSpan, SyntaxStyle,
};
pub use text_measurement::{
    cached_measure_width, cached_truncate_name, release_text_caches, FontFamily,
};

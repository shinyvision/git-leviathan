use crate::services::{
    CommitDiffResult, ConflictResolutionResult, DirtyDiffSignature, GitError,
    MergedCommitDiffResult, ModifyDeleteConflict, RefsSnapshot, RepoSnapshot,
    WorkingTreeDiffResult,
};

/// Read-only view of the repository.
///
/// Segregated so that callers (e.g. diff panel, sidebar) that only need reads
/// don't get a dependency on write methods. Also serves as the super-trait for
/// every write trait — every mutator wants to re-read state after acting.
pub trait RepoRead: Send + Sync {
    fn load_repo(&self, commit_limit: usize) -> Result<RepoSnapshot, GitError>;
    fn load_refs_snapshot(&self, commit_limit: usize) -> Result<RefsSnapshot, GitError>;
    fn head_hash(&self) -> Result<Option<String>, GitError>;
    fn current_branch_name(&self) -> Result<Option<String>, GitError>;
    fn load_commit_diff(&self, commit_idx: usize, hash: &str)
        -> Result<CommitDiffResult, GitError>;
    fn load_merged_commit_diff(
        &self,
        hashes: Vec<String>,
    ) -> Result<MergedCommitDiffResult, GitError>;
    fn load_working_tree_diff(
        &self,
        file_path: &str,
        is_staged: bool,
    ) -> Result<WorkingTreeDiffResult, GitError>;
    fn load_commit_file_diff(
        &self,
        commit_hash: &str,
        file_path: &str,
    ) -> Result<WorkingTreeDiffResult, GitError>;
    fn load_merged_commit_file_diff(
        &self,
        hashes: &[String],
        file_path: &str,
    ) -> Result<WorkingTreeDiffResult, GitError>;
    fn load_conflict_resolution(
        &self,
        file_path: &str,
    ) -> Result<ConflictResolutionResult, GitError>;
    fn load_modify_delete_conflict(
        &self,
        file_path: &str,
    ) -> Result<Option<ModifyDeleteConflict>, GitError>;
    fn compute_dirty_file_signature(&self, file_path: &str, is_staged: bool) -> DirtyDiffSignature;
    /// Whether the current branch can strictly fast-forward to `target_branch`.
    /// Computed on demand (one ancestry walk) when a branch's context menu
    /// opens, rather than eagerly for every branch during `load_repo`.
    fn can_fast_forward_to(&self, target_branch: &str) -> bool;
}

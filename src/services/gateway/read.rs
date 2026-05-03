use crate::services::{
    CommitDiffResult, ConflictResolutionResult, DirtyDiffSignature, GitError,
    MergedCommitDiffResult, RefsSnapshot, RepoSnapshot, WorkingTreeDiffResult,
};

/// Read-only view of the repository.
///
/// Segregated so that callers (e.g. diff panel, sidebar) that only need reads
/// don't get a dependency on write methods. Also serves as the super-trait for
/// every write trait — every mutator wants to re-read state after acting.
pub trait RepoRead: Send + Sync {
    fn load_repo(&self, commit_limit: usize) -> Result<RepoSnapshot, GitError>;
    fn load_refs_snapshot(&self, commit_limit: usize) -> Result<RefsSnapshot, GitError>;
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
    fn compute_dirty_file_signature(&self, file_path: &str, is_staged: bool) -> DirtyDiffSignature;
}

use crate::services::{BranchMergeOutcome, GitError, RemoteCheckoutOutcome, RepoSnapshot};

use super::read::RepoRead;

/// Branch-level mutations. Super-trait on `RepoRead` since every branch op
/// needs to hand back a post-op snapshot.
pub trait BranchOps: RepoRead {
    fn checkout_branch(&self, branch_name: &str) -> Result<RepoSnapshot, GitError>;
    fn checkout_remote_branch(&self, branch_name: &str) -> Result<RemoteCheckoutOutcome, GitError>;
    fn create_branch_from_remote(
        &self,
        new_name: &str,
        remote_ref: &str,
    ) -> Result<RepoSnapshot, GitError>;
    fn reset_branch_to_remote(
        &self,
        branch_name: &str,
        remote_ref: &str,
    ) -> Result<RepoSnapshot, GitError>;
    fn delete_branch(
        &self,
        branch_name: &str,
        is_remote: bool,
        force: bool,
    ) -> Result<RepoSnapshot, GitError>;
    fn delete_branch_all(&self, branch_name: &str) -> Result<RepoSnapshot, GitError>;
    fn rename_branch(
        &self,
        old_name: &str,
        new_name: &str,
        is_remote: bool,
    ) -> Result<RepoSnapshot, GitError>;
    fn create_branch_at_commit(
        &self,
        branch_name: &str,
        commit_hash: &str,
    ) -> Result<RepoSnapshot, GitError>;
    fn merge_branch_into(
        &self,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<BranchMergeOutcome, GitError>;
    fn fast_forward_branch_to_branch(
        &self,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<RepoSnapshot, GitError>;
    fn abort_merge(&self) -> Result<RepoSnapshot, GitError>;
    fn rebase_current_onto(
        &self,
        source_branch: &str,
        target_ref: &str,
    ) -> Result<RepoSnapshot, GitError>;
}

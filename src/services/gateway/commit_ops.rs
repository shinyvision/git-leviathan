use crate::services::{CherryPickOutcome, GitError, RepoSnapshot, ResetMode};

use super::read::RepoRead;

/// Commit-rewriting operations (cherry-pick, squash, reword, reset).
pub trait CommitOps: RepoRead {
    fn reset_current_branch_to_commit(
        &self,
        commit_hash: &str,
        mode: ResetMode,
    ) -> Result<RepoSnapshot, GitError>;
    fn squash_commits(&self, hashes: Vec<String>) -> Result<RepoSnapshot, GitError>;
    fn cherry_pick_commit(
        &self,
        commit_hash: String,
        immediate_commit: bool,
    ) -> Result<CherryPickOutcome, GitError>;
    fn reword_commit(&self, hash: String, new_message: String) -> Result<RepoSnapshot, GitError>;
}

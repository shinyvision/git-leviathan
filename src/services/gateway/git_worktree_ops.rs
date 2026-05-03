use std::path::PathBuf;

use crate::services::{GitError, RepoSnapshot, WorktreeInfo};

use super::read::RepoRead;

/// Real git-worktree operations. Distinct from `WorkingTreeOps` (staging/dirty).
pub trait GitWorktreeOps: RepoRead {
    fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, GitError>;
    fn add_worktree(
        &self,
        path: PathBuf,
        new_branch_name: Option<String>,
        ref_to_checkout: String,
    ) -> Result<RepoSnapshot, GitError>;
    fn remove_worktree(&self, path: PathBuf) -> Result<RepoSnapshot, GitError>;
}

use crate::services::{DirtySnapshot, GitError, RepoSnapshot};

use super::read::RepoRead;

/// Staged/unstaged working-tree mutations.
pub trait WorkingTreeOps: RepoRead {
    fn stage_file(&self, path: &str) -> Result<RepoSnapshot, GitError>;
    fn stage_all_dirty_changes(&self) -> Result<RepoSnapshot, GitError>;
    fn unstage_file(&self, path: &str) -> Result<RepoSnapshot, GitError>;
    fn unstage_all_dirty_changes(&self) -> Result<RepoSnapshot, GitError>;
    fn stage_file_and_load_dirty(&self, path: &str) -> Result<Option<DirtySnapshot>, GitError>;
    fn stage_all_dirty_changes_and_load_dirty(&self) -> Result<Option<DirtySnapshot>, GitError>;
    fn unstage_file_and_load_dirty(&self, path: &str) -> Result<Option<DirtySnapshot>, GitError>;
    fn unstage_all_dirty_changes_and_load_dirty(&self) -> Result<Option<DirtySnapshot>, GitError>;
    fn mark_conflict_resolved(&self, path: &str) -> Result<RepoSnapshot, GitError>;
    fn mark_all_conflicts_resolved(&self) -> Result<RepoSnapshot, GitError>;
    fn discard_file(&self, path: &str) -> Result<RepoSnapshot, GitError>;
    fn discard_all_dirty_changes(&self) -> Result<RepoSnapshot, GitError>;
    fn commit_dirty_changes(
        &self,
        summary: &str,
        description: &str,
    ) -> Result<RepoSnapshot, GitError>;
    fn save_conflict_resolution(
        &self,
        file_path: &str,
        content: &str,
    ) -> Result<RepoSnapshot, GitError>;
}

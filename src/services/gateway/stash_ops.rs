use crate::services::{GitError, RepoSnapshot, StashApplyOutcome};

use super::read::RepoRead;

/// Stash lifecycle operations.
pub trait StashOps: RepoRead {
    fn create_stash(&self, message: Option<&str>) -> Result<RepoSnapshot, GitError>;
    fn apply_stash(&self, hash: &str) -> Result<StashApplyOutcome, GitError>;
    fn pop_stash(&self, hash: &str) -> Result<StashApplyOutcome, GitError>;
    fn drop_stash(&self, hash: &str) -> Result<RepoSnapshot, GitError>;
}

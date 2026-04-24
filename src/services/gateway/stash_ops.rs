use crate::services::{GitError, RepoSnapshot, StashApplyOutcome};

use super::read::RepoRead;

pub trait StashOps: RepoRead {
    fn create_stash(&self) -> Result<RepoSnapshot, GitError>;
    fn apply_stash(&self, index: usize) -> Result<StashApplyOutcome, GitError>;
    fn pop_stash(&self, index: usize) -> Result<StashApplyOutcome, GitError>;
    fn drop_stash(&self, index: usize) -> Result<RepoSnapshot, GitError>;
}

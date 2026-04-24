use crate::services::{GitError, RepoSnapshot};

use super::read::RepoRead;

/// Tag create/delete/push and the tag→remotes cache lookup.
pub trait TagOps: RepoRead {
    fn create_tag(&self, tag_name: &str, target_hash: &str) -> Result<RepoSnapshot, GitError>;
    fn delete_tag(&self, tag_name: &str) -> Result<RepoSnapshot, GitError>;
    fn push_tag(&self, remote_name: &str, tag_name: &str) -> Result<RepoSnapshot, GitError>;
    fn delete_remote_tag(
        &self,
        remote_name: &str,
        tag_name: &str,
    ) -> Result<RepoSnapshot, GitError>;
    /// Remotes known (from most recent fetch) to hold the named tag.
    fn tag_remotes_for(&self, tag_name: &str) -> Vec<String>;
}

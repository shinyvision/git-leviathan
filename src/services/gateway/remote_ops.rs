use crate::services::{GitError, RepoSnapshot};

use super::read::RepoRead;

/// Outcome of a push attempt as seen by the gateway layer. Differs from the
/// lower-level `PushOutcome` by already folding the post-push reload snapshot
/// into the success arm — the caller gets one message regardless of what the
/// push needed.
#[derive(Debug, Clone)]
pub enum PushGatewayOutcome {
    Pushed(Box<RepoSnapshot>),
    NeedsUpstream {
        branch_name: String,
        remote_name: String,
    },
    BehindRemote {
        branch_name: String,
        remote_name: String,
    },
}

/// Network-facing operations (fetch, push, pull, remote management).
pub trait RemoteOps: RepoRead {
    /// Perform the network fetch for all configured remotes. Does NOT return
    /// repository data — follow up with `load_refs_snapshot`. Splitting the
    /// halves is deliberate: keeps the "reload" signal scoped to the
    /// components that consume refs instead of spraying a full-repo snapshot.
    fn fetch_remotes(&self) -> Result<(), GitError>;
    fn push_current_branch(&self) -> Result<PushGatewayOutcome, GitError>;
    fn push_and_set_upstream(
        &self,
        remote_name: &str,
        remote_branch_name: &str,
    ) -> Result<RepoSnapshot, GitError>;
    fn force_push_current_branch(&self) -> Result<PushGatewayOutcome, GitError>;
    fn pull_current_branch(&self) -> Result<RepoSnapshot, GitError>;
    fn add_remote(
        &self,
        name: &str,
        pull_url: &str,
        push_url: &str,
    ) -> Result<RepoSnapshot, GitError>;
}

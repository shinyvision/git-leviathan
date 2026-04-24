use crate::services::{GitError, RepoSnapshot};

use super::read::RepoRead;

/// Push outcome seen by the gateway layer. Unlike the lower-level `PushOutcome`,
/// the success arm already carries the post-push snapshot so the caller gets
/// one message regardless of what the push needed.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // Pushed carries RepoSnapshot; boxing would churn gateway + presenter call sites.
pub enum PushGatewayOutcome {
    Pushed(RepoSnapshot),
    NeedsUpstream {
        branch_name: String,
        remote_name: String,
    },
    BehindRemote {
        branch_name: String,
        remote_name: String,
    },
}

pub trait RemoteOps: RepoRead {
    /// Fetches all configured remotes. Does NOT return repository data —
    /// callers follow up with `load_refs_snapshot`. The split keeps the reload
    /// signal scoped to ref-consuming components instead of spraying a
    /// full-repo snapshot.
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

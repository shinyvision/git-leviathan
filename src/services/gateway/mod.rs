pub mod branch_ops;
pub mod commit_ops;
pub mod git_worktree_ops;
pub mod read;
pub mod remote_ops;
pub mod shared;
pub mod stash_ops;
pub mod tag_ops;
pub mod working_tree_ops;

pub use branch_ops::BranchOps;
pub use commit_ops::CommitOps;
pub use git_worktree_ops::GitWorktreeOps;
pub use read::RepoRead;
pub use remote_ops::{PushGatewayOutcome, RemoteOps};
pub use shared::{GitRepositoryGateway, SharedRepositoryGateway};
pub use stash_ops::StashOps;
pub use tag_ops::TagOps;
pub use working_tree_ops::WorkingTreeOps;

/// Composed super-trait for callers that need full repo access. Narrow callers
/// depend on the segregated sub-traits instead. `RepoRead` is listed explicitly
/// even though every sub-trait already requires it, to document that every
/// full repo is readable.
pub trait Repository:
    RepoRead + BranchOps + WorkingTreeOps + GitWorktreeOps
    + CommitOps + RemoteOps + StashOps + TagOps
{
}

impl<T> Repository for T where
    T: RepoRead + BranchOps + WorkingTreeOps + GitWorktreeOps
        + CommitOps + RemoteOps + StashOps + TagOps
{
}

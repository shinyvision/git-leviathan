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

/// Composed super-trait. Any caller that needs full repo access asks for
/// `&impl Repository`; any caller that needs only a slice asks for the narrow
/// trait instead. Blanket impl means no manual wiring — any type that
/// implements each segregated trait automatically implements this. `RepoRead`
/// is named explicitly even though each sub-trait already requires it; the
/// redundancy documents the fact that full repos are always readable.
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

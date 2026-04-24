//! Pure git domain types — no UI projection, no runtime state.
//!
//! These types represent data sourced from the repository itself
//! (commits, files, identifiers). UI-derived projections live under
//! `view_model/`.

pub mod commit;
pub mod ids;
pub mod worktree;

pub use commit::{ChangeKind, ChangedFile, Commit, CommitKind};
pub use ids::{RepoVersion, TabId};
pub use worktree::WorktreeInfo;

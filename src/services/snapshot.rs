use std::collections::HashSet;
use std::path::PathBuf;

use crate::core::{ChangedFile, WorktreeInfo};

#[derive(Debug, Clone, Default)]
pub struct RepoSnapshot {
    pub commits: Vec<CommitSnapshot>,
    pub refs: Vec<RepoRef>,
    pub dirty: Option<DirtySnapshot>,
    pub stashes: Vec<StashSnapshot>,
    pub repo_name: String,
    pub current_branch: Option<String>,
    pub head_hash: Option<String>,
    pub has_more_commits: bool,
    /// Preferred remote — "origin" when present, else first configured. `None`
    /// when the repo has no remotes. Populated so right-click menus don't have
    /// to synchronously reopen the repo to derive it.
    pub default_remote_name: Option<String>,
    /// Local branch short-names that the current branch can strictly
    /// fast-forward to (i.e. current is a proper ancestor of the branch).
    /// Populated so `BranchLabelRightClicked` / `SidebarBranchRightClicked`
    /// don't have to synchronously call libgit2's `graph_descendant_of`.
    pub fast_forward_candidates: HashSet<String>,
    pub worktrees: Vec<WorktreeInfo>,
    /// Which worktree directory this snapshot reflects. Empty when the repo
    /// is bare (no workdir) or until populated; see Task 8 of the worktrees
    /// rollout. For non-bare repos, the empty path is only seen pre-load.
    #[allow(dead_code)]
    pub active_worktree_path: PathBuf,
}

/// Subset of `RepoSnapshot` produced by a refs-only reload (post-fetch).
///
/// A fetch cannot change the checked-out branch or the repo name, so those
/// fields are absent here. `dirty` IS included because the dirty row is part
/// of the commit graph — excluding it would make the working-tree row vanish
/// from the graph every time a fetch completes.
#[derive(Debug, Clone, Default)]
pub struct RefsSnapshot {
    pub commits: Vec<CommitSnapshot>,
    pub refs: Vec<RepoRef>,
    pub dirty: Option<DirtySnapshot>,
    pub stashes: Vec<StashSnapshot>,
    pub head_hash: Option<String>,
    pub has_more_commits: bool,
    pub default_remote_name: Option<String>,
    pub fast_forward_candidates: HashSet<String>,
    pub worktrees: Vec<WorktreeInfo>,
    /// Which worktree directory this snapshot reflects. Empty when the repo
    /// is bare (no workdir) or until populated; see Task 8 of the worktrees
    /// rollout. For non-bare repos, the empty path is only seen pre-load.
    pub active_worktree_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct StashSnapshot {
    pub stash_index: usize,
    pub hash: String,
    pub parent_hash: String,
    /// Second parent of the stash commit — points to the staged-index commit
    /// git builds as part of the stash tree. Used to inject the index as a
    /// distinct row in the graph so it gets its own lane.
    pub index_hash: Option<String>,
    pub message: String,
    pub author_name: String,
    pub authored_at: i64,
    pub authored_offset_minutes: i32,
}

#[derive(Debug, Clone)]
pub struct DirtySnapshot {
    pub conflicted_files: Vec<ChangedFile>,
    pub staged_files: Vec<ChangedFile>,
    pub unstaged_files: Vec<ChangedFile>,
    pub parent_hashes: Vec<String>,
}

impl DirtySnapshot {
    pub fn has_changes(&self) -> bool {
        !self.conflicted_files.is_empty()
            || !self.staged_files.is_empty()
            || !self.unstaged_files.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct CommitSnapshot {
    pub hash: String,
    pub message: String,
    pub author_name: String,
    pub authored_at: i64,
    pub authored_offset_minutes: i32,
    pub parent_hashes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepoRefKind {
    LocalBranch,
    RemoteBranch,
    Tag,
}

#[derive(Debug, Clone)]
pub struct RepoRef {
    pub name: String,
    pub kind: RepoRefKind,
    pub target_hash: String,
    pub remote_name: Option<String>,
    pub is_current: bool,
    pub upstream_ref: Option<String>,
}

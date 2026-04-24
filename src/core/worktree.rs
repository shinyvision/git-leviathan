use std::path::PathBuf;

/// One row from `git worktree list`. Primary = the repository's original
/// workdir (where `.git` is a directory); secondary = any additional worktree
/// (where `.git` is a file pointing into `<primary>/.git/worktrees/<name>/`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch_name: String,
    pub head_hash: String,
    pub is_primary: bool,
    pub is_locked: bool,
    pub is_prunable: bool,
}

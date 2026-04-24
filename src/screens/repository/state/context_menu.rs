//! Context-menu state structs that the popout controller and `view` layers
//! consume. Kept separate from the controller so the controller file stays
//! focused on the open/close state machine rather than the shape of every
//! menu variant.

use iced::Point;

#[derive(Debug, Clone)]
pub struct ContextMenuState {
    pub branch_name: String,
    /// For tag rows: remote names known to hold this tag. Empty for non-tag.
    pub tag_remote_names: Vec<String>,
    /// For tag rows: name of the first configured remote (for "Push tag to
    /// {remote}" menu entry). None if no remote configured.
    pub default_remote_name: Option<String>,
    pub is_remote: bool,
    pub has_remote: bool,
    pub is_tag: bool,
    /// The remote this branch's remote-counterpart lives on (e.g. "origin"),
    /// when applicable. Used to label remote actions as "<remote>/<branch>".
    pub remote_name: Option<String>,
    /// The short branch name on the remote side, when it differs from
    /// `branch_name` (local tracks a renamed remote). Used for remote rename/
    /// delete git operations.
    pub remote_branch_name: Option<String>,
    /// True if current branch can be fast-forwarded to this branch
    /// (current branch is an ancestor of this branch).
    pub can_fast_forward: bool,
    pub position: Point,
}

/// State for the Reset submenu shown when user clicks "Reset ... to this commit".
#[derive(Debug, Clone)]
pub struct ResetSubmenuState {
    pub commit_hash: String,
    pub position: Point,
}

#[derive(Debug, Clone, Default)]
pub(in crate::screens::repository) struct ResetHoverTracker {
    pub(in crate::screens::repository) commit_hash: String,
    pub(in crate::screens::repository) parent_hovered: bool,
    pub(in crate::screens::repository) submenu_hovered: bool,
    pub(in crate::screens::repository) open_gen: u64,
    pub(in crate::screens::repository) close_gen: u64,
}

/// State for commit context menu (right-clicking a commit).
#[derive(Debug, Clone)]
pub struct CommitContextMenuState {
    pub commit_idx: usize,
    pub commit_hash: String,
    pub position: Point,
    pub stash_index: Option<usize>,
    pub stash_display_name: Option<String>,
    pub selected_indices: Vec<usize>,
    pub selected_hashes: Vec<String>,
}

/// State for dirty file context menu (right-clicking an uncommitted-changes file row).
#[derive(Debug, Clone)]
pub struct DirtyFileContextMenuState {
    pub path: String,
    pub position: Point,
}

/// State for the worktree context menu (right-clicking a worktree sidebar entry).
#[derive(Debug, Clone)]
pub struct WorktreeContextMenuState {
    pub path: std::path::PathBuf,
    pub branch_name: String,
    pub is_active: bool,
    pub position: Point,
}

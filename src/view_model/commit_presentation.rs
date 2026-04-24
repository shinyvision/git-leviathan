use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BranchLabel {
    pub name: String,
    pub kind: BranchLabelKind,
    pub lane_color: usize,
    /// For remote-kind labels, the remote this branch belongs to (e.g. "origin").
    pub remote_name: Option<String>,
    /// For local-kind labels, the configured upstream ref if any
    /// (e.g. "origin/other-branch"). Used to merge a renamed remote into the
    /// same pill as its tracking local branch.
    pub upstream_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchLabelKind {
    CurrentLocal,
    Local,
    Remote,
    Tag,
}

/// Pre-computed display row that merges local/remote/tag labels by name.
/// Created once during projection and cached in CommitPresentation to avoid
/// recomputing on every frame.
#[derive(Debug, Clone)]
pub struct BranchDisplayRow {
    pub name: String,
    pub lane_color: usize,
    pub has_local: bool,
    pub has_remote: bool,
    pub is_current: bool,
    pub is_tag: bool,
    /// Remote this branch exists on, if any (e.g. "origin"). Populated from
    /// the remote-kind label; None when no remote counterpart.
    pub remote_name: Option<String>,
    /// Short branch name on the remote side (e.g. "other-branch"). Differs
    /// from `name` when a local branch tracks a remote of a different name.
    /// Used for remote-side rename/delete git operations.
    pub remote_branch_name: Option<String>,
    pub worktree_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct CommitPresentation {
    pub branch_labels: Vec<BranchLabel>,
    /// Pre-computed display rows (merged local/remote/tag labels).
    /// Avoids recomputing branch_display_rows() on every frame.
    pub branch_display_rows: Vec<BranchDisplayRow>,
    /// Relative time string shown in the commit list (e.g. "1 hour ago")
    pub relative_time: Option<String>,
}

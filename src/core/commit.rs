#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Modified,
    Added,
    Deleted,
}

#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitKind {
    Commit,
    Dirty,
    Stash,
}

/// Pure git-domain commit data.
///
/// Per-commit projection data (relative time, branch labels) lives in
/// `view_model::CommitPresentation`. Diff-loaded state (file counts, file
/// list) lives in `view_model::CommitDiffState`.
#[derive(Debug, Clone)]
pub struct Commit {
    pub kind: CommitKind,
    pub hash: String,
    pub short_hash: String,
    pub message: String,
    pub author: String,
    pub date: String,
    /// Full hashes of all parents, in order. Empty for root/dirty commits.
    pub parent_hashes: Vec<String>,
    pub is_merge_in_progress: bool,
    /// Dirty-commit-only file buckets sourced from the working-tree snapshot.
    /// Empty for `Commit`/`Stash` kinds.
    pub conflicted_files: Vec<ChangedFile>,
    pub staged_files: Vec<ChangedFile>,
    pub unstaged_files: Vec<ChangedFile>,
}

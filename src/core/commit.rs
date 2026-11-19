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

/// Operation that the working-tree panel can finish or abort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkingTreeOperation {
    None,
    Merge,
    Rebase,
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
    /// Full hashes of all parents, in order. A pending merge includes its merge heads.
    pub parent_hashes: Vec<String>,
    pub operation: WorkingTreeOperation,
    /// Dirty-commit-only file buckets sourced from the working-tree snapshot.
    /// Empty for `Commit`/`Stash` kinds.
    pub conflicted_files: Vec<ChangedFile>,
    pub staged_files: Vec<ChangedFile>,
    pub unstaged_files: Vec<ChangedFile>,
}

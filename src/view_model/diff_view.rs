use crate::core::ChangedFile;

/// Per-commit diff-loaded state, carried alongside `Commit` in
/// `RepositoryData`. Populated by `load_commit_diff` for committed rows; for
/// the dirty row it is built from the `DirtySnapshot` at projection time
/// (`diff_loaded=true`) because the file list is already known.
#[derive(Debug, Clone, Default)]
pub struct CommitDiffState {
    /// `true` once the per-commit diff has been computed (even if it resolved
    /// to zero files). Distinguishes "not diffed yet" from "genuinely no file
    /// changes" — e.g. a merge commit whose first-parent tree equals the
    /// merge tree, which would otherwise loop forever showing "Loading files…".
    pub diff_loaded: bool,
    pub modified_count: u32,
    pub added_count: u32,
    pub deleted_count: u32,
    pub files: Vec<ChangedFile>,
}

impl CommitDiffState {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_dirty(
        files: Vec<ChangedFile>,
        modified_count: u32,
        added_count: u32,
        deleted_count: u32,
    ) -> Self {
        Self {
            diff_loaded: true,
            modified_count,
            added_count,
            deleted_count,
            files,
        }
    }
}

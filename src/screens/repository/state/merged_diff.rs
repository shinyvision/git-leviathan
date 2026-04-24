//! Cache of the last computed merged-commit-diff projection plus a version
//! tag. Version bumps every time a new request is spawned so out-of-order
//! responses can be discarded in `commands/loaders.rs`. The two fields only
//! make sense together, so they live here as one value.

use crate::core::RepoVersion;
use crate::services::MergedCommitDiffResult;

#[derive(Default)]
pub(in crate::screens::repository) struct MergedDiffCache {
    result: Option<MergedCommitDiffResult>,
    version: RepoVersion,
}

impl MergedDiffCache {
    pub(in crate::screens::repository) fn new() -> Self {
        Self::default()
    }

    pub(in crate::screens::repository) fn reset(&mut self) {
        self.result = None;
        self.version = RepoVersion::default();
    }

    /// Clears the cached result and bumps the version; returns the new
    /// version so callers can tag an in-flight request.
    pub(in crate::screens::repository) fn invalidate(&mut self) -> RepoVersion {
        self.result = None;
        self.version = self.version.bump();
        self.version
    }

    pub(in crate::screens::repository) fn set(&mut self, result: MergedCommitDiffResult) {
        self.result = Some(result);
    }

    pub(in crate::screens::repository) fn result(&self) -> Option<&MergedCommitDiffResult> {
        self.result.as_ref()
    }

    pub(in crate::screens::repository) fn version(&self) -> RepoVersion {
        self.version
    }
}

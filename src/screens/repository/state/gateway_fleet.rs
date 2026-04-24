//! Per-tab cache of repository gateways keyed by workdir path.
//!
//! Each tab maintains one gateway for the primary repo dir and zero or more
//! gateways for secondary worktree dirs. The `active_path` selects which
//! gateway serves the tab's current reads (dirty state, HEAD, current branch);
//! repo-global ops (add/remove worktree) always route through the primary.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::services::{
    GitError, GitRepositoryGateway, Presenter, SharedRepositoryGateway,
};

pub(crate) struct GatewayFleet {
    primary_path: PathBuf,
    active_path: PathBuf,
    gateways: HashMap<PathBuf, SharedRepositoryGateway>,
    /// Retained for snapshot projection after focus swaps and worktree CRUD
    /// results — passed to the active gateway when projecting per-workdir
    /// state (see Task 7+ of the worktrees rollout).
    #[allow(dead_code)]
    presenter: Arc<dyn Presenter>,
}

impl GatewayFleet {
    pub(crate) fn new(
        primary_path: PathBuf,
        primary_gateway: SharedRepositoryGateway,
        active_path: PathBuf,
        active_gateway: SharedRepositoryGateway,
        presenter: Arc<dyn Presenter>,
    ) -> Self {
        let mut gateways = HashMap::new();
        gateways.insert(primary_path.clone(), primary_gateway);
        if active_path != primary_path {
            gateways.insert(active_path.clone(), active_gateway);
        }
        Self {
            primary_path,
            active_path,
            gateways,
            presenter,
        }
    }

    pub(crate) fn active(&self) -> &SharedRepositoryGateway {
        self.gateways
            .get(&self.active_path)
            .expect("active gateway must exist in fleet")
    }

    pub(crate) fn primary(&self) -> &SharedRepositoryGateway {
        self.gateways
            .get(&self.primary_path)
            .expect("primary gateway must exist in fleet")
    }

    pub(crate) fn active_path(&self) -> &Path {
        &self.active_path
    }

    pub(crate) fn primary_path(&self) -> &Path {
        &self.primary_path
    }

    pub(crate) fn is_active(&self, path: &Path) -> bool {
        self.active_path == path
    }

    /// Build (or fetch cached) gateway for `path`. Does NOT change the active
    /// pointer — caller follows with `swap_active` if appropriate.
    pub(crate) fn ensure(
        &mut self,
        path: PathBuf,
    ) -> Result<&SharedRepositoryGateway, GitError> {
        Ok(self.gateways.entry(path.clone()).or_insert_with(|| {
            let path_str = path.to_string_lossy().to_string();
            GitRepositoryGateway::from_path(path_str)
        }))
    }

    /// Requires `ensure(path)` first — errors if the path isn't cached.
    pub(crate) fn swap_active(&mut self, path: PathBuf) -> Result<(), GitError> {
        if !self.gateways.contains_key(&path) {
            return Err(GitError::Other(format!(
                "cannot swap active to uncached path '{}'",
                path.display(),
            )));
        }
        self.active_path = path;
        Ok(())
    }

    /// Returns the paths of all cached gateways (primary + any worktrees).
    /// Used by `on_worktree_removed` to diff against the new snapshot and evict
    /// stale entries.
    pub(crate) fn cached_paths(&self) -> Vec<PathBuf> {
        self.gateways.keys().cloned().collect()
    }

    /// Drop the cached gateway for `path`. No-op if `path` is currently active
    /// (defensive backstop — the dispatch layer is expected to pre-check with
    /// `is_active` before calling this).
    pub(crate) fn drop_path(&mut self, path: &Path) {
        if self.active_path == path {
            return;
        }
        self.gateways.remove(path);
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::DefaultPresenter;

    fn sample_fleet() -> GatewayFleet {
        let primary = PathBuf::from("/tmp/primary");
        let primary_gateway = GitRepositoryGateway::from_path(primary.to_string_lossy().to_string());
        GatewayFleet::new(
            primary.clone(),
            primary_gateway.clone(),
            primary,
            primary_gateway,
            Arc::new(DefaultPresenter::new()),
        )
    }

    #[test]
    fn ensure_creates_and_caches_gateway() {
        let mut fleet = sample_fleet();
        let extra = PathBuf::from("/tmp/worktree-a");

        let _ = fleet.ensure(extra.clone()).expect("ensure once");
        let first_gw_ptr: *const _ = fleet.gateways.get(&extra).unwrap().as_ref() as *const _;

        let _ = fleet.ensure(extra.clone()).expect("ensure twice");
        let second_gw_ptr: *const _ = fleet.gateways.get(&extra).unwrap().as_ref() as *const _;

        assert!(
            std::ptr::eq(first_gw_ptr, second_gw_ptr),
            "second ensure should return cached gateway, not recreate",
        );
    }

    #[test]
    fn swap_active_requires_ensure_first() {
        let mut fleet = sample_fleet();
        let wt = PathBuf::from("/tmp/not-yet-cached");
        let err = fleet.swap_active(wt).expect_err("swap without ensure");
        let rendered = format!("{:?}", err);
        assert!(rendered.contains("uncached"), "err should mention uncached: {rendered}");
    }

    #[test]
    fn swap_active_changes_active_path_after_ensure() {
        let mut fleet = sample_fleet();
        let wt = PathBuf::from("/tmp/worktree-b");
        let _ = fleet.ensure(wt.clone()).expect("ensure");
        fleet.swap_active(wt.clone()).expect("swap");
        assert_eq!(fleet.active_path(), wt.as_path());
    }

    #[test]
    fn drop_path_noops_when_path_is_active() {
        let mut fleet = sample_fleet();
        let wt = PathBuf::from("/tmp/worktree-c");
        let _ = fleet.ensure(wt.clone()).expect("ensure");
        fleet.swap_active(wt.clone()).expect("swap");
        fleet.drop_path(&wt);
        assert!(fleet.gateways.contains_key(&wt));
        assert_eq!(fleet.active_path(), wt.as_path());
    }

    #[test]
    fn cached_paths_returns_every_gateway() {
        let mut fleet = sample_fleet();
        let wt_a = PathBuf::from("/tmp/wt-a");
        let wt_b = PathBuf::from("/tmp/wt-b");
        let _ = fleet.ensure(wt_a.clone()).expect("ensure a");
        let _ = fleet.ensure(wt_b.clone()).expect("ensure b");
        let mut paths = fleet.cached_paths();
        paths.sort();
        let mut expected = vec![PathBuf::from("/tmp/primary"), wt_a, wt_b];
        expected.sort();
        assert_eq!(paths, expected);
    }

    #[test]
    fn cached_paths_excludes_dropped_path() {
        let mut fleet = sample_fleet();
        let wt = PathBuf::from("/tmp/wt-drop");
        let _ = fleet.ensure(wt.clone()).expect("ensure");
        fleet.drop_path(&wt);
        let paths = fleet.cached_paths();
        assert!(!paths.contains(&wt), "dropped path should not appear in cached_paths");
    }

    #[test]
    fn dropping_everything_except_primary_still_allows_primary_access() {
        let mut fleet = sample_fleet();
        let wt = PathBuf::from("/tmp/worktree-z");
        let _ = fleet.ensure(wt.clone()).expect("ensure");

        // Simulate "drop everything except active".
        // In the real on_worktree_removed code, the active is usually a
        // non-primary worktree; here we keep the sample_fleet default.
        let primary = fleet.primary_path().to_path_buf();
        let cached = fleet.cached_paths();
        for p in cached {
            if fleet.is_active(&p) || p == primary {
                continue;
            }
            fleet.drop_path(&p);
        }

        // primary() must still succeed (used to panic if dropped).
        let _ = fleet.primary();
    }

    #[test]
    fn ensure_then_swap_points_active_at_ensured_gateway() {
        let mut fleet = sample_fleet();
        let wt = PathBuf::from("/tmp/worktree-d");
        let ensured_ptr: *const _ = {
            let gw = fleet.ensure(wt.clone()).expect("ensure");
            gw.as_ref() as *const _
        };
        fleet.swap_active(wt).expect("swap");
        let active_ptr: *const _ = fleet.active().as_ref() as *const _;
        assert!(
            std::ptr::eq(ensured_ptr, active_ptr),
            "active() should return the gateway ensure() just produced",
        );
    }
}

//! Repository screen state, split into focused sub-components.
//!
//! `RepositoryData` composes the pieces that used to live as intermixed
//! fields on a single struct:
//!
//! - [`RepositorySnapshot`] — the latest projected view of the repo.
//! - [`DiffCache`]          — per-commit diff states, plus cache-carry logic.
//! - [`SelectionState`]     — anchor, multi-select, file view mode.
//! - [`AnimationState`]     — push/pull timer state.
//!
//! Hash/branch → commit-idx lookups live in the stateless [`CommitIndex`]
//! namespace, applied to `snapshot` slices on demand.
//!
//! The two cross-cutting update paths (`replace_loaded`, `apply_refs_update`)
//! remain coordination methods on `RepositoryData` because they always need
//! to update more than one sub-component atomically.

mod animation;
mod commit_index;
mod context_menu;
mod diff_cache;
pub(crate) mod gateway_fleet;
mod merged_diff;
mod operation;
mod pending_focus;
mod popout;
mod resize;
mod selection;
mod snapshot;

use std::time::{Duration, Instant};

use iced::widget::scrollable;

use crate::{
    core::{Commit, CommitKind},
    services::CommitDiffResult,
    view_model::{LoadedDirtyIndex, LoadedRefs, LoadedRepo},
    widgets::ROW_H,
};

use super::commit_search::CommitSearch;

pub(in crate::screens::repository) use animation::AnimationState;
pub(in crate::screens::repository) use commit_index::CommitIndex;
pub use context_menu::ContextMenuState;
pub(in crate::screens::repository) use context_menu::{
    eligible_tag_push_remote_names, CommitContextMenuState, DirtyFileContextMenuState,
    ResetSubmenuState,
};
pub(in crate::screens::repository) use diff_cache::DiffCache;
pub(crate) use gateway_fleet::GatewayFleet;
pub(in crate::screens::repository) use merged_diff::MergedDiffCache;
pub(crate) use operation::{OperationCoordinator, OperationId, OperationKind};
pub(in crate::screens::repository) use pending_focus::PendingFocus;
pub(in crate::screens::repository) use popout::{
    BranchPopoutController, BranchPopoutState, BranchPressOutcome, SidebarContextMenuRequest,
};
pub(in crate::screens::repository) use resize::{EffectiveLayout, ResizeState};
pub(in crate::screens::repository) use selection::SelectionState;
pub(in crate::screens::repository) use snapshot::RepositorySnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    Sidebar,
    Center,
    Detail,
}

pub(in crate::screens::repository) const SIDEBAR_DOUBLE_CLICK_WINDOW: Duration =
    Duration::from_millis(500);

const VIEWPORT_CHANGE_EPSILON: f32 = 0.01;
const DEFAULT_CENTER_VIEWPORT_H: f32 = 1400.0;

pub(in crate::screens::repository) struct SidebarClickState {
    pub(in crate::screens::repository) branch_name: String,
    pub(in crate::screens::repository) at: Instant,
}

pub(in crate::screens::repository) fn center_list_scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("repository-center-list")
}

pub(in crate::screens::repository) fn conflict_ours_scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("repository-conflict-ours")
}

pub(in crate::screens::repository) fn conflict_theirs_scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("repository-conflict-theirs")
}

pub(in crate::screens::repository) fn conflict_output_scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("repository-conflict-output")
}

pub(in crate::screens::repository) fn diff_content_scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("repository-diff-content")
}

/// Center-list scroll offset + viewport measurements. Used to decide when to
/// trigger `load_more_commits` (near-bottom) and to feed `center_view` the
/// visible range for virtualisation.
#[derive(Debug, Clone, Copy)]
pub(in crate::screens::repository) struct CenterListState {
    offset_y: f32,
    viewport_h: f32,
}

impl CenterListState {
    pub(in crate::screens::repository) fn new() -> Self {
        Self {
            offset_y: 0.0,
            viewport_h: DEFAULT_CENTER_VIEWPORT_H,
        }
    }

    pub(in crate::screens::repository) fn update(
        &mut self,
        viewport: scrollable::Viewport,
    ) -> bool {
        let next_offset_y = viewport.absolute_offset().y;
        let next_viewport_h = viewport.bounds().height.max(ROW_H);
        let changed = (self.offset_y - next_offset_y).abs() > VIEWPORT_CHANGE_EPSILON
            || (self.viewport_h - next_viewport_h).abs() > VIEWPORT_CHANGE_EPSILON;

        self.offset_y = next_offset_y;
        self.viewport_h = next_viewport_h;

        changed
    }

    pub(in crate::screens::repository) fn offset_y(&self) -> f32 {
        self.offset_y
    }

    pub(in crate::screens::repository) fn viewport_h(&self) -> f32 {
        self.viewport_h
    }

    pub(in crate::screens::repository) fn is_near_bottom(&self, commit_count: usize) -> bool {
        if commit_count == 0 {
            return false;
        }

        let content_height = commit_count as f32 * ROW_H;
        let scroll_bottom = self.offset_y + self.viewport_h;

        scroll_bottom >= content_height * 0.9
    }
}

/// Composition of the five RepositoryScreen state pieces. Fields are public
/// to the screen module so callers can address each sub-component directly —
/// `data.snapshot.commits()`, `data.cache.state(idx)`, `data.selection.anchor()`.
pub(in crate::screens::repository) struct RepositoryData {
    pub(in crate::screens::repository) snapshot: RepositorySnapshot,
    pub(in crate::screens::repository) cache: DiffCache,
    pub(in crate::screens::repository) selection: SelectionState,
    pub(in crate::screens::repository) animation: AnimationState,
    pub(in crate::screens::repository) resize: ResizeState,
    pub(in crate::screens::repository) pending_focus: Option<PendingFocus>,
    /// Branch-label popout + associated context menus. Domain state tied to
    /// the current repo — kept here so screen-level ctx can hand it out
    /// through `&mut data` without a separate ref.
    pub(in crate::screens::repository) branch_popout: BranchPopoutController,
    /// Commit-list search filter state. Lives here (not on the screen) so
    /// the center-panel view can reach it via `data` with no extra ctx field.
    pub(in crate::screens::repository) commit_search: Option<CommitSearch>,
    pub(in crate::screens::repository) operations: OperationCoordinator,
}

impl RepositoryData {
    pub(in crate::screens::repository) fn loading_with_repo_name(repo_name: String) -> Self {
        Self {
            snapshot: RepositorySnapshot::loading_with_repo_name(repo_name),
            cache: DiffCache::new(),
            selection: SelectionState::new(),
            animation: AnimationState::new(),
            resize: ResizeState::new(),
            pending_focus: None,
            branch_popout: BranchPopoutController::default(),
            commit_search: None,
            operations: OperationCoordinator::new(),
        }
    }

    /// Tab hibernation: clear view data and ephemeral UI state but preserve
    /// the snapshot's repo identity so the main bar keeps rendering the
    /// correct repo + branch while the async rehydrate load is in flight.
    pub(in crate::screens::repository) fn hibernate(&mut self) {
        self.snapshot.hibernate();
        self.cache = DiffCache::new();
        self.selection = SelectionState::new();
        self.animation = AnimationState::new();
        self.resize = ResizeState::new();
        self.pending_focus = None;
        self.branch_popout = BranchPopoutController::default();
        self.commit_search = None;
        self.operations = OperationCoordinator::new();
    }

    /// Full-reload coordination. Preserves diff cache entries across the
    /// refresh, swaps the snapshot, then records the new cache.
    pub(in crate::screens::repository) fn replace_loaded(&mut self, loaded: LoadedRepo) {
        let prev_keys = self.snapshot_commit_keys();
        let new_states = self.snapshot.replace_loaded(loaded);
        self.cache
            .replace(&prev_keys, self.snapshot.commits(), new_states);
    }

    /// Fetch-path coordination. Narrower update: preserves repo identity
    /// (name, current branch, sidebar expansion) so open overlays survive.
    pub(in crate::screens::repository) fn apply_refs_update(&mut self, refs: LoadedRefs) {
        let prev_keys = self.snapshot_commit_keys();
        let new_states = self.snapshot.apply_refs_update(refs);
        self.cache
            .replace(&prev_keys, self.snapshot.commits(), new_states);
    }

    /// `(hash, kind)` for each currently-loaded commit. The diff cache carry
    /// reads only these two fields, so capturing them avoids deep-cloning the
    /// whole commit list before a snapshot swap.
    fn snapshot_commit_keys(&self) -> Vec<(String, CommitKind)> {
        self.snapshot
            .commits()
            .iter()
            .map(|c| (c.hash.clone(), c.kind))
            .collect()
    }

    pub(in crate::screens::repository) fn apply_dirty_index_update(
        &mut self,
        loaded: LoadedDirtyIndex,
    ) -> bool {
        match loaded.dirty {
            Some(row) => {
                if !self
                    .snapshot
                    .commits()
                    .first()
                    .is_some_and(|c| c.kind == CommitKind::Dirty)
                {
                    return false;
                }
                let diff_state = row.diff_state.clone();
                self.snapshot.replace_dirty_row(row);
                self.cache.replace_dirty(diff_state);
            }
            None => {
                let had_dirty = self
                    .snapshot
                    .commits()
                    .first()
                    .is_some_and(|c| c.kind == CommitKind::Dirty);
                if had_dirty {
                    self.snapshot.remove_dirty_row();
                    self.cache.remove_dirty();
                    if self.selection.selected_commit() >= self.snapshot.commits().len() {
                        self.selection.replace_with_single(0);
                    }
                }
            }
        }
        true
    }

    /// Applies a `load_commit_diff` result to the cache. Kept as a thin
    /// coordinator so callers don't have to re-pass `snapshot.commits()`.
    pub(in crate::screens::repository) fn apply_commit_diff(&mut self, result: CommitDiffResult) {
        self.cache.apply_result(self.snapshot.commits(), result);
    }

    /// Hash-at-idx lookup. Thin coordinator over `snapshot + index`.
    pub(in crate::screens::repository) fn commit_hash(&self, idx: usize) -> Option<String> {
        CommitIndex::commit_hash(self.snapshot.commits(), idx)
    }

    /// Hash → idx lookup. Thin coordinator over `snapshot + index`.
    pub(in crate::screens::repository) fn commit_index_for_hash(
        &self,
        hash: &str,
    ) -> Option<usize> {
        CommitIndex::index_for_hash(self.snapshot.commits(), hash)
    }

    /// Branch name → idx lookup. Thin coordinator over `snapshot + index`.
    pub(in crate::screens::repository) fn commit_index_for_branch(
        &self,
        branch_name: &str,
        is_remote: bool,
    ) -> Option<usize> {
        CommitIndex::index_for_branch(self.snapshot.commit_presentations(), branch_name, is_remote)
    }

    /// Thin coordinator over `snapshot + cache`.
    pub(in crate::screens::repository) fn commit_needs_diff(&self, idx: usize) -> bool {
        self.cache.commit_needs_diff(idx, self.snapshot.commits())
    }

    /// If the commit at `idx` is a stash entry, returns its
    /// (stash_index, display_name).
    pub(in crate::screens::repository) fn stash_info_for_commit(
        &self,
        idx: usize,
    ) -> Option<(usize, String)> {
        let commit = self.snapshot.commits().get(idx)?;
        if !matches!(commit.kind, CommitKind::Stash) {
            return None;
        }
        for section in self.snapshot.sidebar_sections() {
            for stash in &section.stashes {
                if stash.hash == commit.hash {
                    return Some((stash.stash_index, stash.display_name.clone()));
                }
            }
        }
        None
    }

    pub(in crate::screens::repository) fn selected_commit(
        &self,
        selected_commit: usize,
    ) -> Option<&Commit> {
        self.snapshot.commits().get(selected_commit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::{ChangeKind, ChangedFile},
        services::{presenter::projection, CommitSnapshot, DirtySnapshot, RepoRef, RepoSnapshot},
    };

    fn commit_snapshot(hash: &str) -> CommitSnapshot {
        CommitSnapshot {
            hash: hash.to_string(),
            message: "initial".to_string(),
            author_name: "Ada".to_string(),
            authored_at: 1_700_000_000,
            authored_offset_minutes: 0,
            parent_hashes: vec![],
        }
    }

    fn dirty(staged: Vec<&str>, unstaged: Vec<&str>) -> DirtySnapshot {
        DirtySnapshot {
            conflicted_files: vec![],
            staged_files: staged
                .into_iter()
                .map(|path| ChangedFile {
                    path: path.to_string(),
                    kind: ChangeKind::Modified,
                })
                .collect(),
            unstaged_files: unstaged
                .into_iter()
                .map(|path| ChangedFile {
                    path: path.to_string(),
                    kind: ChangeKind::Modified,
                })
                .collect(),
            parent_hashes: vec!["aaaaaaaa".to_string()],
        }
    }

    fn loaded_repo(dirty: DirtySnapshot) -> crate::view_model::LoadedRepo {
        projection::project_loaded(RepoSnapshot {
            commits: vec![commit_snapshot("aaaaaaaa")],
            refs: Vec::<RepoRef>::new(),
            dirty: Some(dirty),
            stashes: vec![],
            repo_name: "repo".to_string(),
            current_branch: Some("main".to_string()),
            head_hash: Some("aaaaaaaa".to_string()),
            has_more_commits: false,
            default_remote_name: None,
            remote_names: Vec::new(),
            fast_forward_candidates: Default::default(),
            worktrees: vec![],
            active_worktree_path: Default::default(),
        })
    }

    #[test]
    fn dirty_index_update_replaces_dirty_buckets_without_reloading_history() {
        let mut data = RepositoryData::loading_with_repo_name("repo".to_string());
        data.replace_loaded(loaded_repo(dirty(vec![], vec!["file.txt"])));

        let before_history_hash = data.snapshot.commits()[1].hash.clone();
        let loaded = projection::project_dirty_index(Some(dirty(vec!["file.txt"], vec![])));

        assert!(data.apply_dirty_index_update(loaded));
        assert_eq!(data.snapshot.commits()[1].hash, before_history_hash);
        assert_eq!(data.snapshot.commits()[0].staged_files.len(), 1);
        assert!(data.snapshot.commits()[0].unstaged_files.is_empty());
        assert_eq!(data.cache.state(0).unwrap().files[0].path, "file.txt");
    }
}

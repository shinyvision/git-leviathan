//! Per-commit diff loading cache.
//!
//! Owns the `Vec<CommitDiffState>` parallel to `RepositorySnapshot::commits`.
//! Two concerns live here:
//!
//! 1. **Apply result of an async `load_commit_diff` call.** The original
//!    index captured at spawn may no longer point at the original hash if a
//!    snapshot refresh moved it (e.g. a dirty row appeared); `apply_result`
//!    falls back to a hash lookup so the diff does not get silently dropped.
//! 2. **Carry already-loaded diffs across a snapshot refresh.** When a new
//!    projection lands, `preserve_across_reload` replays the previous
//!    cache onto the incoming states, keyed by commit hash.

use std::collections::HashMap;

use crate::{
    core::{Commit, CommitKind},
    services::CommitDiffResult,
    view_model::CommitDiffState,
};

#[derive(Default)]
pub(in crate::screens::repository) struct DiffCache {
    states: Vec<CommitDiffState>,
}

impl DiffCache {
    pub(in crate::screens::repository) fn new() -> Self {
        Self::default()
    }

    pub(in crate::screens::repository) fn states(&self) -> &[CommitDiffState] {
        &self.states
    }

    pub(in crate::screens::repository) fn state(&self, idx: usize) -> Option<&CommitDiffState> {
        self.states.get(idx)
    }

    pub(in crate::screens::repository) fn commit_needs_diff(
        &self,
        idx: usize,
        commits: &[Commit],
    ) -> bool {
        let Some(commit) = commits.get(idx) else {
            return false;
        };
        let Some(diff) = self.states.get(idx) else {
            return false;
        };
        matches!(commit.kind, CommitKind::Commit | CommitKind::Stash) && !diff.diff_loaded
    }

    /// Replace the cache wholesale with the states from a fresh projection,
    /// first merging in any previously-loaded diffs by hash so the details
    /// panel does not re-enter "Loading files…".
    pub(in crate::screens::repository) fn replace(
        &mut self,
        prev_commits: &[Commit],
        new_commits: &[Commit],
        mut new_states: Vec<CommitDiffState>,
    ) {
        preserve_across_reload(prev_commits, &self.states, new_commits, &mut new_states);
        self.states = new_states;
    }

    /// Apply the result of a `load_commit_diff` task. Index-then-hash lookup
    /// guards against the originally-captured index shifting between spawn
    /// and result (see module docs).
    pub(in crate::screens::repository) fn apply_result(
        &mut self,
        commits: &[Commit],
        result: CommitDiffResult,
    ) {
        let CommitDiffResult {
            commit_idx,
            hash,
            modified_count,
            added_count,
            deleted_count,
            files,
        } = result;

        let by_idx_match = commits
            .get(commit_idx)
            .is_some_and(|commit| commit.hash == hash);
        let target_idx = if by_idx_match {
            Some(commit_idx)
        } else {
            commits.iter().position(|commit| commit.hash == hash)
        };
        if let Some(idx) = target_idx {
            if let Some(diff) = self.states.get_mut(idx) {
                diff.modified_count = modified_count;
                diff.added_count = added_count;
                diff.deleted_count = deleted_count;
                diff.files = files;
                diff.diff_loaded = true;
            }
        }
    }

    pub(in crate::screens::repository) fn replace_dirty(&mut self, state: CommitDiffState) {
        if let Some(slot) = self.states.first_mut() {
            *slot = state;
        }
    }

    pub(in crate::screens::repository) fn remove_dirty(&mut self) {
        if !self.states.is_empty() {
            self.states.remove(0);
        }
    }
}

fn preserve_across_reload(
    prev_commits: &[Commit],
    prev_states: &[CommitDiffState],
    new_commits: &[Commit],
    new_states: &mut [CommitDiffState],
) {
    let cached: HashMap<String, CommitDiffState> = prev_commits
        .iter()
        .zip(prev_states.iter())
        .filter(|(commit, diff)| {
            matches!(commit.kind, CommitKind::Commit | CommitKind::Stash) && diff.diff_loaded
        })
        .map(|(commit, diff)| (commit.hash.clone(), diff.clone()))
        .collect();

    for (commit, diff_slot) in new_commits.iter().zip(new_states.iter_mut()) {
        if let Some(cached_diff) = cached.get(&commit.hash) {
            *diff_slot = cached_diff.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ChangeKind, ChangedFile, Commit, CommitKind};

    fn sample_commit(hash: &str) -> Commit {
        Commit {
            kind: CommitKind::Commit,
            hash: hash.to_string(),
            short_hash: hash.chars().take(7).collect(),
            message: "message".to_string(),
            author: "author".to_string(),
            date: "today".to_string(),
            parent_hashes: vec![],
            is_merge_in_progress: false,
            conflicted_files: vec![],
            staged_files: vec![],
            unstaged_files: vec![],
        }
    }

    fn empty_states(count: usize) -> Vec<CommitDiffState> {
        (0..count).map(|_| CommitDiffState::empty()).collect()
    }

    #[test]
    fn apply_result_ignores_stale_hash() {
        let mut cache = DiffCache::new();
        let commits = vec![sample_commit("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")];
        cache.states = empty_states(commits.len());

        cache.apply_result(
            &commits,
            CommitDiffResult {
                commit_idx: 0,
                hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                modified_count: 3,
                added_count: 1,
                deleted_count: 0,
                files: vec![ChangedFile {
                    path: "tracked.txt".to_string(),
                    kind: ChangeKind::Modified,
                }],
            },
        );

        assert_eq!(cache.states[0].modified_count, 0);
        assert!(cache.states[0].files.is_empty());
    }

    #[test]
    fn apply_result_updates_matching_hash() {
        let mut cache = DiffCache::new();
        let commits = vec![sample_commit("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")];
        cache.states = empty_states(commits.len());

        cache.apply_result(
            &commits,
            CommitDiffResult {
                commit_idx: 0,
                hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                modified_count: 2,
                added_count: 1,
                deleted_count: 0,
                files: vec![ChangedFile {
                    path: "tracked.txt".to_string(),
                    kind: ChangeKind::Added,
                }],
            },
        );

        assert_eq!(cache.states[0].modified_count, 2);
        assert_eq!(cache.states[0].files.len(), 1);
    }

    #[test]
    fn apply_result_falls_back_to_hash_when_index_shifted() {
        let mut cache = DiffCache::new();
        let commits = vec![
            sample_commit("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            sample_commit("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ];
        cache.states = empty_states(commits.len());

        cache.apply_result(
            &commits,
            CommitDiffResult {
                commit_idx: 0,
                hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                modified_count: 2,
                added_count: 0,
                deleted_count: 0,
                files: vec![ChangedFile {
                    path: "tracked.txt".to_string(),
                    kind: ChangeKind::Modified,
                }],
            },
        );

        assert!(!cache.states[0].diff_loaded);
        assert!(cache.states[1].diff_loaded);
        assert_eq!(cache.states[1].modified_count, 2);
    }

    #[test]
    fn zero_diff_commit_marked_loaded() {
        let mut cache = DiffCache::new();
        let commits = vec![sample_commit("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")];
        cache.states = empty_states(commits.len());

        assert!(cache.commit_needs_diff(0, &commits));

        cache.apply_result(
            &commits,
            CommitDiffResult {
                commit_idx: 0,
                hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                modified_count: 0,
                added_count: 0,
                deleted_count: 0,
                files: vec![],
            },
        );

        assert!(cache.states[0].diff_loaded);
        assert!(!cache.commit_needs_diff(0, &commits));
    }

    #[test]
    fn replace_carries_loaded_diffs_by_hash() {
        let mut cache = DiffCache::new();
        let prev_commits = vec![
            sample_commit("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            sample_commit("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        ];
        let mut prev_states = empty_states(prev_commits.len());
        prev_states[0].diff_loaded = true;
        prev_states[0].modified_count = 7;
        prev_states[0].files = vec![ChangedFile {
            path: "file.txt".to_string(),
            kind: ChangeKind::Modified,
        }];
        cache.states = prev_states;

        // New projection has the same two commits but reversed, plus a new one
        // at the front; the loaded diff for the `aaa…` hash must migrate.
        let new_commits = vec![
            sample_commit("cccccccccccccccccccccccccccccccccccccccc"),
            sample_commit("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            sample_commit("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ];
        let new_states = empty_states(new_commits.len());

        cache.replace(&prev_commits, &new_commits, new_states);

        assert!(!cache.states[0].diff_loaded, "new commit stays unloaded");
        assert!(
            !cache.states[1].diff_loaded,
            "previously-unloaded stays unloaded"
        );
        assert!(
            cache.states[2].diff_loaded,
            "previously-loaded migrated by hash"
        );
        assert_eq!(cache.states[2].modified_count, 7);
        assert_eq!(cache.states[2].files.len(), 1);
    }

    #[test]
    fn replace_drops_diffs_for_vanished_hashes() {
        let mut cache = DiffCache::new();
        let prev_commits = vec![sample_commit("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")];
        let mut prev_states = empty_states(prev_commits.len());
        prev_states[0].diff_loaded = true;
        cache.states = prev_states;

        let new_commits = vec![sample_commit("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")];
        let new_states = empty_states(new_commits.len());

        cache.replace(&prev_commits, &new_commits, new_states);

        assert!(!cache.states[0].diff_loaded);
    }
}

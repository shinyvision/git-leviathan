//! Read-only view of the latest projected repository state.
//!
//! `RepositorySnapshot` is what the presenter produced. Every field is data
//! the graph/sidebar/header rendering pipelines consume; nothing here is
//! user-controlled session state (selection, scroll, overlay). The snapshot
//! is replaced wholesale on `replace_loaded`, or narrowly on `apply_refs_update`
//! which is the fetch-path variant.

use std::collections::HashSet;

use std::path::Path;

use crate::{
    core::{Commit, RepoVersion, WorktreeInfo},
    services::presenter::signature::SignatureTracker,
    services::RepoRef,
    view_model::{
        CommitPresentation, GraphRow, LoadedRefs, LoadedRepo, RepositoryProjection, SidebarSection,
    },
};

pub(in crate::screens::repository) struct RepositorySnapshot {
    commits: Vec<Commit>,
    commit_presentations: Vec<CommitPresentation>,
    graph_rows: Vec<GraphRow>,
    sidebar_sections: Vec<SidebarSection>,
    num_lanes: usize,
    repo_name: String,
    current_branch: String,
    head_hash: Option<String>,
    default_remote_name: Option<String>,
    fast_forward_candidates: HashSet<String>,
    worktrees: Vec<WorktreeInfo>,
    /// Local + remote branch refs. Read by `App::sync_repository_to_plugins`
    /// to populate the plugin-host `leviathan.repository` tables.
    branch_refs: Vec<RepoRef>,
    tracker: SignatureTracker,
}

impl RepositorySnapshot {
    pub(in crate::screens::repository) fn loading_with_repo_name(repo_name: String) -> Self {
        Self {
            commits: vec![],
            commit_presentations: vec![],
            graph_rows: vec![],
            sidebar_sections: vec![],
            num_lanes: 6,
            repo_name,
            current_branch: String::new(),
            head_hash: None,
            default_remote_name: None,
            fast_forward_candidates: HashSet::new(),
            worktrees: Vec::new(),
            branch_refs: Vec::new(),
            tracker: SignatureTracker::new(),
        }
    }

    /// Clears commit/graph/sidebar view data but preserves identity fields
    /// (`repo_name`, `current_branch`, `head_hash`, `default_remote_name`).
    /// Used by tab hibernation: the repo on disk does not change while a tab
    /// sits in the background, so the main bar can keep showing the correct
    /// repo + branch instead of flashing "Loading…" during the async rehydrate.
    pub(in crate::screens::repository) fn hibernate(&mut self) {
        self.commits.clear();
        self.commit_presentations.clear();
        self.graph_rows.clear();
        self.sidebar_sections.clear();
        self.fast_forward_candidates.clear();
        self.worktrees.clear();
        self.branch_refs.clear();
        self.tracker = SignatureTracker::new();
    }

    /// Full-reload path. Absorbs the graph/sidebar fields; returns the
    /// `commit_diff_states` so callers can hand them to the diff cache.
    pub(in crate::screens::repository) fn replace_loaded(
        &mut self,
        loaded: LoadedRepo,
    ) -> Vec<crate::view_model::CommitDiffState> {
        let LoadedRepo {
            projection,
            signature,
            ..
        } = loaded;
        self.tracker.observe(signature);
        self.apply_projection(projection)
    }

    /// Fetch path. `repo_name`, `current_branch`, and the sidebar expansion
    /// set are intentionally NOT touched — a fetch cannot change any of them,
    /// and keeping them out of the mutation set is what lets an open overlay
    /// or in-progress commit message survive a fetch completion.
    pub(in crate::screens::repository) fn apply_refs_update(&mut self, refs: LoadedRefs) -> Vec<crate::view_model::CommitDiffState> {
        let LoadedRefs {
            commits,
            commit_presentations,
            commit_diff_states,
            graph_rows,
            sidebar_sections,
            num_lanes,
            head_hash,
            default_remote_name,
            fast_forward_candidates,
            signature,
            has_more_commits: _,
            branch_refs,
        } = refs;

        self.tracker.observe(signature);

        self.commits = commits;
        self.commit_presentations = commit_presentations;
        self.graph_rows = graph_rows;
        self.sidebar_sections = sidebar_sections;
        self.num_lanes = num_lanes;
        self.head_hash = head_hash;
        self.default_remote_name = default_remote_name;
        self.fast_forward_candidates = fast_forward_candidates;
        self.branch_refs = branch_refs;

        commit_diff_states
    }

    fn apply_projection(
        &mut self,
        projection: RepositoryProjection,
    ) -> Vec<crate::view_model::CommitDiffState> {
        let RepositoryProjection {
            commits,
            commit_presentations,
            commit_diff_states,
            graph_rows,
            sidebar_sections,
            num_lanes,
            repo_name,
            current_branch,
            head_hash,
            default_remote_name,
            fast_forward_candidates,
            worktrees,
            branch_refs,
        } = projection;

        self.commits = commits;
        self.commit_presentations = commit_presentations;
        self.graph_rows = graph_rows;
        self.sidebar_sections = sidebar_sections;
        self.num_lanes = num_lanes;
        self.repo_name = repo_name;
        self.current_branch = current_branch;
        self.head_hash = head_hash;
        self.default_remote_name = default_remote_name;
        self.fast_forward_candidates = fast_forward_candidates;
        self.worktrees = worktrees;
        self.branch_refs = branch_refs;

        commit_diff_states
    }

    pub(in crate::screens::repository) fn commits(&self) -> &[Commit] {
        &self.commits
    }

    pub(in crate::screens::repository) fn commit_presentations(&self) -> &[CommitPresentation] {
        &self.commit_presentations
    }

    pub(in crate::screens::repository) fn graph_rows(&self) -> &[GraphRow] {
        &self.graph_rows
    }

    pub(in crate::screens::repository) fn graph_revision(&self) -> RepoVersion {
        self.tracker.revision()
    }

    pub(in crate::screens::repository) fn sidebar_sections(&self) -> &[SidebarSection] {
        &self.sidebar_sections
    }

    pub(in crate::screens::repository) fn num_lanes(&self) -> usize {
        self.num_lanes
    }

    pub(in crate::screens::repository) fn repo_name(&self) -> &str {
        &self.repo_name
    }

    pub(in crate::screens::repository) fn current_branch(&self) -> &str {
        &self.current_branch
    }

    /// Local + remote branch refs from the latest projection. Exposed
    /// crate-wide so `App::sync_repository_to_plugins` can rebuild the
    /// plugin-host `leviathan.repository` tables without re-reading libgit2.
    pub(crate) fn branch_refs(&self) -> &[RepoRef] {
        &self.branch_refs
    }

    pub(in crate::screens::repository) fn head_hash(&self) -> Option<&str> {
        self.head_hash.as_deref()
    }

    pub(in crate::screens::repository) fn default_remote_name(&self) -> Option<&str> {
        self.default_remote_name.as_deref()
    }

    pub(in crate::screens::repository) fn can_fast_forward_to(&self, branch_name: &str) -> bool {
        self.fast_forward_candidates.contains(branch_name)
    }

    /// Path of whichever worktree (primary or secondary) currently has
    /// `branch_name` checked out. Used to route branch-checkout gestures into
    /// a focus-swap when the target branch already lives in another worktree:
    /// libgit2 happily re-checks-out the branch on the current worktree,
    /// which just drags its tree state onto the active workdir as a pile of
    /// uncommitted changes — almost never what the user wanted.
    pub(in crate::screens::repository) fn worktree_path_for_branch(
        &self,
        branch_name: &str,
    ) -> Option<&Path> {
        self.worktrees
            .iter()
            .find(|w| w.branch_name == branch_name)
            .map(|w| w.path.as_path())
    }

    /// Count of commits between HEAD (inclusive) and `target_hash` (exclusive)
    /// reachable via first-parent. Returns None if target not on chain.
    pub(in crate::screens::repository) fn first_parent_distance_from_head(
        &self,
        target_hash: &str,
    ) -> Option<usize> {
        let head = self.head_hash.as_deref()?;
        let mut count = 0usize;
        let mut current = head.to_string();
        loop {
            if current == target_hash {
                return Some(count);
            }
            let commit = self.commits.iter().find(|c| c.hash == current)?;
            let parent = commit.first_parent_hash.clone()?;
            current = parent;
            count += 1;
            if count > self.commits.len() + 1 {
                return None;
            }
        }
    }

}

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use crate::core::{Commit, CommitKind, WorktreeInfo};
use crate::services::{
    CommitSnapshot, DirtySnapshot, RefsSnapshot, RepoRef, RepoRefKind, RepoSnapshot, StashSnapshot,
};
use crate::utils::initials;
use crate::view_model::{
    Branch, BranchLabel, BranchLabelKind, CommitDiffState, CommitPresentation, GraphRow,
    GraphSegment, LoadedRefs, LoadedRepo, ProjectionSignature, RepositoryProjection,
    SidebarSection, SidebarSectionKind, SidebarStash,
};
use crate::widgets::branch_label::branch_display_rows;

mod format;
mod stash;

use format::{format_date, relative_time, short_hash};
pub use stash::stash_display_name;

const DIRTY_COMMIT_HASH: &str = "__git_leviathan_dirty__";

pub fn compute_signature(snapshot: &RepoSnapshot) -> ProjectionSignature {
    use std::collections::hash_map::DefaultHasher;

    let mut h = DefaultHasher::new();

    snapshot.commits.len().hash(&mut h);
    for commit in &snapshot.commits {
        commit.hash.hash(&mut h);
        commit.parent_hashes.len().hash(&mut h);
        for parent in &commit.parent_hashes {
            parent.hash(&mut h);
        }
    }

    snapshot.refs.len().hash(&mut h);
    for r in &snapshot.refs {
        r.name.hash(&mut h);
        r.target_hash.hash(&mut h);
        r.kind.hash(&mut h);
        r.is_current.hash(&mut h);
        r.remote_name.hash(&mut h);
        r.upstream_ref.hash(&mut h);
    }

    snapshot.stashes.len().hash(&mut h);
    for s in &snapshot.stashes {
        s.hash.hash(&mut h);
        s.parent_hash.hash(&mut h);
        s.index_hash.hash(&mut h);
    }

    snapshot.current_branch.hash(&mut h);
    snapshot.head_hash.hash(&mut h);

    snapshot.dirty.is_some().hash(&mut h);
    if let Some(d) = &snapshot.dirty {
        d.conflicted_files.len().hash(&mut h);
        d.staged_files.len().hash(&mut h);
        d.unstaged_files.len().hash(&mut h);
        for f in &d.conflicted_files {
            f.path.hash(&mut h);
        }
        for f in &d.staged_files {
            f.path.hash(&mut h);
        }
        for f in &d.unstaged_files {
            f.path.hash(&mut h);
        }
        for p in &d.parent_hashes {
            p.hash(&mut h);
        }
    }

    ProjectionSignature(h.finish())
}

pub fn project_loaded(snapshot: RepoSnapshot) -> LoadedRepo {
    let signature = compute_signature(&snapshot);
    let has_more_commits = snapshot.has_more_commits;
    LoadedRepo {
        projection: project_repo(snapshot),
        has_more_commits,
        signature,
    }
}

pub fn project_refs(snapshot: RefsSnapshot) -> LoadedRefs {
    // Rehydrate into a RepoSnapshot with the non-fetch fields zeroed out so
    // we can reuse `project_repo` — the graph/sidebar computation is the
    // same, we just drop `repo_name` and `current_branch` from the result.
    // `dirty` is kept because the working-tree row is part of the graph.
    let RefsSnapshot {
        commits,
        refs,
        dirty,
        stashes,
        head_hash,
        has_more_commits,
        default_remote_name,
        fast_forward_candidates,
        worktrees,
        active_worktree_path,
    } = snapshot;

    let repo_snapshot = RepoSnapshot {
        commits,
        refs,
        dirty,
        stashes,
        repo_name: String::new(),
        current_branch: None,
        head_hash,
        has_more_commits,
        default_remote_name,
        fast_forward_candidates,
        worktrees,
        active_worktree_path,
    };

    let signature = compute_signature(&repo_snapshot);
    let has_more_commits = repo_snapshot.has_more_commits;
    let projection = project_repo(repo_snapshot);
    let RepositoryProjection {
        commits,
        commit_presentations,
        commit_diff_states,
        graph_rows,
        sidebar_sections,
        num_lanes,
        head_hash,
        default_remote_name,
        fast_forward_candidates,
        repo_name: _,
        current_branch: _,
        worktrees: _,
        branch_refs,
    } = projection;

    LoadedRefs {
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
        has_more_commits,
        branch_refs,
    }
}

pub fn project_repo(snapshot: RepoSnapshot) -> RepositoryProjection {
    let RepoSnapshot {
        commits: commit_snapshots,
        refs,
        dirty,
        stashes,
        repo_name,
        current_branch,
        head_hash,
        has_more_commits: _,
        default_remote_name,
        fast_forward_candidates,
        worktrees,
        active_worktree_path: _,
    } = snapshot;

    let worktree_paths_by_branch: HashMap<&str, &std::path::PathBuf> = worktrees
        .iter()
        .filter(|w| !w.is_primary)
        .map(|w| (w.branch_name.as_str(), &w.path))
        .collect();

    let stash_index_by_hash: HashMap<String, usize> = stashes
        .iter()
        .map(|stash| (stash.hash.clone(), stash.stash_index))
        .collect();
    // Snapshot the pre-inject commit order so we can decide, per stash, whether
    // its parent is the bottom-most leaf in view or is a mid-branch commit.
    let original_commit_hashes: Vec<String> =
        commit_snapshots.iter().map(|c| c.hash.clone()).collect();
    // "Absorbed" stashes: parent is the last leaf in view, so the stash should
    // collapse onto the parent's lane with no arm. For these we inject WIP
    // with a single parent (no INDEX, no fake-merge parent swap), which means
    // compute_graph_rows won't place the base on a secondary lane and no
    // side-arm segment gets seeded on the base row.
    let absorbed_stash_hashes: HashSet<String> = stashes
        .iter()
        .filter(|stash| original_commit_hashes.last() == Some(&stash.parent_hash))
        .map(|stash| stash.hash.clone())
        .collect();
    let mut commit_snapshots = inject_stashes(commit_snapshots, &stashes, &absorbed_stash_hashes);
    let stash_hashes: HashSet<String> = stashes.iter().map(|stash| stash.hash.clone()).collect();
    let index_hashes: HashSet<String> = stashes
        .iter()
        .filter(|stash| !absorbed_stash_hashes.contains(&stash.hash))
        .filter_map(|stash| stash.index_hash.clone())
        .collect();
    // Non-absorbed stashes inject INDEX + swap WIP parents to `[index, base]`;
    // that fake-merge would otherwise promote the stash lane to mfp and pull
    // the main trunk off its leftmost lane onto the stash lane. Exempt their
    // hashes from mfp promotion so the trunk stays put.
    let mfp_exempt: HashSet<String> = stashes
        .iter()
        .filter(|stash| !absorbed_stash_hashes.contains(&stash.hash))
        .flat_map(|stash| std::iter::once(stash.hash.clone()).chain(stash.index_hash.clone()))
        .collect();

    let branch_refs: Vec<RepoRef> = refs
        .iter()
        .filter(|r| !matches!(r.kind, RepoRefKind::Tag))
        .cloned()
        .collect();

    let has_dirty_changes = dirty.as_ref().is_some_and(DirtySnapshot::has_changes);
    let dirty_parent_hashes = if has_dirty_changes {
        resolve_dirty_parent_hashes(&head_hash, &refs, &commit_snapshots, dirty.as_ref())
    } else {
        Vec::new()
    };
    let (mut graph_rows, num_lanes) = if has_dirty_changes {
        let graph_commits = graph_commits_with_dirty(&commit_snapshots, &dirty_parent_hashes);
        compute_graph_rows_with_exemptions(&graph_commits, &mfp_exempt)
    } else {
        compute_graph_rows_with_exemptions(&commit_snapshots, &mfp_exempt)
    };
    if has_dirty_changes {
        mark_dirty_graph_path(&mut graph_rows, &commit_snapshots, &dirty_parent_hashes);
    }

    // INDEX rows were injected only to reserve a lane for the stash's index
    // commit; they shouldn't surface as visible rows. Strip them from both
    // the snapshot list and the parallel graph_rows before any downstream
    // work treats them as real commits.
    let row_offset = if has_dirty_changes { 1 } else { 0 };
    let index_row_indices: Vec<usize> = commit_snapshots
        .iter()
        .enumerate()
        .filter(|(_, c)| index_hashes.contains(&c.hash))
        .map(|(i, _)| i)
        .collect();
    for &snapshot_idx in index_row_indices.iter().rev() {
        commit_snapshots.remove(snapshot_idx);
        graph_rows.remove(snapshot_idx + row_offset);
    }

    mark_stash_graph_paths(
        &mut graph_rows,
        &commit_snapshots,
        &stash_hashes,
        has_dirty_changes,
    );

    let mut commit_presentations = build_commit_presentations(&commit_snapshots, &refs);
    if has_dirty_changes {
        commit_presentations.insert(0, empty_commit_presentation());
    }

    for (presentation, row) in commit_presentations.iter_mut().zip(&graph_rows) {
        for label in &mut presentation.branch_labels {
            label.lane_color = row.commit_lane;
        }
        let mut display_rows = branch_display_rows(&presentation.branch_labels);
        for dr in &mut display_rows {
            dr.worktree_path = worktree_paths_by_branch
                .get(dr.name.as_str())
                .map(|p| (*p).clone());
        }
        presentation.branch_display_rows = display_rows;
    }

    let mut commits: Vec<Commit> = commit_snapshots
        .iter()
        .map(|snapshot| {
            if let Some(stash_index) = stash_index_by_hash.get(&snapshot.hash) {
                project_stash_commit(snapshot, *stash_index)
            } else {
                project_commit(snapshot)
            }
        })
        .collect();
    let mut commit_diff_states: Vec<CommitDiffState> = (0..commits.len())
        .map(|_| CommitDiffState::empty())
        .collect();

    if let Some(dirty) = dirty.filter(DirtySnapshot::has_changes) {
        let (dirty_commit, dirty_diff) = project_dirty_commit(dirty);
        commits.insert(0, dirty_commit);
        commit_diff_states.insert(0, dirty_diff);
    }

    RepositoryProjection {
        commits,
        commit_presentations,
        commit_diff_states,
        graph_rows,
        sidebar_sections: build_sidebar_sections(&refs, &stashes, &worktrees),
        num_lanes,
        repo_name,
        current_branch: current_branch.unwrap_or_else(|| "HEAD".to_string()),
        head_hash,
        default_remote_name,
        fast_forward_candidates,
        worktrees: worktrees.clone(),
        branch_refs,
    }
}

fn inject_stashes(
    mut commits: Vec<CommitSnapshot>,
    stashes: &[StashSnapshot],
    absorbed_stash_hashes: &HashSet<String>,
) -> Vec<CommitSnapshot> {
    for stash in stashes {
        let Some(parent_idx) = commits
            .iter()
            .position(|commit| commit.hash == stash.parent_hash)
        else {
            continue;
        };

        let is_absorbed = absorbed_stash_hashes.contains(&stash.hash);

        if !is_absorbed {
            if let Some(index_hash) = &stash.index_hash {
                commits.insert(
                    parent_idx,
                    CommitSnapshot {
                        hash: index_hash.clone(),
                        message: format!("index for stash@{{{}}}", stash.stash_index),
                        author_name: stash.author_name.clone(),
                        authored_at: stash.authored_at,
                        authored_offset_minutes: stash.authored_offset_minutes,
                        parent_hashes: vec![stash.parent_hash.clone()],
                    },
                );
            }
        }

        let parent_hashes = if is_absorbed {
            vec![stash.parent_hash.clone()]
        } else {
            match &stash.index_hash {
                Some(index_hash) => vec![index_hash.clone(), stash.parent_hash.clone()],
                None => vec![stash.parent_hash.clone()],
            }
        };
        commits.insert(
            parent_idx,
            CommitSnapshot {
                hash: stash.hash.clone(),
                message: stash.message.clone(),
                author_name: stash.author_name.clone(),
                authored_at: stash.authored_at,
                authored_offset_minutes: stash.authored_offset_minutes,
                parent_hashes,
            },
        );
    }
    commits
}

fn mark_stash_graph_paths(
    rows: &mut [GraphRow],
    commits: &[CommitSnapshot],
    stash_hashes: &HashSet<String>,
    has_dirty_changes: bool,
) {
    if stash_hashes.is_empty() {
        return;
    }

    let row_offset = if has_dirty_changes { 1 } else { 0 };

    let stash_positions: Vec<(usize, usize, Option<usize>)> = commits
        .iter()
        .enumerate()
        .filter(|(_, commit)| stash_hashes.contains(&commit.hash))
        .map(|(idx, commit)| {
            let row_idx = idx + row_offset;
            let lane = rows[row_idx].commit_lane;
            let parent_row_idx = commit.parent_hashes.iter().find_map(|parent_hash| {
                commits
                    .iter()
                    .enumerate()
                    .skip(idx + 1)
                    .find(|(_, c)| &c.hash == parent_hash)
                    .map(|(parent_idx, _)| parent_idx + row_offset)
            });
            (row_idx, lane, parent_row_idx)
        })
        .collect();

    for (stash_row_idx, stash_lane, parent_row_idx) in stash_positions {
        let stash_lane_f = stash_lane as f32;

        let ghost_lanes: Vec<f32> = rows[stash_row_idx]
            .connectors
            .iter()
            .map(|conn| conn.to_lane)
            .collect();

        let stop_idx = parent_row_idx.unwrap_or(rows.len().saturating_sub(1));
        let stash_collapses_onto_parent = parent_row_idx
            .map(|idx| rows[idx].commit_lane as f32 == stash_lane_f)
            .unwrap_or(false);

        {
            let row = &mut rows[stash_row_idx];
            row.is_stash = true;
            row.is_merge = false;
            row.connectors.clear();
        }

        {
            let row = &mut rows[stash_row_idx];
            move_lane_segments(&mut row.lower, &mut row.dotted_lower, stash_lane_f);
        }

        for (row_idx, row) in rows
            .iter_mut()
            .enumerate()
            .take(stop_idx + 1)
            .skip(stash_row_idx + 1)
        {
            let mut converged = false;

            let mut i = 0;
            while i < row.upper.len() {
                if row.upper[i].from_lane == stash_lane_f {
                    if row.upper[i].to_lane != stash_lane_f {
                        converged = true;
                    }
                    row.dotted_upper.push(row.upper.remove(i));
                } else {
                    i += 1;
                }
            }

            if converged || row_idx == stop_idx {
                break;
            }

            move_lane_segments(&mut row.lower, &mut row.dotted_lower, stash_lane_f);
        }

        if stash_collapses_onto_parent {
            for ghost_lane_f in &ghost_lanes {
                drop_ghost_lane_segments(&mut rows[stash_row_idx..=stop_idx], *ghost_lane_f);
            }
        }
    }
}

fn drop_ghost_lane_segments(rows: &mut [GraphRow], ghost_lane: f32) {
    for row in rows.iter_mut() {
        row.upper
            .retain(|s| !(s.from_lane == ghost_lane || s.to_lane == ghost_lane));
        row.dotted_upper
            .retain(|s| !(s.from_lane == ghost_lane || s.to_lane == ghost_lane));
        row.lower
            .retain(|s| !(s.from_lane == ghost_lane || s.to_lane == ghost_lane));
        row.dotted_lower
            .retain(|s| !(s.from_lane == ghost_lane || s.to_lane == ghost_lane));
        row.connectors
            .retain(|s| !(s.from_lane == ghost_lane || s.to_lane == ghost_lane));
        row.dotted_connectors
            .retain(|s| !(s.from_lane == ghost_lane || s.to_lane == ghost_lane));
    }
}

fn project_commit(snapshot: &CommitSnapshot) -> Commit {
    Commit {
        kind: CommitKind::Commit,
        hash: snapshot.hash.clone(),
        short_hash: short_hash(&snapshot.hash),
        message: snapshot.message.clone(),
        author: snapshot.author_name.clone(),
        date: format_date(snapshot.authored_at, snapshot.authored_offset_minutes),
        parent_hashes: snapshot.parent_hashes.clone(),
        is_merge_in_progress: false,
        conflicted_files: vec![],
        staged_files: vec![],
        unstaged_files: vec![],
    }
}

fn project_stash_commit(snapshot: &CommitSnapshot, stash_index: usize) -> Commit {
    Commit {
        kind: CommitKind::Stash,
        hash: snapshot.hash.clone(),
        short_hash: format!("stash@{{{}}}", stash_index),
        message: stash_display_name(&snapshot.message),
        author: snapshot.author_name.clone(),
        date: format_date(snapshot.authored_at, snapshot.authored_offset_minutes),
        parent_hashes: snapshot.parent_hashes.clone(),
        is_merge_in_progress: false,
        conflicted_files: vec![],
        staged_files: vec![],
        unstaged_files: vec![],
    }
}

fn project_dirty_commit(snapshot: DirtySnapshot) -> (Commit, CommitDiffState) {
    let DirtySnapshot {
        conflicted_files,
        staged_files,
        unstaged_files,
        parent_hashes,
    } = snapshot;
    let is_merge_in_progress = parent_hashes.len() >= 2;
    let files: Vec<_> = unstaged_files
        .iter()
        .chain(conflicted_files.iter())
        .chain(staged_files.iter())
        .cloned()
        .collect();
    let added_count = files
        .iter()
        .filter(|file| file.kind == crate::core::ChangeKind::Added)
        .count() as u32;
    let deleted_count = files
        .iter()
        .filter(|file| file.kind == crate::core::ChangeKind::Deleted)
        .count() as u32;
    let modified_count = files.len() as u32 - added_count - deleted_count;

    let commit = Commit {
        kind: CommitKind::Dirty,
        hash: String::new(),
        short_hash: "WIP".to_string(),
        message: "WIP".to_string(),
        author: "Working Tree".to_string(),
        date: "uncommitted changes".to_string(),
        parent_hashes: parent_hashes.clone(),
        is_merge_in_progress,
        conflicted_files,
        staged_files,
        unstaged_files,
    };
    // Dirty commits get their file list from the working-tree snapshot
    // itself, not from `load_commit_diff`, so the "diff" is already
    // resolved at projection time.
    let diff = CommitDiffState::from_dirty(files, modified_count, added_count, deleted_count);
    (commit, diff)
}

fn empty_commit_presentation() -> CommitPresentation {
    CommitPresentation {
        branch_labels: vec![],
        branch_display_rows: vec![],
        relative_time: None,
    }
}

fn build_commit_presentations(
    commits: &[CommitSnapshot],
    refs: &[RepoRef],
) -> Vec<CommitPresentation> {
    let ref_map = build_ref_map(refs);

    commits
        .iter()
        .map(|commit| {
            let branch_labels = ref_map.get(&commit.hash).cloned().unwrap_or_default();
            let branch_display_rows = branch_display_rows(&branch_labels);
            CommitPresentation {
                branch_display_rows,
                branch_labels,
                relative_time: relative_time(commit.authored_at),
            }
        })
        .collect()
}

fn resolve_dirty_parent_hash(
    head_hash: &Option<String>,
    refs: &[RepoRef],
    commits: &[CommitSnapshot],
) -> Option<String> {
    head_hash
        .clone()
        .or_else(|| {
            refs.iter()
                .find(|repo_ref| repo_ref.is_current)
                .map(|repo_ref| repo_ref.target_hash.clone())
        })
        .or_else(|| commits.first().map(|commit| commit.hash.clone()))
}

fn resolve_dirty_parent_hashes(
    head_hash: &Option<String>,
    refs: &[RepoRef],
    commits: &[CommitSnapshot],
    dirty: Option<&DirtySnapshot>,
) -> Vec<String> {
    let mut parent_hashes = dirty
        .map(|dirty| dirty.parent_hashes.clone())
        .unwrap_or_default();

    if parent_hashes.is_empty() {
        parent_hashes.extend(resolve_dirty_parent_hash(head_hash, refs, commits));
    }

    let mut deduped = Vec::new();
    for hash in parent_hashes {
        if !hash.is_empty() && !deduped.iter().any(|existing| existing == &hash) {
            deduped.push(hash);
        }
    }
    deduped
}

fn graph_commits_with_dirty(
    commits: &[CommitSnapshot],
    dirty_parent_hashes: &[String],
) -> Vec<CommitSnapshot> {
    let mut graph_commits = Vec::with_capacity(commits.len() + 1);
    graph_commits.push(CommitSnapshot {
        hash: DIRTY_COMMIT_HASH.to_string(),
        message: "WIP".to_string(),
        author_name: "Working Tree".to_string(),
        authored_at: 0,
        authored_offset_minutes: 0,
        parent_hashes: dirty_parent_hashes.to_vec(),
    });
    graph_commits.extend(commits.iter().cloned());
    graph_commits
}

fn mark_dirty_graph_path(
    rows: &mut [GraphRow],
    commits: &[CommitSnapshot],
    dirty_parent_hashes: &[String],
) {
    let Some(dirty_row) = rows.first_mut() else {
        return;
    };

    dirty_row.has_commit = false;
    dirty_row.author_initials.clear();
    dirty_row.is_merge = false;

    let dirty_lane = dirty_row.commit_lane as f32;
    let connector_lanes = dirty_row
        .connectors
        .iter()
        .map(|connector| connector.to_lane)
        .collect::<Vec<_>>();
    dirty_row
        .dotted_connectors
        .append(&mut dirty_row.connectors);

    let mut dirty_paths = Vec::new();
    let first_parent_row_idx = dirty_parent_hashes
        .first()
        .and_then(|hash| commit_row_idx(commits, hash))
        .unwrap_or_else(|| rows.len().saturating_sub(1));
    dirty_paths.push((dirty_lane, first_parent_row_idx));

    for (parent_hash, lane) in dirty_parent_hashes.iter().skip(1).zip(connector_lanes) {
        let parent_row_idx =
            commit_row_idx(commits, parent_hash).unwrap_or_else(|| rows.len().saturating_sub(1));
        dirty_paths.push((lane, parent_row_idx));
    }

    for (lane, last_dirty_path_idx) in dirty_paths {
        for (idx, row) in rows.iter_mut().enumerate().take(last_dirty_path_idx + 1) {
            if idx > 0 {
                move_lane_segments(&mut row.upper, &mut row.dotted_upper, lane);
            }
            if idx < last_dirty_path_idx {
                let has_incoming_connector = row.connectors.iter().any(|c| c.to_lane == lane);
                if has_incoming_connector {
                    break;
                }
                move_lane_segments(&mut row.lower, &mut row.dotted_lower, lane);
            }
        }
    }
}

fn commit_row_idx(commits: &[CommitSnapshot], hash: &str) -> Option<usize> {
    commits
        .iter()
        .position(|commit| commit.hash == hash)
        .map(|idx| idx + 1)
}

fn move_lane_segments(
    source: &mut Vec<GraphSegment>,
    destination: &mut Vec<GraphSegment>,
    lane: f32,
) {
    let mut idx = 0;
    while idx < source.len() {
        let segment = &source[idx];
        if segment.from_lane == lane && segment.to_lane == lane {
            destination.push(source.remove(idx));
        } else {
            idx += 1;
        }
    }
}

fn build_ref_map(refs: &[RepoRef]) -> HashMap<String, Vec<BranchLabel>> {
    let mut map = HashMap::new();

    for repo_ref in refs {
        let kind = match repo_ref.kind {
            RepoRefKind::LocalBranch if repo_ref.is_current => BranchLabelKind::CurrentLocal,
            RepoRefKind::LocalBranch => BranchLabelKind::Local,
            RepoRefKind::RemoteBranch => BranchLabelKind::Remote,
            RepoRefKind::Tag => BranchLabelKind::Tag,
        };

        map.entry(repo_ref.target_hash.clone())
            .or_insert_with(Vec::new)
            .push(BranchLabel {
                name: repo_ref.name.clone(),
                kind,
                lane_color: 0,
                remote_name: repo_ref.remote_name.clone(),
                upstream_ref: repo_ref.upstream_ref.clone(),
            });
    }

    map
}

pub fn build_sidebar_sections(
    refs: &[RepoRef],
    stashes: &[StashSnapshot],
    worktrees: &[WorktreeInfo],
) -> Vec<SidebarSection> {
    let current = refs.iter().find(|r| r.is_current);
    let current_hash = current.map(|r| r.target_hash.clone());
    let upstream_ref_name = current.and_then(|r| r.upstream_ref.clone());

    let mut local_branches = Vec::new();
    let mut remotes_map: HashMap<String, Vec<Branch>> = HashMap::new();
    let mut tags = Vec::new();

    for repo_ref in refs {
        match repo_ref.kind {
            RepoRefKind::LocalBranch => local_branches.push(Branch {
                name: repo_ref.name.clone(),
                is_current: repo_ref.is_current,
                children: vec![],
                worktree_path: None,
            }),
            RepoRefKind::RemoteBranch => {
                let remote_name = repo_ref
                    .remote_name
                    .clone()
                    .unwrap_or_else(|| "origin".to_string());
                let full_ref = format!("{}/{}", remote_name, repo_ref.name);
                let is_current = upstream_ref_name.as_ref() == Some(&full_ref)
                    && current_hash.as_ref() == Some(&repo_ref.target_hash);
                remotes_map.entry(remote_name).or_default().push(Branch {
                    name: repo_ref.name.clone(),
                    is_current,
                    children: vec![],
                    worktree_path: None,
                });
            }
            RepoRefKind::Tag => tags.push(Branch {
                name: repo_ref.name.clone(),
                is_current: false,
                children: vec![],
                worktree_path: None,
            }),
        }
    }

    local_branches.sort_by(|left, right| left.name.cmp(&right.name));

    let mut remote_section_branches: Vec<Branch> = remotes_map
        .into_iter()
        .map(|(remote_name, mut children)| {
            children.sort_by(|left, right| left.name.cmp(&right.name));
            Branch {
                name: remote_name,
                is_current: false,
                children,
                worktree_path: None,
            }
        })
        .collect();
    remote_section_branches.sort_by(|left, right| left.name.cmp(&right.name));

    tags.sort_by(|left, right| right.name.cmp(&left.name));

    let sidebar_stashes: Vec<SidebarStash> = stashes
        .iter()
        .map(|stash| SidebarStash {
            stash_index: stash.stash_index,
            hash: stash.hash.clone(),
            display_name: stash_display_name(&stash.message),
        })
        .collect();

    let worktree_entries: Vec<Branch> = worktrees
        .iter()
        .filter(|w| !w.is_primary)
        .map(|w| Branch {
            name: w.branch_name.clone(),
            is_current: false,
            children: Vec::new(),
            worktree_path: Some(w.path.clone()),
        })
        .collect();

    vec![
        SidebarSection {
            kind: SidebarSectionKind::Local,
            count: local_branches.len() as u32,
            branches: local_branches,
            stashes: vec![],
        },
        SidebarSection {
            kind: SidebarSectionKind::Remote,
            count: remote_section_branches
                .iter()
                .map(|branch| branch.children.len() as u32)
                .sum(),
            branches: remote_section_branches,
            stashes: vec![],
        },
        SidebarSection {
            kind: SidebarSectionKind::Worktrees,
            count: worktree_entries.len() as u32,
            branches: worktree_entries,
            stashes: vec![],
        },
        SidebarSection {
            kind: SidebarSectionKind::Stashes,
            count: sidebar_stashes.len() as u32,
            branches: vec![],
            stashes: sidebar_stashes,
        },
        SidebarSection {
            kind: SidebarSectionKind::Tags,
            count: tags.len() as u32,
            branches: tags,
            stashes: vec![],
        },
    ]
}

#[cfg(test)]
fn compute_graph_rows(commits: &[CommitSnapshot]) -> (Vec<GraphRow>, usize) {
    compute_graph_rows_with_exemptions(commits, &HashSet::new())
}

fn compute_graph_rows_with_exemptions(
    commits: &[CommitSnapshot],
    mfp_exempt: &HashSet<String>,
) -> (Vec<GraphRow>, usize) {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut mfp: Vec<bool> = Vec::new();
    let mut max_lanes = 1;
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let matching: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.as_deref() == Some(commit.hash.as_str()))
            .map(|(idx, _)| idx)
            .collect();

        let commit_lane = if !matching.is_empty() {
            matching
                .iter()
                .copied()
                .find(|&idx| mfp.get(idx).copied().unwrap_or(false))
                .unwrap_or(matching[0])
        } else {
            find_free_slot_mfp(&mut lanes, &mut mfp)
        };

        let upper = (0..lanes.len())
            .filter_map(|idx| {
                lanes[idx].as_ref()?;
                let to_lane = if matching.contains(&idx) {
                    commit_lane
                } else {
                    idx
                };
                Some(GraphSegment {
                    from_lane: idx as f32,
                    to_lane: to_lane as f32,
                })
            })
            .collect();

        let was_mfp = mfp.get(commit_lane).copied().unwrap_or(false);
        let is_merge = commit.parent_hashes.len() >= 2;

        for (idx, slot) in lanes.iter_mut().enumerate() {
            if slot.as_deref() == Some(commit.hash.as_str()) {
                *slot = None;
                if idx != commit_lane {
                    mfp[idx] = false;
                }
            }
        }

        let is_exempt = mfp_exempt.contains(&commit.hash);
        if let Some(first_parent) = commit.parent_hashes.first() {
            lanes[commit_lane] = Some(first_parent.clone());
            mfp[commit_lane] = if is_exempt {
                was_mfp
            } else {
                is_merge || was_mfp
            };
        } else {
            mfp[commit_lane] = false;
        }

        let mut connectors = Vec::new();
        for parent_hash in commit.parent_hashes.iter().skip(1) {
            let slot = match lanes
                .iter()
                .position(|lane| lane.as_deref() == Some(parent_hash.as_str()))
            {
                Some(existing) => existing,
                None => {
                    let slot = find_free_slot_mfp(&mut lanes, &mut mfp);
                    lanes[slot] = Some(parent_hash.clone());
                    mfp[slot] = false;
                    slot
                }
            };

            if slot != commit_lane {
                connectors.push(GraphSegment {
                    from_lane: commit_lane as f32,
                    to_lane: slot as f32,
                });
            }
        }

        while lanes.last().is_some_and(|slot| slot.is_none()) {
            lanes.pop();
            mfp.pop();
        }

        max_lanes = max_lanes.max(lanes.len().max(1));

        let lower = (0..lanes.len())
            .filter_map(|idx| {
                lanes[idx].as_ref()?;
                Some(GraphSegment {
                    from_lane: idx as f32,
                    to_lane: idx as f32,
                })
            })
            .collect();

        rows.push(GraphRow {
            upper,
            lower,
            dotted_upper: vec![],
            dotted_lower: vec![],
            dotted_connectors: vec![],
            commit_lane,
            author_initials: initials(&commit.author_name),
            is_merge: commit.parent_hashes.len() >= 2,
            connectors,
            has_commit: true,
            is_stash: false,
        });
    }

    (rows, max_lanes)
}

fn find_free_slot(lanes: &mut Vec<Option<String>>) -> usize {
    if let Some(idx) = lanes.iter().position(|slot| slot.is_none()) {
        idx
    } else {
        lanes.push(None);
        lanes.len() - 1
    }
}

fn find_free_slot_mfp(lanes: &mut Vec<Option<String>>, mfp: &mut Vec<bool>) -> usize {
    let slot = find_free_slot(lanes);
    while mfp.len() < lanes.len() {
        mfp.push(false);
    }
    slot
}

#[cfg(test)]
mod tests {
    use super::{build_ref_map, compute_graph_rows, format::relative_time_from_diff, project_repo};
    use crate::{
        core::{ChangeKind, ChangedFile, CommitKind},
        services::{CommitSnapshot, DirtySnapshot, RepoRef, RepoRefKind, RepoSnapshot},
        view_model::{BranchLabelKind, GraphRow, GraphSegment},
    };

    fn commit(hash: &str, parent_hashes: &[&str], message: &str) -> CommitSnapshot {
        CommitSnapshot {
            hash: hash.to_string(),
            message: message.to_string(),
            author_name: "Graph Test".to_string(),
            authored_at: 0,
            authored_offset_minutes: 0,
            parent_hashes: parent_hashes.iter().map(|hash| hash.to_string()).collect(),
        }
    }

    fn has_segment(segments: &[GraphSegment], from_lane: usize, to_lane: usize) -> bool {
        segments.iter().any(|segment| {
            segment.from_lane == from_lane as f32 && segment.to_lane == to_lane as f32
        })
    }

    #[test]
    fn main_first_parent_chain_stays_on_its_lane_below_merge() {
        use crate::services::StashSnapshot;
        let mywork_tip = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa01";
        let yep = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa02";
        let merge = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa05";
        let mywork_commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa06";
        let temp1 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa07";
        let good = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa09";
        let b3 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa10";
        let older = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa11";
        let stash_wip = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa20";
        let stash_index = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa21";

        let projection = project_repo(RepoSnapshot {
            commits: vec![
                commit(mywork_tip, &[yep], "Alright"),
                commit(yep, &[mywork_commit], "yep"),
                commit(merge, &[temp1, mywork_commit], "Merge"),
                commit(mywork_commit, &[temp1], "my work"),
                commit(temp1, &[good], "temp"),
                commit(good, &[b3], "GOOD!"),
                commit(b3, &[older], "temp"),
                commit(older, &[], "older"),
            ],
            refs: vec![
                RepoRef {
                    name: "my-work".into(),
                    kind: RepoRefKind::LocalBranch,
                    target_hash: mywork_tip.into(),
                    remote_name: None,
                    is_current: true,
                    upstream_ref: None,
                },
                RepoRef {
                    name: "main".into(),
                    kind: RepoRefKind::LocalBranch,
                    target_hash: merge.into(),
                    remote_name: None,
                    is_current: false,
                    upstream_ref: None,
                },
            ],
            dirty: None,
            stashes: vec![StashSnapshot {
                stash_index: 0,
                hash: stash_wip.into(),
                parent_hash: b3.into(),
                index_hash: Some(stash_index.into()),
                message: "WIP on main".into(),
                author_name: "T".into(),
                authored_at: 0,
                authored_offset_minutes: 0,
            }],
            repo_name: "r".into(),
            current_branch: Some("my-work".into()),
            head_hash: Some(mywork_tip.into()),
            has_more_commits: false,
            ..Default::default()
        });

        let rows_by_hash = |hash: &str| -> &GraphRow {
            projection
                .graph_rows
                .iter()
                .zip(projection.commits.iter())
                .find(|(_, c)| c.hash == hash)
                .map(|(row, _)| row)
                .expect("row exists")
        };

        assert_eq!(rows_by_hash(merge).commit_lane, 1);
        assert_eq!(rows_by_hash(mywork_commit).commit_lane, 0);
        assert_eq!(rows_by_hash(temp1).commit_lane, 1);
        assert_eq!(rows_by_hash(good).commit_lane, 1);
        assert_eq!(rows_by_hash(b3).commit_lane, 1);
        let stash_wip_row = projection
            .graph_rows
            .iter()
            .zip(projection.commits.iter())
            .find(|(_, c)| c.short_hash == "stash@{0}")
            .map(|(row, _)| row)
            .expect("stash row");
        assert!(stash_wip_row.is_stash);
        assert_eq!(stash_wip_row.commit_lane, 0);
        assert!(!stash_wip_row.dotted_lower.is_empty());
    }

    #[test]
    fn stash_to_the_right_of_main_trunk_does_not_pull_trunk_off_its_lane() {
        use crate::services::StashSnapshot;
        let head = "23396b48655745b85d8cffe63211ca10790048ab";
        let base = "b4f863dbad0564383cb59302d773fb7643d7c392";
        let older = "b03a42900000000000000000000000000000000a";
        let stash_wip = "441e65330e302e65be6c43b006170f7d938ac383";
        let stash_index = "c66ca00623cae085e5cb83546e14295a5205bf9f";

        let projection = project_repo(RepoSnapshot {
            commits: vec![
                commit(head, &[base], "Fix sidebar expansions"),
                commit(base, &[older], "sidebar"),
                commit(older, &[], "temp"),
            ],
            refs: vec![RepoRef {
                name: "main".into(),
                kind: RepoRefKind::LocalBranch,
                target_hash: head.into(),
                remote_name: None,
                is_current: true,
                upstream_ref: None,
            }],
            dirty: None,
            stashes: vec![StashSnapshot {
                stash_index: 0,
                hash: stash_wip.into(),
                parent_hash: base.into(),
                index_hash: Some(stash_index.into()),
                message: "WIP on main".into(),
                author_name: "T".into(),
                authored_at: 0,
                authored_offset_minutes: 0,
            }],
            repo_name: "r".into(),
            current_branch: Some("main".into()),
            head_hash: Some(head.into()),
            has_more_commits: false,
            ..Default::default()
        });

        let row_by_hash = |hash: &str| -> &GraphRow {
            projection
                .graph_rows
                .iter()
                .zip(projection.commits.iter())
                .find(|(_, c)| c.hash == hash)
                .map(|(row, _)| row)
                .expect("row exists")
        };

        assert_eq!(row_by_hash(head).commit_lane, 0);
        assert_eq!(row_by_hash(base).commit_lane, 0);
        let stash_row = projection
            .graph_rows
            .iter()
            .zip(projection.commits.iter())
            .find(|(_, c)| c.short_hash == "stash@{0}")
            .map(|(row, _)| row)
            .expect("stash row");
        assert!(stash_row.is_stash);
        assert_eq!(stash_row.commit_lane, 1);

        let base_row = row_by_hash(base);
        for segment in base_row
            .upper
            .iter()
            .chain(base_row.connectors.iter())
            .chain(base_row.dotted_connectors.iter())
        {
            assert!(
                !(segment.from_lane == 1.0 && segment.to_lane == 0.0),
                "trunk parent row must not have a solid stash-lane arm (got {:?})",
                segment
            );
        }
        assert!(
            base_row
                .dotted_upper
                .iter()
                .any(|s| s.from_lane == 1.0 && s.to_lane == 0.0),
            "trunk parent row must keep a dotted 1→0 ghost arm"
        );
        for segment in stash_row.connectors.iter().chain(stash_row.upper.iter()) {
            assert!(
                !(segment.from_lane == 1.0 && segment.to_lane == 0.0
                    || segment.from_lane == 0.0 && segment.to_lane == 1.0),
                "stash row must not draw a solid merge arm (got {:?})",
                segment
            );
        }
    }

    #[test]
    fn stash_at_top_of_view_with_mid_branch_parent_has_no_solid_arm() {
        use crate::services::StashSnapshot;
        let base = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let older = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let stash_wip = "cccccccccccccccccccccccccccccccccccccccc";
        let stash_index = "dddddddddddddddddddddddddddddddddddddddd";

        let projection = project_repo(RepoSnapshot {
            commits: vec![
                commit(base, &[older], "R5"),
                commit(older, &[], "older — trunk continues past R5"),
            ],
            refs: vec![RepoRef {
                name: "main".into(),
                kind: RepoRefKind::LocalBranch,
                target_hash: base.into(),
                remote_name: None,
                is_current: true,
                upstream_ref: None,
            }],
            dirty: None,
            stashes: vec![StashSnapshot {
                stash_index: 0,
                hash: stash_wip.into(),
                parent_hash: base.into(),
                index_hash: Some(stash_index.into()),
                message: "WIP".into(),
                author_name: "T".into(),
                authored_at: 0,
                authored_offset_minutes: 0,
            }],
            repo_name: "r".into(),
            current_branch: Some("main".into()),
            head_hash: Some(base.into()),
            has_more_commits: false,
            ..Default::default()
        });

        let row_by_hash = |hash: &str| -> &GraphRow {
            projection
                .graph_rows
                .iter()
                .zip(projection.commits.iter())
                .find(|(_, c)| c.hash == hash)
                .map(|(row, _)| row)
                .expect("row exists")
        };

        let stash_row = projection
            .graph_rows
            .iter()
            .zip(projection.commits.iter())
            .find(|(_, c)| c.short_hash == "stash@{0}")
            .map(|(row, _)| row)
            .expect("stash row");

        assert_eq!(stash_row.commit_lane, 0);
        assert_eq!(row_by_hash(base).commit_lane, 0);

        let base_row = row_by_hash(base);
        for segment in base_row
            .upper
            .iter()
            .chain(base_row.connectors.iter())
            .chain(base_row.dotted_connectors.iter())
        {
            assert!(
                !(segment.from_lane == 1.0 && segment.to_lane == 0.0),
                "R5 row must not have a solid 1→0 arm (got {:?})",
                segment
            );
        }
        for segment in stash_row
            .connectors
            .iter()
            .chain(stash_row.upper.iter())
            .chain(stash_row.lower.iter())
        {
            assert!(
                segment.from_lane != 1.0 && segment.to_lane != 1.0,
                "stash row must not leave a lane-1 solid segment (got {:?})",
                segment
            );
        }
        assert!(
            !stash_row.dotted_lower.is_empty(),
            "stash must keep its dotted tail"
        );
        assert!(
            base_row
                .dotted_upper
                .iter()
                .any(|s| s.from_lane == 0.0 && s.to_lane == 0.0),
            "R5 row must have dotted passthrough linking to the stash above"
        );
    }

    #[test]
    fn stash_attached_to_last_leaf_collapses_onto_same_lane() {
        use crate::services::StashSnapshot;
        let leaf = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let stash_wip = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let stash_index = "cccccccccccccccccccccccccccccccccccccccc";

        let projection = project_repo(RepoSnapshot {
            commits: vec![commit(leaf, &[], "R5 — last leaf")],
            refs: vec![RepoRef {
                name: "main".into(),
                kind: RepoRefKind::LocalBranch,
                target_hash: leaf.into(),
                remote_name: None,
                is_current: true,
                upstream_ref: None,
            }],
            dirty: None,
            stashes: vec![StashSnapshot {
                stash_index: 0,
                hash: stash_wip.into(),
                parent_hash: leaf.into(),
                index_hash: Some(stash_index.into()),
                message: "WIP".into(),
                author_name: "T".into(),
                authored_at: 0,
                authored_offset_minutes: 0,
            }],
            repo_name: "r".into(),
            current_branch: Some("main".into()),
            head_hash: Some(leaf.into()),
            has_more_commits: false,
            ..Default::default()
        });

        let row_by_hash = |hash: &str| -> &GraphRow {
            projection
                .graph_rows
                .iter()
                .zip(projection.commits.iter())
                .find(|(_, c)| c.hash == hash)
                .map(|(row, _)| row)
                .expect("row exists")
        };

        assert_eq!(row_by_hash(leaf).commit_lane, 0);
        let stash_row = projection
            .graph_rows
            .iter()
            .zip(projection.commits.iter())
            .find(|(_, c)| c.short_hash == "stash@{0}")
            .map(|(row, _)| row)
            .expect("stash row");
        assert_eq!(stash_row.commit_lane, 0);

        assert_eq!(
            projection.num_lanes, 1,
            "absorbed stash must not create a second lane"
        );
        assert!(
            stash_row.connectors.is_empty(),
            "stash row must have no merge connectors"
        );
        let leaf_row = row_by_hash(leaf);
        for segment in leaf_row.upper.iter().chain(leaf_row.dotted_upper.iter()) {
            assert!(
                segment.from_lane <= 0.0 && segment.to_lane <= 0.0,
                "leaf row must not have any segment touching lane 1 (got {:?})",
                segment
            );
        }
    }

    #[test]
    fn project_repo_prefixes_dirty_state_as_blank_graph_row() {
        let head_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let projection = project_repo(RepoSnapshot {
            commits: vec![commit(head_hash, &[], "head commit")],
            refs: vec![],
            dirty: Some(DirtySnapshot {
                conflicted_files: vec![],
                staged_files: vec![ChangedFile {
                    path: "staged.txt".to_string(),
                    kind: ChangeKind::Added,
                }],
                unstaged_files: vec![ChangedFile {
                    path: "unstaged.txt".to_string(),
                    kind: ChangeKind::Modified,
                }],
                parent_hashes: vec![head_hash.to_string()],
            }),
            repo_name: "repo".to_string(),
            current_branch: Some("main".to_string()),
            head_hash: Some(head_hash.to_string()),
            stashes: vec![],
            has_more_commits: false,
            ..Default::default()
        });

        assert_eq!(projection.commits.len(), 2);
        assert_eq!(projection.commits[0].kind, CommitKind::Dirty);
        assert_eq!(projection.commits[0].message, "WIP");
        assert!(!projection.commits[0].is_merge_in_progress);
        assert_eq!(projection.commits[0].staged_files.len(), 1);
        assert_eq!(projection.commits[0].unstaged_files.len(), 1);
        assert_eq!(projection.commits[1].hash, head_hash);

        assert_eq!(projection.graph_rows.len(), 2);
        assert!(!projection.graph_rows[0].has_commit);
        assert!(projection.graph_rows[0].upper.is_empty());
        assert!(projection.graph_rows[0].lower.is_empty());
        assert_eq!(projection.graph_rows[0].dotted_lower.len(), 1);
        assert!(projection.graph_rows[1].has_commit);

        assert_eq!(projection.commit_presentations.len(), 2);
        assert!(projection.commit_presentations[0].branch_labels.is_empty());
        assert!(projection.commit_presentations[0].relative_time.is_none());

        assert_eq!(projection.commit_diff_states.len(), 2);
        assert!(projection.commit_diff_states[0].diff_loaded);
        assert_eq!(projection.commit_diff_states[0].files.len(), 2);
        assert!(!projection.commit_diff_states[1].diff_loaded);
    }

    #[test]
    fn project_repo_reserves_lane_zero_for_dirty_current_branch_before_other_tips() {
        let head_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let other_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let shared_hash = "cccccccccccccccccccccccccccccccccccccccc";
        let projection = project_repo(RepoSnapshot {
            commits: vec![
                commit(other_hash, &[shared_hash], "newer other tip"),
                commit(head_hash, &[shared_hash], "current branch head"),
                commit(shared_hash, &[], "shared parent"),
            ],
            refs: vec![
                RepoRef {
                    name: "main".to_string(),
                    kind: RepoRefKind::LocalBranch,
                    target_hash: head_hash.to_string(),
                    remote_name: None,
                    is_current: true,
                    upstream_ref: None,
                },
                RepoRef {
                    name: "feature".to_string(),
                    kind: RepoRefKind::LocalBranch,
                    target_hash: other_hash.to_string(),
                    remote_name: None,
                    is_current: false,
                    upstream_ref: None,
                },
            ],
            dirty: Some(DirtySnapshot {
                conflicted_files: vec![],
                staged_files: vec![ChangedFile {
                    path: "staged.txt".to_string(),
                    kind: ChangeKind::Added,
                }],
                unstaged_files: vec![],
                parent_hashes: vec![head_hash.to_string()],
            }),
            repo_name: "repo".to_string(),
            current_branch: Some("main".to_string()),
            head_hash: Some(head_hash.to_string()),
            stashes: vec![],
            has_more_commits: false,
            ..Default::default()
        });

        assert_eq!(projection.num_lanes, 2);
        assert_eq!(projection.graph_rows[0].commit_lane, 0);
        assert!(!projection.graph_rows[0].has_commit);
        assert_eq!(projection.graph_rows[1].commit_lane, 1);
        assert_eq!(projection.graph_rows[2].commit_lane, 0);
        assert!(has_segment(&projection.graph_rows[0].dotted_lower, 0, 0));
        assert!(has_segment(&projection.graph_rows[1].dotted_upper, 0, 0));
        assert!(has_segment(&projection.graph_rows[1].dotted_lower, 0, 0));
        assert!(has_segment(&projection.graph_rows[2].dotted_upper, 0, 0));
        assert!(!has_segment(&projection.graph_rows[2].dotted_lower, 0, 0));
    }

    #[test]
    fn project_repo_draws_pending_dirty_merge_as_dotted_merge_lane() {
        let develop_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let feature_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let shared_hash = "cccccccccccccccccccccccccccccccccccccccc";
        let projection = project_repo(RepoSnapshot {
            commits: vec![
                commit(feature_hash, &[shared_hash], "feature tip"),
                commit(develop_hash, &[shared_hash], "develop head"),
                commit(shared_hash, &[], "shared parent"),
            ],
            refs: vec![
                RepoRef {
                    name: "develop".to_string(),
                    kind: RepoRefKind::LocalBranch,
                    target_hash: develop_hash.to_string(),
                    remote_name: None,
                    is_current: true,
                    upstream_ref: None,
                },
                RepoRef {
                    name: "feature/a".to_string(),
                    kind: RepoRefKind::LocalBranch,
                    target_hash: feature_hash.to_string(),
                    remote_name: None,
                    is_current: false,
                    upstream_ref: None,
                },
            ],
            dirty: Some(DirtySnapshot {
                conflicted_files: vec![ChangedFile {
                    path: "conflicted.txt".to_string(),
                    kind: ChangeKind::Modified,
                }],
                staged_files: vec![],
                unstaged_files: vec![],
                parent_hashes: vec![develop_hash.to_string(), feature_hash.to_string()],
            }),
            repo_name: "repo".to_string(),
            current_branch: Some("develop".to_string()),
            head_hash: Some(develop_hash.to_string()),
            stashes: vec![],
            has_more_commits: false,
            ..Default::default()
        });

        assert_eq!(projection.num_lanes, 2);
        assert!(projection.commits[0].is_merge_in_progress);
        assert!(!projection.graph_rows[0].has_commit);
        assert!(projection.graph_rows[0].connectors.is_empty());
        assert!(has_segment(
            &projection.graph_rows[0].dotted_connectors,
            0,
            1
        ));
        assert!(has_segment(&projection.graph_rows[0].dotted_lower, 0, 0));
        assert!(has_segment(&projection.graph_rows[0].dotted_lower, 1, 1));
        assert!(has_segment(&projection.graph_rows[1].dotted_upper, 1, 1));
        assert!(!has_segment(&projection.graph_rows[1].dotted_lower, 1, 1));
        assert!(has_segment(&projection.graph_rows[2].dotted_upper, 0, 0));
    }

    #[test]
    fn compute_graph_rows_keeps_leftmost_lane_continuous_for_shared_first_parent() {
        let commits = vec![
            commit(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"],
                "left tip",
            ),
            commit(
                "cccccccccccccccccccccccccccccccccccccccc",
                &["dddddddddddddddddddddddddddddddddddddddd"],
                "right tip",
            ),
            commit(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                &["dddddddddddddddddddddddddddddddddddddddd"],
                "left trunk",
            ),
            commit(
                "dddddddddddddddddddddddddddddddddddddddd",
                &[],
                "shared parent",
            ),
        ];

        let (rows, max_lanes) = compute_graph_rows(&commits);

        assert_eq!(max_lanes, 2);
        assert_eq!(rows[2].commit_lane, 0);
        assert!(has_segment(&rows[2].lower, 0, 0));
        assert!(has_segment(&rows[2].lower, 1, 1));
        assert!(has_segment(&rows[3].upper, 1, 0));
    }

    #[test]
    fn compute_graph_rows_keeps_existing_leftmost_lane_when_current_lane_is_to_the_right() {
        let commits = vec![
            commit(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &["dddddddddddddddddddddddddddddddddddddddd"],
                "left tip",
            ),
            commit(
                "cccccccccccccccccccccccccccccccccccccccc",
                &["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"],
                "right tip",
            ),
            commit(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                &["dddddddddddddddddddddddddddddddddddddddd"],
                "right trunk",
            ),
            commit(
                "dddddddddddddddddddddddddddddddddddddddd",
                &[],
                "shared parent",
            ),
        ];

        let (rows, max_lanes) = compute_graph_rows(&commits);

        assert_eq!(max_lanes, 2);
        assert_eq!(rows[2].commit_lane, 1);
        assert!(has_segment(&rows[2].lower, 0, 0));
        assert!(has_segment(&rows[2].lower, 1, 1));
        assert!(has_segment(&rows[3].upper, 1, 0));
    }

    #[test]
    fn compute_graph_rows_draws_merge_connector_for_existing_secondary_parent_lane() {
        let commits = vec![
            commit(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &["cccccccccccccccccccccccccccccccccccccccc"],
                "main tip",
            ),
            commit(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                &["eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"],
                "side tip",
            ),
            commit(
                "cccccccccccccccccccccccccccccccccccccccc",
                &[
                    "dddddddddddddddddddddddddddddddddddddddd",
                    "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                ],
                "merge commit",
            ),
            commit(
                "dddddddddddddddddddddddddddddddddddddddd",
                &["eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"],
                "main parent",
            ),
            commit(
                "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                &[],
                "shared parent",
            ),
        ];

        let (rows, max_lanes) = compute_graph_rows(&commits);

        assert_eq!(max_lanes, 2);
        assert_eq!(rows[2].commit_lane, 0);
        assert!(has_segment(&rows[2].connectors, 0, 1));
        assert!(has_segment(&rows[2].lower, 1, 1));
    }

    #[test]
    fn relative_time_uses_week_month_and_year_buckets() {
        const DAY: i64 = 24 * 60 * 60;

        let cases = [
            (7 * DAY, "1 week ago"),
            (14 * DAY, "2 weeks ago"),
            (30 * DAY, "1 month ago"),
            (61 * DAY, "2 months ago"),
            (365 * DAY, "1 year ago"),
            (730 * DAY, "2 years ago"),
        ];

        for (diff, expected) in cases {
            assert_eq!(relative_time_from_diff(diff), expected);
        }
    }

    #[test]
    fn dotted_path_stops_at_row_with_incoming_connector_to_dirty_lane() {
        let p_hash = "pppppppppppppppppppppppppppppppppppppppp";
        let m_hash = "mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm";
        let m_first_hash = "ffffffffffffffffffffffffffffffffffffffff";

        let projection = project_repo(RepoSnapshot {
            commits: vec![
                commit(m_hash, &[m_first_hash, p_hash], "merge into testing"),
                commit(p_hash, &[], "hotfix parent"),
            ],
            refs: vec![RepoRef {
                name: "hotfix".to_string(),
                kind: RepoRefKind::LocalBranch,
                target_hash: p_hash.to_string(),
                remote_name: None,
                is_current: true,
                upstream_ref: None,
            }],
            dirty: Some(DirtySnapshot {
                conflicted_files: vec![],
                staged_files: vec![ChangedFile {
                    path: "file.txt".to_string(),
                    kind: ChangeKind::Modified,
                }],
                unstaged_files: vec![],
                parent_hashes: vec![p_hash.to_string()],
            }),
            repo_name: "repo".to_string(),
            current_branch: Some("hotfix".to_string()),
            head_hash: Some(p_hash.to_string()),
            stashes: vec![],
            has_more_commits: false,
            ..Default::default()
        });

        assert_eq!(projection.graph_rows.len(), 3);

        let row_wip = &projection.graph_rows[0];
        let row_m = &projection.graph_rows[1];
        let row_p = &projection.graph_rows[2];

        assert!(has_segment(&row_wip.dotted_lower, 0, 0));
        assert!(!has_segment(&row_wip.lower, 0, 0));

        assert!(has_segment(&row_m.dotted_upper, 0, 0));
        assert!(!has_segment(&row_m.dotted_lower, 0, 0));
        assert!(has_segment(&row_m.lower, 0, 0));

        assert!(!has_segment(&row_p.dotted_upper, 0, 0));
        assert!(has_segment(&row_p.upper, 0, 0));
    }

    #[test]
    fn compute_graph_rows_draws_first_parent_fork_on_parent_row() {
        let commits = vec![
            commit(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                &["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"],
                "side branch",
            ),
            commit(
                "cccccccccccccccccccccccccccccccccccccccc",
                &["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"],
                "trunk",
            ),
            commit("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", &[], "parent"),
        ];

        let (rows, max_lanes) = compute_graph_rows(&commits);

        assert_eq!(max_lanes, 2);
        assert_eq!(rows[2].commit_lane, 0);
        assert!(has_segment(&rows[1].lower, 1, 1));
        assert!(has_segment(&rows[2].upper, 1, 0));
    }

    #[test]
    fn relative_time_just_now_below_one_minute() {
        assert_eq!(relative_time_from_diff(0), "just now");
        assert_eq!(relative_time_from_diff(59), "just now");
    }

    #[test]
    fn relative_time_singular_plural_minute_forms() {
        assert_eq!(relative_time_from_diff(60), "1 minute ago");
        assert_eq!(relative_time_from_diff(120), "2 minutes ago");
        assert_eq!(relative_time_from_diff(59 * 60), "59 minutes ago");
    }

    #[test]
    fn relative_time_hour_boundaries() {
        assert_eq!(relative_time_from_diff(60 * 60), "1 hour ago");
        assert_eq!(relative_time_from_diff(2 * 60 * 60), "2 hours ago");
        assert_eq!(relative_time_from_diff(23 * 60 * 60), "23 hours ago");
    }

    #[test]
    fn relative_time_day_and_yesterday() {
        assert_eq!(relative_time_from_diff(24 * 60 * 60), "yesterday");
        assert_eq!(relative_time_from_diff(2 * 24 * 60 * 60), "2 days ago");
        assert_eq!(relative_time_from_diff(6 * 24 * 60 * 60), "6 days ago");
    }

    #[test]
    fn relative_time_week_month_year_units() {
        let day = 24 * 60 * 60;
        assert_eq!(relative_time_from_diff(7 * day), "1 week ago");
        assert_eq!(relative_time_from_diff(30 * day), "1 month ago");
        assert_eq!(relative_time_from_diff(365 * day), "1 year ago");
        assert_eq!(relative_time_from_diff(2 * 365 * day), "2 years ago");
    }

    #[test]
    fn build_ref_map_groups_multiple_refs_on_same_commit() {
        let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let refs = vec![
            RepoRef {
                name: "main".into(),
                kind: RepoRefKind::LocalBranch,
                target_hash: hash.into(),
                remote_name: None,
                is_current: true,
                upstream_ref: None,
            },
            RepoRef {
                name: "v1.0".into(),
                kind: RepoRefKind::Tag,
                target_hash: hash.into(),
                remote_name: None,
                is_current: false,
                upstream_ref: None,
            },
        ];
        let map = build_ref_map(&refs);
        let labels = map.get(hash).expect("hash bucket present");
        assert_eq!(labels.len(), 2);
        assert!(labels
            .iter()
            .any(|l| matches!(l.kind, BranchLabelKind::CurrentLocal)));
        assert!(labels
            .iter()
            .any(|l| matches!(l.kind, BranchLabelKind::Tag)));
    }

    #[test]
    fn build_ref_map_marks_current_branch_as_current_local() {
        let refs = vec![RepoRef {
            name: "feature".into(),
            kind: RepoRefKind::LocalBranch,
            target_hash: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            remote_name: None,
            is_current: true,
            upstream_ref: None,
        }];
        let map = build_ref_map(&refs);
        let labels = &map["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"];
        assert_eq!(labels.len(), 1);
        assert!(matches!(labels[0].kind, BranchLabelKind::CurrentLocal));
    }

    #[test]
    fn build_ref_map_remote_branch_becomes_remote_label() {
        let refs = vec![RepoRef {
            name: "origin/main".into(),
            kind: RepoRefKind::RemoteBranch,
            target_hash: "cccccccccccccccccccccccccccccccccccccccc".into(),
            remote_name: Some("origin".into()),
            is_current: false,
            upstream_ref: None,
        }];
        let map = build_ref_map(&refs);
        let labels = &map["cccccccccccccccccccccccccccccccccccccccc"];
        assert_eq!(labels.len(), 1);
        assert!(matches!(labels[0].kind, BranchLabelKind::Remote));
        assert_eq!(labels[0].remote_name.as_deref(), Some("origin"));
    }

    /// Fixture snapshot regression: build a deterministic repo with a
    /// branch fork, project it, and lock down the shape of the result.
    #[test]
    fn fixture_repo_projects_stable_sidebar_and_graph() {
        use crate::services::{
            test_support::{checkout_branch_for_setup, commit_all, init_test_repo, write_file},
            GitService,
        };

        let (temp_repo, repo) = init_test_repo("fixture_snapshot_regression");

        let default_branch = repo
            .head()
            .expect("head exists after init")
            .shorthand()
            .expect("head shorthand is utf-8")
            .to_string();

        {
            let head_commit = repo
                .head()
                .expect("head exists")
                .peel_to_commit()
                .expect("head points to commit");
            repo.branch("side", &head_commit, false)
                .expect("create side branch");
        }

        write_file(&temp_repo.path, "tracked.txt", "main-progress\n");
        let main_tip_oid = commit_all(&repo, "main progress");
        repo.tag_lightweight(
            "main-alias",
            &repo
                .find_object(main_tip_oid, None)
                .expect("find main tip object"),
            false,
        )
        .expect("create tag");

        checkout_branch_for_setup(&repo, "side");
        write_file(&temp_repo.path, "tracked.txt", "side-progress\n");
        commit_all(&repo, "side progress");

        checkout_branch_for_setup(&repo, &default_branch);

        let service = GitService::open(temp_repo.path_str()).expect("open service");
        let snapshot = service.load_repo(100);
        let projection = super::project_repo(snapshot);

        assert_eq!(projection.current_branch, default_branch);
        assert_eq!(
            projection.commits.len(),
            3,
            "3 commits expected: initial, side progress, main progress"
        );
        assert_eq!(projection.graph_rows.len(), projection.commits.len());
        assert!(projection.num_lanes >= 2);

        use crate::view_model::SidebarSectionKind;
        let local_section = projection
            .sidebar_sections
            .iter()
            .find(|s| matches!(s.kind, SidebarSectionKind::Local))
            .expect("local branches section present");
        let local_names: Vec<&str> = local_section
            .branches
            .iter()
            .map(|b| b.name.as_str())
            .collect();
        assert!(local_names.contains(&"side"));
        assert!(local_names.contains(&default_branch.as_str()));

        let tags_section = projection
            .sidebar_sections
            .iter()
            .find(|s| matches!(s.kind, SidebarSectionKind::Tags))
            .expect("tags section present");
        let tag_names: Vec<&str> = tags_section
            .branches
            .iter()
            .map(|b| b.name.as_str())
            .collect();
        assert_eq!(tag_names, vec!["main-alias"]);

        let default_commit_hash = projection
            .head_hash
            .clone()
            .expect("head hash present on non-empty repo");
        let side_row_hash = projection
            .commits
            .iter()
            .find(|c| c.message.contains("side progress"))
            .map(|c| c.hash.clone())
            .expect("side progress commit present");
        assert_ne!(default_commit_hash, side_row_hash);

        assert!(projection.default_remote_name.is_none());
    }

    #[test]
    fn worktrees_sidebar_section_lists_non_primary_only() {
        use crate::core::WorktreeInfo;
        use std::path::PathBuf;

        let primary_path = PathBuf::from("/tmp/primary");
        let wt_path = PathBuf::from("/tmp/wt");

        let snapshot = RepoSnapshot {
            worktrees: vec![
                WorktreeInfo {
                    path: primary_path,
                    branch_name: "main".into(),
                    head_hash: "abc".into(),
                    is_primary: true,
                    is_locked: false,
                    is_prunable: false,
                },
                WorktreeInfo {
                    path: wt_path.clone(),
                    branch_name: "feature-x".into(),
                    head_hash: "def".into(),
                    is_primary: false,
                    is_locked: false,
                    is_prunable: false,
                },
            ],
            ..RepoSnapshot::default()
        };

        use crate::view_model::SidebarSectionKind;
        let sections =
            super::build_sidebar_sections(&snapshot.refs, &snapshot.stashes, &snapshot.worktrees);
        let worktree_section = sections
            .iter()
            .find(|s| matches!(s.kind, SidebarSectionKind::Worktrees))
            .expect("worktrees section");
        assert_eq!(worktree_section.branches.len(), 1);
        let entry = &worktree_section.branches[0];
        assert_eq!(entry.name, "feature-x");
        assert_eq!(entry.worktree_path.as_ref(), Some(&wt_path));
    }
}

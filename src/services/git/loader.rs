use std::collections::HashSet;

use git2::{BranchType, Sort};

use crate::services::{CommitSnapshot, RefsSnapshot, RepoRefKind, RepoSnapshot};

use super::{
    checkout::is_branch_ancestor_of_branch, refs::load_refs, stashes::load_stashes,
    working_tree::load_dirty_snapshot, GitService,
};

pub(super) fn load_repo(service: &GitService, commit_limit: usize) -> RepoSnapshot {
    let refs = load_refs(service);
    let (commits, has_more_commits) = load_commits(service, &refs, commit_limit);
    let current_branch = service.current_branch_name();
    let remote_names = load_remote_names(service);
    let default_remote_name = pick_default_remote(&remote_names);
    let fast_forward_candidates =
        load_fast_forward_candidates(service, current_branch.as_deref(), &refs);
    let worktrees = super::worktrees::list_worktrees(service).unwrap_or_default();
    let active_worktree_path = service
        .repo
        .workdir()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
        .unwrap_or_default();

    RepoSnapshot {
        commits,
        refs,
        dirty: load_dirty_snapshot(service),
        stashes: load_stashes(service),
        repo_name: repo_name(service),
        current_branch,
        head_hash: head_hash(service),
        has_more_commits,
        default_remote_name,
        remote_names,
        fast_forward_candidates,
        worktrees,
        active_worktree_path,
    }
}

/// Read-only local state needed to refresh the graph + sidebar refs tree after
/// a fetch. Skips working-tree inspection, HEAD-branch lookup, and repo-name
/// derivation — fetch cannot change any of those, and keeping them out of the
/// payload is what lets the fetch-path state update be architecturally narrow.
pub(super) fn load_refs_snapshot(service: &GitService, commit_limit: usize) -> RefsSnapshot {
    let refs = load_refs(service);
    let (commits, has_more_commits) = load_commits(service, &refs, commit_limit);
    let current_branch = service.current_branch_name();
    let remote_names = load_remote_names(service);
    let default_remote_name = pick_default_remote(&remote_names);
    let fast_forward_candidates =
        load_fast_forward_candidates(service, current_branch.as_deref(), &refs);
    let worktrees = super::worktrees::list_worktrees(service).unwrap_or_default();
    let active_worktree_path = service
        .repo
        .workdir()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
        .unwrap_or_default();

    RefsSnapshot {
        commits,
        refs,
        dirty: load_dirty_snapshot(service),
        stashes: load_stashes(service),
        head_hash: head_hash(service),
        has_more_commits,
        default_remote_name,
        remote_names,
        fast_forward_candidates,
        worktrees,
        active_worktree_path,
    }
}

fn load_remote_names(service: &GitService) -> Vec<String> {
    service
        .repo
        .remotes()
        .ok()
        .map(|r| r.iter().flatten().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

fn pick_default_remote(remotes: &[String]) -> Option<String> {
    remotes
        .iter()
        .find(|r| r.as_str() == "origin")
        .cloned()
        .or_else(|| remotes.first().cloned())
}

/// Compute the set of local branches the current branch is a strict ancestor
/// of. Mirrors `is_branch_ancestor_of_branch` but in bulk so right-click
/// menus can answer the "can fast-forward?" question from cached data.
fn load_fast_forward_candidates(
    service: &GitService,
    current_branch: Option<&str>,
    refs: &[crate::services::RepoRef],
) -> HashSet<String> {
    let Some(current) = current_branch else {
        return HashSet::new();
    };
    let current = current.to_string();
    if service
        .repo
        .find_branch(&current, BranchType::Local)
        .is_err()
    {
        return HashSet::new();
    }

    refs.iter()
        .filter(|r| r.kind == RepoRefKind::LocalBranch && r.name != current)
        .filter_map(|r| {
            is_branch_ancestor_of_branch(service, &current, &r.name)
                .ok()
                .filter(|&can_ff| can_ff)
                .map(|_| r.name.clone())
        })
        .collect()
}

fn load_commits(
    service: &GitService,
    refs: &[crate::services::RepoRef],
    commit_limit: usize,
) -> (Vec<CommitSnapshot>, bool) {
    let mut revwalk = match service.repo.revwalk() {
        Ok(revwalk) => revwalk,
        Err(_) => return (vec![], false),
    };
    let _ = revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME);

    for repo_ref in refs {
        let Ok(oid) = git2::Oid::from_str(&repo_ref.target_hash) else {
            continue;
        };
        let _ = revwalk.push(oid);
    }

    if let Ok(head) = service.repo.head() {
        if let Some(oid) = head.target() {
            let _ = revwalk.push(oid);
        }
    }

    let mut commits = Vec::with_capacity(commit_limit);
    let mut has_more_commits = false;

    for oid_result in revwalk {
        let oid = match oid_result {
            Ok(oid) => oid,
            Err(_) => continue,
        };
        let commit = match service.repo.find_commit(oid) {
            Ok(commit) => commit,
            Err(_) => continue,
        };

        if commits.len() == commit_limit {
            has_more_commits = true;
            break;
        }

        let author = commit.author();
        commits.push(CommitSnapshot {
            hash: oid.to_string(),
            message: commit.message().unwrap_or("").trim_end().to_string(),
            author_name: author.name().unwrap_or("Unknown").to_string(),
            authored_at: author.when().seconds(),
            authored_offset_minutes: author.when().offset_minutes(),
            parent_hashes: (0..commit.parent_count())
                .filter_map(|idx| commit.parent_id(idx).ok())
                .map(|parent_oid| parent_oid.to_string())
                .collect(),
        });
    }

    (commits, has_more_commits)
}

fn repo_name(service: &GitService) -> String {
    service
        .repo
        .workdir()
        .or_else(|| service.repo.path().parent())
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string())
}

fn head_hash(service: &GitService) -> Option<String> {
    service
        .repo
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok())
        .map(|commit| commit.id().to_string())
}

#[cfg(test)]
mod tests {
    use super::{load_refs_snapshot, load_repo, pick_default_remote};
    use crate::services::{git::GitService, test_support};

    #[test]
    fn pick_default_remote_returns_none_when_empty() {
        assert_eq!(pick_default_remote(&[]), None);
    }

    #[test]
    fn pick_default_remote_returns_only_remote_when_single() {
        let remotes = vec!["upstream".to_string()];
        assert_eq!(pick_default_remote(&remotes), Some("upstream".to_string()));
    }

    #[test]
    fn pick_default_remote_prefers_origin_over_first() {
        let remotes = vec![
            "upstream".to_string(),
            "origin".to_string(),
            "fork".to_string(),
        ];
        assert_eq!(pick_default_remote(&remotes), Some("origin".to_string()));
    }

    #[test]
    fn pick_default_remote_returns_first_when_no_origin() {
        let remotes = vec!["upstream".to_string(), "fork".to_string()];
        assert_eq!(pick_default_remote(&remotes), Some("upstream".to_string()));
    }

    #[test]
    fn snapshots_include_all_remote_names_in_repo_order() {
        let (temp_repo, repo) = test_support::init_test_repo("load_repo_remote_names");
        test_support::add_remote(&repo, "upstream", "https://example.invalid/upstream.git");
        test_support::add_remote(&repo, "origin", "https://example.invalid/origin.git");
        test_support::add_remote(&repo, "fork", "https://example.invalid/fork.git");

        let service = GitService::open(temp_repo.path_str()).expect("open service");
        // Keep libgit2's repository order; selecting the default remote must
        // not move "origin" to the front of the exposed list.
        let expected = vec![
            "fork".to_string(),
            "origin".to_string(),
            "upstream".to_string(),
        ];

        let repo_snapshot = load_repo(&service, 10);
        assert_eq!(repo_snapshot.remote_names, expected);
        assert_eq!(
            repo_snapshot.default_remote_name,
            Some("origin".to_string())
        );

        let refs_snapshot = load_refs_snapshot(&service, 10);
        assert_eq!(refs_snapshot.remote_names, expected);
        assert_eq!(
            refs_snapshot.default_remote_name,
            Some("origin".to_string())
        );
    }
}

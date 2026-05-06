use git2::Repository;

use crate::core::{ChangeKind, ChangedFile};
use crate::services::git_error::GitError;
use crate::services::WorkingTreeDiffResult;

use super::helpers::{find_commit_or, wrap_git2_error};
use super::{CommitDiffResult, MergedCommitDiffResult};

pub(super) fn load_merged_commit_file_diff(
    repo_path: &str,
    hashes: &[String],
    file_path: &str,
) -> Result<WorkingTreeDiffResult, GitError> {
    if hashes.len() < 2 {
        if let Some(hash) = hashes.first() {
            return super::working_tree_diff::load_commit_file_diff_standalone(
                repo_path, hash, file_path,
            );
        }
        return Err(GitError::Other("no commits selected".to_string()));
    }

    let repo = Repository::open(repo_path).map_err(|e| wrap_git2_error("open repo", e))?;

    // hashes[0] = newest commit, hashes[last] = oldest commit
    // Diff: oldest commit's parent tree → newest commit's tree for the specified file
    let newest_oid = parse_oid(&hashes[0])?;
    let newest_commit = find_commit_or(&repo, newest_oid)?;
    let newest_tree = newest_commit.tree().ok();

    let oldest_oid = parse_oid(hashes.last().unwrap())?;
    let oldest_commit = find_commit_or(&repo, oldest_oid)?;
    let oldest_parent_tree = oldest_commit.parent(0).ok().and_then(|p| p.tree().ok());

    let mut opts = git2::DiffOptions::new();
    opts.pathspec(file_path);

    let diff = repo
        .diff_tree_to_tree(
            oldest_parent_tree.as_ref(),
            newest_tree.as_ref(),
            Some(&mut opts),
        )
        .map_err(|e| wrap_git2_error("create merged commit file diff", e))?;

    let old_content = || content_from_tree(&repo, oldest_parent_tree.as_ref(), file_path);
    let new_content = || content_from_tree(&repo, newest_tree.as_ref(), file_path);

    super::working_tree_diff::process_git_diff(diff, file_path, old_content, new_content)
}

fn parse_oid(hash: &str) -> Result<git2::Oid, GitError> {
    git2::Oid::from_str(hash).map_err(|e| GitError::Other(format!("invalid commit hash: {e}")))
}

fn content_from_tree(
    repo: &Repository,
    tree: Option<&git2::Tree>,
    file_path: &str,
) -> super::working_tree_diff::FileContent {
    let Some(tree) = tree else {
        return super::working_tree_diff::FileContent::missing();
    };
    let Ok(entry) = tree.get_path(std::path::Path::new(file_path)) else {
        return super::working_tree_diff::FileContent::missing();
    };
    let Ok(blob) = repo.find_blob(entry.id()) else {
        return super::working_tree_diff::FileContent::missing();
    };
    super::working_tree_diff::content_from_blob(&blob)
}

pub(super) fn load_merged_commit_diff(
    repo_path: &str,
    hashes: &[String],
) -> Result<MergedCommitDiffResult, GitError> {
    let repo = Repository::open(repo_path).map_err(|e| wrap_git2_error("open repo", e))?;

    // Net diff: oldest commit's parent tree → newest commit's tree. Git gives us
    // the correct final status per path (e.g. add-then-delete collapses to nothing,
    // modify-then-delete shows as Deleted) and omits no-op paths automatically.
    let newest_oid = parse_oid(&hashes[0])?;
    let newest_commit = find_commit_or(&repo, newest_oid)?;
    let newest_tree = newest_commit.tree().ok();

    let oldest_oid = parse_oid(hashes.last().unwrap())?;
    let oldest_commit = find_commit_or(&repo, oldest_oid)?;
    let oldest_parent_tree = oldest_commit.parent(0).ok().and_then(|p| p.tree().ok());

    let diff = repo
        .diff_tree_to_tree(oldest_parent_tree.as_ref(), newest_tree.as_ref(), None)
        .map_err(|e| wrap_git2_error("create merged commit diff", e))?;

    let mut modified_count = 0u32;
    let mut added_count = 0u32;
    let mut deleted_count = 0u32;
    let mut files: Vec<ChangedFile> = Vec::new();

    let _ = diff.foreach(
        &mut |delta, _progress| {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();

            let kind = match delta.status() {
                git2::Delta::Added => {
                    added_count += 1;
                    ChangeKind::Added
                }
                git2::Delta::Deleted => {
                    deleted_count += 1;
                    ChangeKind::Deleted
                }
                _ => {
                    modified_count += 1;
                    ChangeKind::Modified
                }
            };
            files.push(ChangedFile { path, kind });
            true
        },
        None,
        None,
        None,
    );

    files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(MergedCommitDiffResult {
        hashes: hashes.to_vec(),
        modified_count,
        added_count,
        deleted_count,
        files,
    })
}

pub(super) fn load_commit_diff(
    repo_path: &str,
    commit_idx: usize,
    hash: &str,
) -> Result<CommitDiffResult, GitError> {
    let span = crate::perf::Span::new("git.commit_diff_stats_load")
        .field("repo", repo_path)
        .field("commit_idx", commit_idx)
        .field("hash", hash);
    let repo = Repository::open(repo_path).map_err(|e| wrap_git2_error("open repo", e))?;
    let oid = parse_oid(hash)?;
    let commit = find_commit_or(&repo, oid)?;
    let (modified_count, added_count, deleted_count, files) = diff_stats(&repo, &commit);
    span.finish_with("files", files.len());
    Ok(CommitDiffResult {
        commit_idx,
        hash: hash.to_string(),
        modified_count,
        added_count,
        deleted_count,
        files,
    })
}

pub(super) fn diff_stats(
    repo: &Repository,
    commit: &git2::Commit,
) -> (u32, u32, u32, Vec<ChangedFile>) {
    let tree = commit.tree().ok();
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

    let diff = match repo.diff_tree_to_tree(parent_tree.as_ref(), tree.as_ref(), None) {
        Ok(d) => d,
        Err(_) => return (0, 0, 0, vec![]),
    };

    let mut stats = DiffStats::default();
    stats.add_diff(&diff);

    if is_current_stash_commit(repo, commit.id()) {
        if let Some(untracked_tree) = commit.parent(2).ok().and_then(|p| p.tree().ok()) {
            if let Ok(untracked_diff) = repo.diff_tree_to_tree(None, Some(&untracked_tree), None) {
                stats.add_diff(&untracked_diff);
            }
        }
    }

    stats.finish()
}

#[derive(Default)]
struct DiffStats {
    modified_count: u32,
    added_count: u32,
    deleted_count: u32,
    files: Vec<ChangedFile>,
}

impl DiffStats {
    fn add_diff(&mut self, diff: &git2::Diff<'_>) {
        let _ = diff.foreach(
            &mut |delta, _progress| {
                self.add_delta(delta);
                true
            },
            None,
            None,
            None,
        );
    }

    fn add_delta(&mut self, delta: git2::DiffDelta<'_>) {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        let kind = match delta.status() {
            git2::Delta::Added => {
                self.added_count += 1;
                ChangeKind::Added
            }
            git2::Delta::Deleted => {
                self.deleted_count += 1;
                ChangeKind::Deleted
            }
            _ => {
                self.modified_count += 1;
                ChangeKind::Modified
            }
        };
        self.files.push(ChangedFile { path, kind });
    }

    fn finish(mut self) -> (u32, u32, u32, Vec<ChangedFile>) {
        self.files.sort_by(|a, b| a.path.cmp(&b.path));
        (
            self.modified_count,
            self.added_count,
            self.deleted_count,
            self.files,
        )
    }
}

pub(super) fn is_current_stash_commit(repo: &Repository, oid: git2::Oid) -> bool {
    repo.reflog("refs/stash")
        .ok()
        .is_some_and(|reflog| reflog.iter().any(|entry| entry.id_new() == oid))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::load_commit_diff;
    use crate::{
        core::ChangeKind,
        services::{
            test_support::{commit_all, configure_test_signature, init_test_repo, write_file},
            GitService,
        },
    };

    fn kind_for(files: &[crate::core::ChangedFile], path: &str) -> Option<ChangeKind> {
        files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.kind.clone())
    }

    #[test]
    fn stash_diff_includes_tracked_and_untracked_file_kinds() {
        let (temp_repo, repo) = init_test_repo("stash_diff_file_kinds");
        configure_test_signature(&repo);
        write_file(&temp_repo.path, "deleted.txt", "delete me\n");
        commit_all(&repo, "add deleted fixture");

        write_file(&temp_repo.path, "tracked.txt", "changed\n");
        fs::remove_file(temp_repo.path.join("deleted.txt")).expect("failed to delete fixture");
        write_file(&temp_repo.path, "staged_added.txt", "staged\n");
        write_file(&temp_repo.path, "untracked.txt", "untracked\n");
        let mut index = repo.index().expect("failed to open index");
        index
            .add_path(Path::new("staged_added.txt"))
            .expect("failed to stage added file");
        index.write().expect("failed to write index");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service.create_stash().expect("failed to create stash");
        let stash = service
            .load_repo(super::super::COMMIT_LOAD_LIMIT)
            .stashes
            .into_iter()
            .next()
            .expect("stash should exist");

        let result =
            load_commit_diff(temp_repo.path_str(), 0, &stash.hash).expect("stash diff loads");

        assert_eq!(result.modified_count, 1);
        assert_eq!(result.added_count, 2);
        assert_eq!(result.deleted_count, 1);
        assert_eq!(
            kind_for(&result.files, "tracked.txt"),
            Some(ChangeKind::Modified)
        );
        assert_eq!(
            kind_for(&result.files, "deleted.txt"),
            Some(ChangeKind::Deleted)
        );
        assert_eq!(
            kind_for(&result.files, "staged_added.txt"),
            Some(ChangeKind::Added)
        );
        assert_eq!(
            kind_for(&result.files, "untracked.txt"),
            Some(ChangeKind::Added)
        );
    }
}

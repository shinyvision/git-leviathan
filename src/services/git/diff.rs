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
) -> Option<String> {
    let tree = tree?;
    let entry = tree.get_path(std::path::Path::new(file_path)).ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;
    let content = blob.content();
    Some(String::from_utf8_lossy(content).into_owned())
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
    let repo = Repository::open(repo_path).map_err(|e| wrap_git2_error("open repo", e))?;
    let oid = parse_oid(hash)?;
    let commit = find_commit_or(&repo, oid)?;
    let (modified_count, added_count, deleted_count, files) = diff_stats(&repo, &commit);
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

    (modified_count, added_count, deleted_count, files)
}

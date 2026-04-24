use git2::{IndexAddOption, Oid, RepositoryState, Status, StatusEntry, StatusOptions, StatusShow};
use std::path::Path;

use crate::{
    core::{ChangeKind, ChangedFile},
    services::{git_error::GitError, DirtySnapshot},
};

use super::helpers::{find_commit_or, wrap_git2_error};
use super::GitService;

pub(super) fn load_dirty_snapshot(service: &GitService) -> Option<DirtySnapshot> {
    let mut options = StatusOptions::new();
    options
        .show(StatusShow::IndexAndWorkdir)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);

    let statuses = service.repo.statuses(Some(&mut options)).ok()?;
    let mut conflicted_files = Vec::new();
    let mut staged_files = Vec::new();
    let mut unstaged_files = Vec::new();

    for entry in statuses.iter() {
        let status = entry.status();
        if status.is_conflicted() {
            conflicted_files.push(ChangedFile {
                path: status_path(&entry),
                kind: ChangeKind::Modified,
            });
            continue;
        }
        if let Some(kind) = staged_change_kind(status) {
            staged_files.push(ChangedFile {
                path: status_path(&entry),
                kind,
            });
        }
        if let Some(kind) = unstaged_change_kind(status) {
            unstaged_files.push(ChangedFile {
                path: status_path(&entry),
                kind,
            });
        }
    }

    conflicted_files.sort_by(|left, right| left.path.cmp(&right.path));
    staged_files.sort_by(|left, right| left.path.cmp(&right.path));
    unstaged_files.sort_by(|left, right| left.path.cmp(&right.path));

    let snapshot = DirtySnapshot {
        conflicted_files,
        staged_files,
        unstaged_files,
        parent_hashes: dirty_parent_hashes(service),
    };
    snapshot.has_changes().then_some(snapshot)
}

fn dirty_parent_hashes(service: &GitService) -> Vec<String> {
    let mut parent_hashes = Vec::new();
    if let Some(head_oid) = service.repo.head().ok().and_then(|head| head.target()) {
        parent_hashes.push(head_oid.to_string());
    }

    if service.repo.state() == RepositoryState::Merge {
        if let Ok(merge_heads) = merge_head_oids(service) {
            for oid in merge_heads {
                let hash = oid.to_string();
                if !parent_hashes.iter().any(|existing| existing == &hash) {
                    parent_hashes.push(hash);
                }
            }
        }
    }

    parent_hashes
}

pub(super) fn commit_dirty_changes(
    service: &mut GitService,
    summary: &str,
    description: &str,
) -> Result<(), GitError> {
    let summary = summary.trim();
    if summary.is_empty() {
        return Err(GitError::Other(
            "commit summary cannot be empty".to_string(),
        ));
    }

    let dirty = load_dirty_snapshot(service)
        .ok_or_else(|| GitError::Other("there are no changes to commit".to_string()))?;
    if dirty.staged_files.is_empty() {
        return Err(GitError::Other(
            "there are no staged changes to commit".to_string(),
        ));
    }

    let signature = service
        .repo
        .signature()
        .map_err(|e| wrap_git2_error("read git author identity", e))?;
    let message = commit_message(summary, description);
    let is_merge_state = service.repo.state() == RepositoryState::Merge;

    let mut index = service
        .repo
        .index()
        .map_err(|e| wrap_git2_error("open repository index", e))?;
    let tree_id = index
        .write_tree()
        .map_err(|e| wrap_git2_error("write commit tree", e))?;
    let tree = service
        .repo
        .find_tree(tree_id)
        .map_err(|e| wrap_git2_error("find commit tree", e))?;
    let parents = commit_parents(service)?;

    if let Some(parent) = parents.first() {
        let parent_tree = parent
            .tree()
            .map_err(|e| wrap_git2_error("read parent tree", e))?;
        if !is_merge_state && parent_tree.id() == tree.id() {
            return Err(GitError::Other(
                "there are no changes to commit".to_string(),
            ));
        }
    }

    let parent_refs = parents.iter().collect::<Vec<_>>();
    service
        .repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            &message,
            &tree,
            &parent_refs,
        )
        .map_err(|e| wrap_git2_error("create commit", e))?;
    if is_merge_state {
        service
            .repo
            .cleanup_state()
            .map_err(|e| wrap_git2_error("clean merge state", e))?;
    }
    Ok(())
}

pub(super) fn stage_file(service: &mut GitService, path: &str) -> Result<(), GitError> {
    if path.is_empty() {
        return Err(GitError::Other("file path cannot be empty".to_string()));
    }

    let mut index = service
        .repo
        .index()
        .map_err(|e| wrap_git2_error("open repository index", e))?;
    let relative_path = Path::new(path);
    let exists_in_workdir = service
        .repo
        .workdir()
        .map(|workdir| workdir.join(relative_path).exists())
        .unwrap_or(false);
    if exists_in_workdir {
        index
            .add_path(relative_path)
            .map_err(|e| wrap_git2_error(&format!("stage file '{path}'"), e))?;
    } else {
        index
            .remove_path(relative_path)
            .map_err(|e| wrap_git2_error(&format!("stage deletion '{path}'"), e))?;
    }
    index
        .write()
        .map_err(|e| wrap_git2_error("write repository index", e))?;
    Ok(())
}

pub(super) fn stage_all_dirty_changes(service: &mut GitService) -> Result<(), GitError> {
    let mut index = service
        .repo
        .index()
        .map_err(|e| wrap_git2_error("open repository index", e))?;
    index
        .add_all(["."].iter(), IndexAddOption::DEFAULT, None)
        .map_err(|e| wrap_git2_error("stage changes", e))?;
    index
        .update_all(["."].iter(), None)
        .map_err(|e| wrap_git2_error("stage deletions", e))?;
    index
        .write()
        .map_err(|e| wrap_git2_error("write repository index", e))?;
    Ok(())
}

pub(super) fn unstage_file(service: &mut GitService, path: &str) -> Result<(), GitError> {
    if path.is_empty() {
        return Err(GitError::Other("file path cannot be empty".to_string()));
    }

    let head = head_object(service)?;
    service
        .repo
        .reset_default(head.as_ref(), [path])
        .map_err(|e| wrap_git2_error(&format!("unstage file '{path}'"), e))?;
    Ok(())
}

pub(super) fn unstage_all_dirty_changes(service: &mut GitService) -> Result<(), GitError> {
    let Some(dirty) = load_dirty_snapshot(service) else {
        return Ok(());
    };
    let paths = dirty
        .staged_files
        .into_iter()
        .map(|file| file.path)
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Ok(());
    }

    let head = head_object(service)?;
    service
        .repo
        .reset_default(head.as_ref(), paths.iter().map(String::as_str))
        .map_err(|e| wrap_git2_error("unstage changes", e))?;
    Ok(())
}

pub(super) fn discard_file(service: &mut GitService, path: &str) -> Result<(), GitError> {
    if path.is_empty() {
        return Err(GitError::Other("file path cannot be empty".to_string()));
    }

    let head = head_object(service)?;
    if head.is_some() {
        service
            .repo
            .reset_default(head.as_ref(), [path])
            .map_err(|e| wrap_git2_error(&format!("reset index for '{path}'"), e))?;
    }

    let in_head = head
        .as_ref()
        .and_then(|object| object.peel_to_tree().ok())
        .and_then(|tree| tree.get_path(Path::new(path)).ok())
        .is_some();

    if in_head {
        let mut checkout = git2::build::CheckoutBuilder::new();
        checkout.force().path(path);
        service
            .repo
            .checkout_head(Some(&mut checkout))
            .map_err(|e| wrap_git2_error(&format!("restore '{path}'"), e))?;
    } else if let Some(workdir) = service.repo.workdir() {
        let full = workdir.join(Path::new(path));
        if full.exists() {
            std::fs::remove_file(&full)
                .map_err(|e| GitError::Other(format!("failed to delete '{}': {e}", path)))?;
        }
    }

    Ok(())
}

pub(super) fn discard_all_dirty_changes(service: &mut GitService) -> Result<(), GitError> {
    let mut untracked_paths: Vec<String> = Vec::new();
    {
        let mut options = StatusOptions::new();
        options
            .show(StatusShow::IndexAndWorkdir)
            .include_untracked(true)
            .recurse_untracked_dirs(true);
        if let Ok(statuses) = service.repo.statuses(Some(&mut options)) {
            for entry in statuses.iter() {
                let status = entry.status();
                if status.is_wt_new() && !status.is_index_new() {
                    if let Some(path) = entry.path() {
                        untracked_paths.push(path.to_string());
                    }
                }
            }
        }
    }

    if let Some(head) = head_object(service)? {
        service
            .repo
            .reset(&head, git2::ResetType::Hard, None)
            .map_err(|e| wrap_git2_error("discard changes", e))?;
    }

    if let Some(workdir) = service.repo.workdir().map(|p| p.to_path_buf()) {
        for path in untracked_paths {
            let full = workdir.join(&path);
            let _ = std::fs::remove_file(&full);
        }
    }

    Ok(())
}

pub(super) fn mark_conflict_resolved(service: &mut GitService, path: &str) -> Result<(), GitError> {
    stage_file(service, path)
}

pub(super) fn mark_all_conflicts_resolved(service: &mut GitService) -> Result<(), GitError> {
    let paths = conflict_paths(service)?;
    for path in paths {
        stage_file(service, &path)?;
    }
    Ok(())
}

fn conflict_paths(service: &GitService) -> Result<Vec<String>, GitError> {
    let index = service
        .repo
        .index()
        .map_err(|e| wrap_git2_error("open repository index", e))?;
    let conflicts = index
        .conflicts()
        .map_err(|e| wrap_git2_error("read index conflicts", e))?;
    let mut paths = Vec::new();

    for conflict in conflicts {
        let conflict = conflict.map_err(|e| wrap_git2_error("read index conflict", e))?;
        let path = conflict
            .our
            .as_ref()
            .or(conflict.their.as_ref())
            .or(conflict.ancestor.as_ref())
            .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
            .unwrap_or_default();
        if !path.is_empty() && !paths.iter().any(|existing| existing == &path) {
            paths.push(path);
        }
    }

    paths.sort();
    Ok(paths)
}

fn head_object(service: &GitService) -> Result<Option<git2::Object<'_>>, GitError> {
    match service.repo.revparse_single("HEAD") {
        Ok(object) => Ok(Some(object)),
        Err(err)
            if matches!(
                err.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) =>
        {
            Ok(None)
        }
        Err(err) => Err(wrap_git2_error("read HEAD", err)),
    }
}

fn staged_change_kind(status: Status) -> Option<ChangeKind> {
    if status.is_index_new() {
        Some(ChangeKind::Added)
    } else if status.is_index_deleted() {
        Some(ChangeKind::Deleted)
    } else if status.is_index_modified()
        || status.is_index_renamed()
        || status.is_index_typechange()
    {
        Some(ChangeKind::Modified)
    } else {
        None
    }
}

fn unstaged_change_kind(status: Status) -> Option<ChangeKind> {
    if status.is_wt_new() {
        Some(ChangeKind::Added)
    } else if status.is_wt_deleted() {
        Some(ChangeKind::Deleted)
    } else if status.is_wt_modified()
        || status.is_wt_renamed()
        || status.is_wt_typechange()
        || status.is_conflicted()
    {
        Some(ChangeKind::Modified)
    } else {
        None
    }
}

fn status_path(entry: &StatusEntry<'_>) -> String {
    entry
        .index_to_workdir()
        .or_else(|| entry.head_to_index())
        .and_then(|delta| {
            delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(|path| path.to_string_lossy().into_owned())
        })
        .or_else(|| entry.path().map(ToString::to_string))
        .unwrap_or_default()
}

fn commit_message(summary: &str, description: &str) -> String {
    let description = description.trim();
    if description.is_empty() {
        summary.to_string()
    } else {
        format!("{}\n\n{}", summary, description)
    }
}

fn commit_parents(service: &GitService) -> Result<Vec<git2::Commit<'_>>, GitError> {
    let mut parent_oids = Vec::new();
    if let Some(head_oid) = service.repo.head().ok().and_then(|head| head.target()) {
        parent_oids.push(head_oid);
    }

    if service.repo.state() == RepositoryState::Merge {
        for oid in merge_head_oids(service)? {
            if !parent_oids.iter().any(|existing| existing == &oid) {
                parent_oids.push(oid);
            }
        }
    }

    parent_oids
        .into_iter()
        .map(|oid| find_commit_or(&service.repo, oid))
        .collect()
}

fn merge_head_oids(service: &GitService) -> Result<Vec<Oid>, GitError> {
    let path = service.repo.path().join("MERGE_HEAD");
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| GitError::Other(format!("failed to read MERGE_HEAD: {e}")))?;
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            Oid::from_str(line)
                .map_err(|e| GitError::Other(format!("invalid MERGE_HEAD oid '{line}': {e}")))
        })
        .collect()
}

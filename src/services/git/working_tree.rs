use git2::{Oid, RepositoryState, Status, StatusEntry, StatusOptions, StatusShow};
use std::path::Path;

use crate::{
    core::{ChangeKind, ChangedFile, WorkingTreeOperation},
    services::{git_error::GitError, DirtySnapshot},
};

use super::helpers::{command_dir, process_output_detail, wrap_git2_error};
use super::process::{spawn_git_command, spawn_program};
use super::rebase;
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
                path: status_path(&entry, StatusShow::IndexAndWorkdir),
                kind: ChangeKind::Modified,
            });
            continue;
        }
        if let Some(kind) = staged_change_kind(status) {
            staged_files.push(ChangedFile {
                path: status_path(&entry, StatusShow::Index),
                kind,
            });
        }
        if let Some(kind) = unstaged_change_kind(status) {
            unstaged_files.push(ChangedFile {
                path: status_path(&entry, StatusShow::Workdir),
                kind,
            });
        }
    }

    conflicted_files.sort_by(|left, right| left.path.cmp(&right.path));
    staged_files.sort_by(|left, right| left.path.cmp(&right.path));
    unstaged_files.sort_by(|left, right| left.path.cmp(&right.path));

    let operation = match service.repo.state() {
        RepositoryState::Merge => WorkingTreeOperation::Merge,
        state if rebase::in_progress(state) => WorkingTreeOperation::Rebase,
        _ => WorkingTreeOperation::None,
    };
    let snapshot = DirtySnapshot {
        operation,
        conflicted_files,
        staged_files,
        unstaged_files,
        parent_hashes: dirty_parent_hashes(service),
    };
    snapshot.needs_working_tree_row().then_some(snapshot)
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
    if summary.is_empty()
        && (service.repo.state() != RepositoryState::Merge || !description.trim().is_empty())
    {
        return Err(GitError::Other(
            "commit summary cannot be empty".to_string(),
        ));
    }

    let dirty = load_dirty_snapshot(service)
        .ok_or_else(|| GitError::Other("there are no changes to commit".to_string()))?;
    if dirty.operation == WorkingTreeOperation::Rebase {
        return Err(GitError::Other(
            "use Continue Rebase to finish the current step".into(),
        ));
    }
    if !dirty.conflicted_files.is_empty() {
        return Err(GitError::Other(
            "resolve the remaining conflicts before committing".into(),
        ));
    }
    if dirty.operation != WorkingTreeOperation::Merge && dirty.staged_files.is_empty() {
        return Err(GitError::Other(
            "there are no staged changes to commit".to_string(),
        ));
    }

    let message = commit_message(summary, description);
    let args = if summary.is_empty() {
        ["commit", "--no-edit", "--cleanup=strip"]
    } else {
        ["commit", "-m", &message]
    };
    let output = spawn_git_command(&command_dir(&service.repo)?, &args, "commit")?;
    if output.status.success() {
        Ok(())
    } else {
        Err(GitError::Other(format!(
            "commit failed: {}",
            process_output_detail(&output)
        )))
    }
}

pub(super) fn stage_file(service: &mut GitService, path: &str) -> Result<(), GitError> {
    stage_files(service, &[path.to_owned()])
}

pub(super) fn stage_files(service: &GitService, paths: &[String]) -> Result<(), GitError> {
    let paths = expand_rename_sources(service, paths.to_vec(), StatusShow::Workdir)?;
    update_index_paths(service, &["add", "--all"], &paths)
}

pub(super) fn stage_all_dirty_changes(service: &mut GitService) -> Result<(), GitError> {
    let mut index = service
        .repo
        .index()
        .map_err(|e| wrap_git2_error("open repository index", e))?;
    index
        .read(true)
        .map_err(|e| wrap_git2_error("read repository index", e))?;
    if index.has_conflicts() {
        // The Stage All button belongs to the unstaged section. Only the
        // explicit resolution actions should remove unmerged index entries.
        let mut options = StatusOptions::new();
        options.include_untracked(true).recurse_untracked_dirs(true);
        let statuses = service
            .repo
            .statuses(Some(&mut options))
            .map_err(|e| wrap_git2_error("inspect unstaged files", e))?;
        let paths = statuses
            .iter()
            .filter(|entry| {
                !entry.status().is_conflicted() && unstaged_change_kind(entry.status()).is_some()
            })
            .map(|entry| status_path(&entry, StatusShow::Workdir))
            .collect::<Vec<_>>();
        return update_index_paths(service, &["add", "--all"], &paths);
    }
    let output = spawn_git_command(
        &command_dir(&service.repo)?,
        &["add", "--all"],
        "stage all changes",
    )?;
    if output.status.success() {
        Ok(())
    } else {
        Err(GitError::Other(process_output_detail(&output)))
    }
}

pub(super) fn unstage_file(service: &mut GitService, path: &str) -> Result<(), GitError> {
    unstage_files(service, &[path.to_owned()])
}

pub(super) fn unstage_files(service: &GitService, paths: &[String]) -> Result<(), GitError> {
    let paths = expand_rename_sources(service, paths.to_vec(), StatusShow::Index)?;
    update_index_paths(service, &["reset", "--quiet"], &paths)
}

// Git takes the index lock for the entire operation. Literal NUL-delimited
// paths preserve wildcard characters and newlines in multiselections.
fn update_index_paths(
    service: &GitService,
    args: &[&str],
    paths: &[String],
) -> Result<(), GitError> {
    if paths.is_empty() {
        return Ok(());
    }
    if paths.iter().any(|p| p.is_empty() || p.contains('\0')) {
        return Err(GitError::Other("invalid file path".into()));
    }
    let mut command = std::process::Command::new("git");
    command
        .current_dir(command_dir(&service.repo)?)
        .arg("--literal-pathspecs")
        .args(args)
        .args(["--pathspec-from-file=-", "--pathspec-file-nul"]);
    let mut input = Vec::new();
    for path in paths {
        input.extend_from_slice(path.as_bytes());
        input.push(0);
    }
    let output = spawn_program(command, &input, "update index paths")?;
    if output.status.success() {
        Ok(())
    } else {
        Err(GitError::Other(process_output_detail(&output)))
    }
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
    unstage_files(service, &paths)
}

pub(super) fn discard_file(service: &mut GitService, path: &str) -> Result<(), GitError> {
    if path.is_empty() {
        return Err(GitError::Other("file path cannot be empty".to_string()));
    }

    let paths = expand_rename_sources(service, vec![path.to_string()], StatusShow::Workdir)?;
    let paths = expand_rename_sources(service, paths, StatusShow::Index)?;
    let head = head_object(service)?;
    update_index_paths(service, &["reset", "--quiet"], &paths)?;

    let head_tree = head.as_ref().and_then(|object| object.peel_to_tree().ok());
    for path in &paths {
        let in_head = head_tree
            .as_ref()
            .and_then(|tree| tree.get_path(Path::new(path)).ok())
            .is_some();

        if in_head {
            let output = spawn_git_command(
                &command_dir(&service.repo)?,
                &["checkout-index", "--force", "--", path],
                "restore file",
            )?;
            if !output.status.success() {
                return Err(GitError::Other(process_output_detail(&output)));
            }
        } else if let Some(workdir) = service.repo.workdir() {
            let full = workdir.join(Path::new(path));
            if full.symlink_metadata().is_ok() {
                std::fs::remove_file(&full)
                    .map_err(|e| GitError::Other(format!("failed to delete '{}': {e}", path)))?;
            }
        }
    }

    Ok(())
}

pub(super) fn discard_all_dirty_changes(service: &mut GitService) -> Result<(), GitError> {
    let repo_dir = command_dir(&service.repo)?;
    if let Some(head) = head_object(service)? {
        service
            .repo
            .reset(&head, git2::ResetType::Hard, None)
            .map_err(|e| wrap_git2_error("discard changes", e))?;
    } else {
        let output = spawn_git_command(&repo_dir, &["read-tree", "--empty"], "clear unborn index")?;
        if !output.status.success() {
            return Err(GitError::Other(process_output_detail(&output)));
        }
    }
    // Ask Git which paths are still untracked after restoring the index.
    // It also knows how to preserve ignored files and nested repositories.
    let output = spawn_git_command(&repo_dir, &["clean", "-fd"], "discard untracked files")?;
    if output.status.success() {
        Ok(())
    } else {
        Err(GitError::Other(process_output_detail(&output)))
    }
}

pub(super) fn mark_conflict_resolved(service: &mut GitService, path: &str) -> Result<(), GitError> {
    update_index_paths(service, &["add", "--all"], &[path.to_owned()])
}

pub(super) fn mark_all_conflicts_resolved(service: &mut GitService) -> Result<(), GitError> {
    let mut index = service
        .repo
        .index()
        .map_err(|e| wrap_git2_error("open repository index", e))?;
    index
        .read(true)
        .map_err(|e| wrap_git2_error("read repository index", e))?;
    let conflicts = index
        .conflicts()
        .map_err(|e| wrap_git2_error("read index conflicts", e))?;
    let mut paths = Vec::new();
    for conflict in conflicts {
        let conflict = conflict.map_err(|e| wrap_git2_error("read index conflict", e))?;
        for entry in [conflict.our, conflict.their, conflict.ancestor]
            .into_iter()
            .flatten()
        {
            paths.push(
                String::from_utf8(entry.path)
                    .map_err(|_| GitError::Other("conflicted filename is not UTF-8".into()))?,
            );
        }
    }
    paths.sort_unstable();
    paths.dedup();
    update_index_paths(service, &["add", "--all"], &paths)
}

fn expand_rename_sources(
    service: &GitService,
    mut paths: Vec<String>,
    side: StatusShow,
) -> Result<Vec<String>, GitError> {
    if paths.is_empty() {
        return Ok(paths);
    }
    let mut options = StatusOptions::new();
    options
        .show(side)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);
    let statuses = service
        .repo
        .statuses(Some(&mut options))
        .map_err(|e| wrap_git2_error("inspect renames", e))?;
    for entry in statuses.iter() {
        let Some(delta) = status_delta(&entry, side) else {
            continue;
        };
        let (Some(old), Some(new)) = (delta.old_file().path(), delta.new_file().path()) else {
            continue;
        };
        if old == new {
            continue;
        }
        let old = old.to_string_lossy().into_owned();
        let new = new.to_string_lossy().into_owned();
        if paths.contains(&new) && !paths.contains(&old) {
            paths.push(old);
        }
    }
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

fn status_delta<'a>(entry: &'a StatusEntry<'_>, side: StatusShow) -> Option<git2::DiffDelta<'a>> {
    match side {
        StatusShow::Index => entry.head_to_index(),
        StatusShow::Workdir => entry.index_to_workdir(),
        StatusShow::IndexAndWorkdir => entry.index_to_workdir().or_else(|| entry.head_to_index()),
    }
}

fn status_path(entry: &StatusEntry<'_>, side: StatusShow) -> String {
    status_delta(entry, side)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::{commit_all, init_test_repo, write_file};

    #[test]
    fn renames_show_the_actual_index_and_worktree_paths_and_stage_both_sides() {
        let (temp, repo) = init_test_repo("rename_twice");
        let mut service = GitService::open(temp.path_str()).unwrap();
        std::fs::rename(temp.path.join("tracked.txt"), temp.path.join("middle.txt")).unwrap();
        stage_file(&mut service, "middle.txt").unwrap();
        std::fs::rename(temp.path.join("middle.txt"), temp.path.join("final.txt")).unwrap();
        let changes = load_dirty_snapshot(&service).unwrap();
        assert_eq!(changes.staged_files.len(), 1);
        assert_eq!(changes.staged_files[0].path, "middle.txt");
        assert_eq!(changes.unstaged_files.len(), 1);
        assert_eq!(changes.unstaged_files[0].path, "final.txt");
        stage_file(&mut service, "final.txt").unwrap();
        let index = git2::Repository::open(repo.path())
            .unwrap()
            .index()
            .unwrap();
        assert_eq!(index.len(), 1);
        assert!(index.get_path(Path::new("final.txt"), 0).is_some());
    }

    #[test]
    fn discarding_a_literal_filename_preserves_other_staged_and_worktree_edits() {
        let (temp, repo) = init_test_repo("discard_literal");
        for path in ["item[1].txt", "item1.txt"] {
            write_file(&temp.path, path, "original");
        }
        commit_all(&repo, "add files");
        let mut service = GitService::open(temp.path_str()).unwrap();
        for path in ["item[1].txt", "item1.txt"] {
            write_file(&temp.path, path, "staged");
        }
        stage_all_dirty_changes(&mut service).unwrap();
        write_file(&temp.path, "item1.txt", "working");
        discard_file(&mut service, "item[1].txt").unwrap();
        assert_eq!(
            std::fs::read(temp.path.join("item[1].txt")).unwrap(),
            b"original"
        );
        assert_eq!(
            std::fs::read(temp.path.join("item1.txt")).unwrap(),
            b"working"
        );
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        let entry = index.get_path(Path::new("item1.txt"), 0).unwrap();
        assert_eq!(repo.find_blob(entry.id).unwrap().content(), b"staged");
    }

    #[test]
    fn discard_all_restores_recreated_file_and_preserves_ignored_files() {
        let (temp, repo) = init_test_repo("discard_recreated");
        write_file(&temp.path, ".gitignore", "ignored\n");
        commit_all(&repo, "ignore generated file");
        let mut service = GitService::open(temp.path_str()).unwrap();
        std::fs::remove_file(temp.path.join("tracked.txt")).unwrap();
        stage_file(&mut service, "tracked.txt").unwrap();
        write_file(&temp.path, "tracked.txt", "replacement");
        write_file(&temp.path, "ignored", "keep");
        write_file(&temp.path, "untracked", "discard");
        discard_all_dirty_changes(&mut service).unwrap();
        assert_eq!(
            std::fs::read(temp.path.join("tracked.txt")).unwrap(),
            b"base\n"
        );
        assert_eq!(std::fs::read(temp.path.join("ignored")).unwrap(), b"keep");
        assert!(!temp.path.join("untracked").exists());
        assert!(load_dirty_snapshot(&service).is_none());
    }

    #[test]
    fn unborn_branch_can_unstage_and_discard_files_before_the_first_commit() {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        std::fs::write(repo.path().join("info/exclude"), "ignored\n").unwrap();
        let mut service = GitService::open(temp.path().to_str().unwrap()).unwrap();
        for path in ["ignored", "first", "second"] {
            write_file(temp.path(), path, "file");
        }
        stage_all_dirty_changes(&mut service).unwrap();
        unstage_file(&mut service, "first").unwrap();
        assert_eq!(
            load_dirty_snapshot(&service).unwrap().staged_files[0].path,
            "second"
        );
        discard_all_dirty_changes(&mut service).unwrap();
        assert_eq!(std::fs::read(temp.path().join("ignored")).unwrap(), b"file");
        assert!(!temp.path().join("first").exists());
        assert!(!temp.path().join("second").exists());
        assert!(repo.index().unwrap().is_empty());
        assert!(repo.head().is_err());
    }

    /// Unstaging a staged rename must clear both the new path (the delta's
    /// visible path) and the old path's staged deletion; otherwise "unstage
    /// all" leaves the original file staged-deleted.
    #[test]
    fn unstage_all_clears_both_sides_of_a_staged_rename() {
        let (temp, repo) = init_test_repo("unstage_rename");
        write_file(&temp.path, "old_name.txt", "some content here\n");
        commit_all(&repo, "add old_name.txt");

        // Rename old_name.txt -> new_name.txt and stage both the deletion and
        // the addition so git records it as a rename.
        std::fs::rename(
            temp.path.join("old_name.txt"),
            temp.path.join("new_name.txt"),
        )
        .expect("rename file");

        let mut service = GitService::open(temp.path_str()).expect("open service");
        stage_all_dirty_changes(&mut service).expect("stage rename");

        // Sanity: the index now has a staged deletion of old_name.txt.
        let before = load_dirty_snapshot(&service).expect("dirty after staging");
        assert!(
            !before.staged_files.is_empty(),
            "rename should produce staged changes"
        );

        unstage_all_dirty_changes(&mut service).expect("unstage all");

        // Nothing should remain staged: both old and new paths are back to
        // the working tree.
        let after = load_dirty_snapshot(&service).expect("dirty after unstage");
        assert!(
            after.staged_files.is_empty(),
            "no staged changes should remain after unstage all, got: {:?}",
            after.staged_files
        );
    }
}

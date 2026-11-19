use git2::{Oid, StashFlags};

use super::checkout::workdir_has_changes;
use super::helpers::{command_dir, index_has_conflicts, process_output_detail};
use super::process::spawn_git_command;
use super::{stashes, GitService};
use crate::services::git_error::GitError;

pub(super) fn maybe_stash_workdir(
    service: &mut GitService,
    context: &str,
) -> Result<Option<Oid>, GitError> {
    if !workdir_has_changes(service)? {
        return Ok(None);
    }

    let signature = service
        .repo
        .signature()
        .or_else(|_| git2::Signature::now("Git Leviathan", "git-leviathan@example.invalid"))
        .map_err(|e| GitError::StashFailed(format!("failed to create stash signature: {e}")))?;

    let message = format!("Git Leviathan auto-stash {context}");
    match service.repo.stash_save2(
        &signature,
        Some(&message),
        Some(StashFlags::INCLUDE_UNTRACKED),
    ) {
        Ok(oid) => Ok(Some(oid)),
        // libgit2 returns ENOTFOUND when its own diff sees no stashable content,
        // even though `workdir_has_changes` flagged the tree (e.g. submodule
        // state, mode-only differences). Treat as "no stash needed".
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(e) => Err(GitError::StashFailed(format!(
            "failed to stash local changes {}: {e}",
            context
        ))),
    }
}

fn wait_for_clean_workdir(service: &GitService) -> bool {
    const POLL_INTERVAL_MS: u64 = 500;
    const MAX_POLLS: u32 = 10;

    for attempt in 0..MAX_POLLS {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        }
        if let Ok(false) = workdir_has_changes(service) {
            return true;
        }
    }
    false
}

/// Restore this operation's worktree and index even if a linked worktree
/// has pushed another stash in the meantime.
pub(super) fn apply_autostash(
    service: &GitService,
    repo_dir: &str,
    stash: Oid,
) -> Result<stashes::StashApplyStatus, GitError> {
    let hash = stash.to_string();
    let output = spawn_git_command(
        repo_dir,
        &["stash", "apply", "--index", &hash],
        "restore auto-stash",
    )?;
    if !output.status.success() {
        if index_has_conflicts(&service.repo) {
            return Ok(stashes::StashApplyStatus::Conflicted);
        }
        return Err(GitError::StashFailed(process_output_detail(&output)));
    }
    // A cleanup failure leaves a recoverable copy. The restoration has
    // already succeeded, and its staged changes must be kept.
    if let Err(error) = stashes::drop_stash(service, &hash) {
        eprintln!(
            "git_leviathan: restored auto-stash {hash}, but could not drop its entry: {error}"
        );
    }
    Ok(stashes::StashApplyStatus::Applied)
}

pub(super) fn restore_stash(service: &GitService, stash: Option<Oid>, context: &str) {
    let Some(stash) = stash else { return };
    if !wait_for_clean_workdir(service) {
        eprintln!("git_leviathan: keeping auto-stash {stash} after {context}: working tree did not settle");
        return;
    }
    let Ok(repo_dir) = command_dir(&service.repo) else {
        return;
    };
    match apply_autostash(service, &repo_dir, stash) {
        Ok(stashes::StashApplyStatus::Applied) => (),
        Ok(stashes::StashApplyStatus::Conflicted) => {
            eprintln!("git_leviathan: keeping conflicted auto-stash {stash} after {context}");
            let _ = spawn_git_command(
                &repo_dir,
                &["reset", "--hard", "HEAD"],
                "reset after failed stash restore",
            );
        }
        Err(error) => {
            eprintln!("git_leviathan: keeping auto-stash {stash} after {context}: {error}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::{init_test_repo, write_file};

    #[test]
    fn restoring_an_autostash_preserves_the_index_and_a_newer_stash() {
        let (temp, _repo) = init_test_repo("restore_autostash");
        let mut service = GitService::open(temp.path_str()).unwrap();
        write_file(&temp.path, "tracked.txt", "staged text");
        service.stage_file("tracked.txt").unwrap();
        write_file(&temp.path, "tracked.txt", "unstaged text");
        let ours = maybe_stash_workdir(&mut service, "before checkout").unwrap();
        write_file(&temp.path, "another", "independent stash");
        service.create_stash(Some("newer stash")).unwrap();
        let newer = stashes::load_stashes(&service)[0].hash.clone();
        restore_stash(&service, ours, "test");
        let dirty = service.load_dirty_snapshot().unwrap();
        assert_eq!(dirty.staged_files[0].path, "tracked.txt");
        assert_eq!(dirty.unstaged_files[0].path, "tracked.txt");
        assert_eq!(
            std::fs::read(temp.path.join("tracked.txt")).unwrap(),
            b"unstaged text"
        );
        let mut index = service.repo.index().unwrap();
        index.read(true).unwrap();
        let entry = index
            .get_path(std::path::Path::new("tracked.txt"), 0)
            .unwrap();
        assert_eq!(
            service.repo.find_blob(entry.id).unwrap().content(),
            b"staged text"
        );
        let remaining = stashes::load_stashes(&service);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].hash, newer);
        assert!(!temp.path.join("another").exists());
    }
}

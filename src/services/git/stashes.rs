use crate::services::{git_error::GitError, StashSnapshot};

use super::helpers::{command_dir, spawn_git_command};
use super::GitService;

pub(super) fn load_stashes(service: &GitService) -> Vec<StashSnapshot> {
    let Ok(reflog) = service.repo.reflog("refs/stash") else {
        return Vec::new();
    };

    let mut stashes = Vec::with_capacity(reflog.len());
    for (idx, entry) in reflog.iter().enumerate() {
        let oid = entry.id_new();
        if oid.is_zero() {
            continue;
        }
        let Ok(commit) = service.repo.find_commit(oid) else {
            continue;
        };
        let parent_hash = commit
            .parent_id(0)
            .map(|id| id.to_string())
            .unwrap_or_default();
        let index_hash = commit.parent_id(1).ok().map(|id| id.to_string());
        let author = commit.author();
        let message = entry
            .message()
            .map(|s| s.to_string())
            .or_else(|| commit.summary().map(|s| s.to_string()))
            .unwrap_or_default();

        stashes.push(StashSnapshot {
            stash_index: idx,
            hash: oid.to_string(),
            parent_hash,
            index_hash,
            message,
            author_name: author.name().unwrap_or("Unknown").to_string(),
            authored_at: author.when().seconds(),
            authored_offset_minutes: author.when().offset_minutes(),
        });
    }

    stashes
}

#[derive(Debug, Clone, Copy)]
pub(super) enum StashApplyStatus {
    Applied,
    Conflicted,
}

/// Resolve the current `stash@{N}` index of the stash whose commit is `hash`,
/// reading the live reflog. Stash indices shift as entries are pushed/popped
/// (e.g. by auto-stash during checkout/squash), so an index captured in an
/// older snapshot can point at the wrong stash — always re-resolve by the
/// stable commit hash right before operating.
fn resolve_stash_index_by_hash(service: &GitService, hash: &str) -> Option<usize> {
    let reflog = service.repo.reflog("refs/stash").ok()?;
    reflog
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.id_new().to_string() == hash)
        .map(|(idx, _)| idx)
}

fn run_stash_subcommand(
    service: &GitService,
    subcmd: &str,
    hash: &str,
) -> Result<StashApplyStatus, GitError> {
    let index = resolve_stash_index_by_hash(service, hash).ok_or_else(|| {
        GitError::Other(format!(
            "stash {hash} no longer exists (it may have been applied or dropped)"
        ))
    })?;
    let repo_dir = command_dir(&service.repo)?;
    let stash_ref = format!("stash@{{{}}}", index);
    let op = format!("stash {subcmd}");
    let output = spawn_git_command(&repo_dir, &["stash", subcmd, &stash_ref], &op)?;

    if output.status.success() {
        Ok(StashApplyStatus::Applied)
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stdout.contains("CONFLICT") || stderr.contains("CONFLICT") {
            return Ok(StashApplyStatus::Conflicted);
        }
        let stderr = stderr.trim().to_string();
        Err(GitError::Other(if stderr.is_empty() {
            format!("git stash {} failed", subcmd)
        } else {
            stderr
        }))
    }
}

pub(super) fn create_stash(
    service: &GitService,
    message: Option<&str>,
) -> Result<(), GitError> {
    let repo_dir = command_dir(&service.repo)?;
    let mut args: Vec<&str> = vec!["stash", "push", "--include-untracked"];
    if let Some(message) = message.map(str::trim).filter(|m| !m.is_empty()) {
        args.push("-m");
        args.push(message);
    }
    let output = spawn_git_command(&repo_dir, &args, "stash push")
        .map_err(|e| GitError::StashFailed(e.to_string()))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(GitError::StashFailed(if stderr.is_empty() {
            "git stash push failed".to_string()
        } else {
            stderr
        }))
    }
}

pub(super) fn apply_stash(
    service: &GitService,
    hash: &str,
) -> Result<StashApplyStatus, GitError> {
    run_stash_subcommand(service, "apply", hash)
}

pub(super) fn pop_stash(service: &GitService, hash: &str) -> Result<StashApplyStatus, GitError> {
    run_stash_subcommand(service, "pop", hash)
}

pub(super) fn drop_stash(service: &GitService, hash: &str) -> Result<(), GitError> {
    run_stash_subcommand(service, "drop", hash).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::{commit_all, init_test_repo, write_file};

    /// Dropping by the stash's stable commit hash must remove that exact stash
    /// even after later stashes shift the reflog indices. Regression: the old
    /// index-based path would drop whatever now sat at the captured index.
    #[test]
    fn drop_stash_by_hash_targets_the_right_entry_after_indices_shift() {
        let (temp, repo) = init_test_repo("stash_hash_drop");
        // Ensure there is a committed file to modify for each stash.
        write_file(&temp.path, "tracked.txt", "base\n");
        commit_all(&repo, "base for stash test");

        let mut service = GitService::open(temp.path_str()).expect("open service");

        // Stash A (will end up at a higher index as more are pushed).
        write_file(&temp.path, "tracked.txt", "change A\n");
        create_stash(&service, Some("stash A")).expect("stash A");
        let stash_a_hash = load_stashes(&service)
            .into_iter()
            .find(|s| s.message.contains("stash A"))
            .expect("stash A present")
            .hash;

        // Push two more stashes so A's index shifts (0 -> 2).
        write_file(&temp.path, "tracked.txt", "change B\n");
        create_stash(&service, Some("stash B")).expect("stash B");
        write_file(&temp.path, "tracked.txt", "change C\n");
        create_stash(&service, Some("stash C")).expect("stash C");

        // A is no longer at index 0; confirm the shift really happened.
        let before = load_stashes(&service);
        assert_eq!(before.len(), 3);
        let a_index = before
            .iter()
            .find(|s| s.hash == stash_a_hash)
            .expect("A still present")
            .stash_index;
        assert_ne!(a_index, 0, "A should have shifted off index 0");

        // Drop by A's stable hash.
        drop_stash(&mut service, &stash_a_hash).expect("drop A by hash");

        let after = load_stashes(&service);
        assert_eq!(after.len(), 2, "exactly one stash dropped");
        assert!(
            after.iter().all(|s| s.hash != stash_a_hash),
            "stash A must be the one dropped"
        );
        assert!(
            after.iter().any(|s| s.message.contains("stash B")),
            "stash B must survive"
        );
        assert!(
            after.iter().any(|s| s.message.contains("stash C")),
            "stash C must survive"
        );
    }

    #[test]
    fn drop_stash_by_missing_hash_errors_instead_of_dropping_wrong_entry() {
        let (temp, repo) = init_test_repo("stash_hash_missing");
        write_file(&temp.path, "tracked.txt", "base\n");
        commit_all(&repo, "base");
        let mut service = GitService::open(temp.path_str()).expect("open service");
        write_file(&temp.path, "tracked.txt", "change\n");
        create_stash(&service, Some("only stash")).expect("stash");

        let result = drop_stash(&mut service, "0000000000000000000000000000000000000000");
        assert!(result.is_err(), "dropping a non-existent hash must error");
        assert_eq!(load_stashes(&service).len(), 1, "the real stash survives");
    }
}

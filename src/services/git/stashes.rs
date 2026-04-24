use crate::services::{git_error::GitError, StashSnapshot};

use super::helpers::spawn_git_command;
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

fn repo_dir_str(service: &GitService) -> String {
    service
        .repo
        .workdir()
        .unwrap_or_else(|| service.repo.path())
        .to_string_lossy()
        .into_owned()
}

fn run_stash_subcommand(
    service: &GitService,
    subcmd: &str,
    index: usize,
) -> Result<StashApplyStatus, GitError> {
    let repo_dir = repo_dir_str(service);
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

pub(super) fn create_stash(service: &GitService) -> Result<(), GitError> {
    let repo_dir = repo_dir_str(service);
    let output = spawn_git_command(
        &repo_dir,
        &["stash", "push", "--include-untracked"],
        "stash push",
    )
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
    index: usize,
) -> Result<StashApplyStatus, GitError> {
    run_stash_subcommand(service, "apply", index)
}

pub(super) fn pop_stash(service: &GitService, index: usize) -> Result<StashApplyStatus, GitError> {
    run_stash_subcommand(service, "pop", index)
}

pub(super) fn drop_stash(service: &GitService, index: usize) -> Result<(), GitError> {
    run_stash_subcommand(service, "drop", index).map(|_| ())
}

use super::helpers::{find_commit_or, spawn_git_command};
use super::squash::{maybe_stash_workdir, restore_stash};
use super::GitService;
use crate::services::git_error::GitError;

#[derive(Debug, Clone, Copy)]
pub(crate) enum CherryPickStatus {
    Committed,
    Conflicted,
}

pub(super) fn cherry_pick_commit(
    service: &mut GitService,
    commit_hash: &str,
    immediate_commit: bool,
) -> Result<CherryPickStatus, GitError> {
    let oid = git2::Oid::from_str(commit_hash)
        .map_err(|e| GitError::Other(format!("invalid commit hash '{}': {}", commit_hash, e)))?;
    let _ = find_commit_or(&service.repo, oid)?;

    if !service.repo.head().is_ok_and(|h| h.is_branch()) {
        return Err(GitError::Other(
            "cannot cherry-pick with a detached HEAD".to_string(),
        ));
    }

    let repo_dir = service
        .repo
        .workdir()
        .unwrap_or_else(|| service.repo.path())
        .to_string_lossy()
        .into_owned();

    let created_stash = if immediate_commit {
        maybe_stash_workdir(service, "before cherry-pick")?
    } else {
        false
    };

    // git2 has no first-class cherry-pick path that matches CLI behaviour for
    // merge conflicts, so shell out. The CLI writes proper MERGE_MSG / CHERRY_PICK_HEAD.
    let mut args: Vec<&str> = vec!["cherry-pick"];
    if !immediate_commit {
        args.push("-n");
    }
    args.push(commit_hash);

    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo_dir)
        .args(&args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .output()
        .map_err(|e| GitError::Other(format!("failed to invoke git cherry-pick: {}", e)))?;

    if !output.status.success() {
        let has_conflicts = service
            .repo
            .index()
            .map(|idx| idx.has_conflicts())
            .unwrap_or(false);

        if has_conflicts {
            return Ok(CherryPickStatus::Conflicted);
        }

        let _ = spawn_git_command(
            &repo_dir,
            &["cherry-pick", "--abort"],
            "cherry-pick --abort",
        );
        if immediate_commit {
            restore_stash(service, created_stash, "cherry-pick abort");
        }
        return Err(GitError::Other(format!(
            "cherry-pick failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    if immediate_commit {
        restore_stash(service, created_stash, "cherry-pick");
    }

    Ok(CherryPickStatus::Committed)
}

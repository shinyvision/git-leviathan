use super::helpers::{command_dir, process_output_detail, spawn_git_command};
use super::{checkout, GitService};
use crate::services::git_error::GitError;

/// Rebase the currently checked-out branch onto `target_ref`.
///
/// `target_ref` is the argument handed to `git rebase` — a local branch
/// shorthand (`"main"`) or a remote tracking ref (`"origin/main"`).
///
/// On conflict the rebase is aborted and `GitError::Other` returned with
/// the git stderr so the caller can surface it as a toast.
pub(super) fn rebase_current_onto(
    service: &mut GitService,
    source_branch: &str,
    target_ref: &str,
) -> Result<(), GitError> {
    let source_branch = source_branch.trim();
    let target_ref = target_ref.trim();

    if source_branch.is_empty() || target_ref.is_empty() {
        return Err(GitError::EmptyBranchName);
    }
    if source_branch == target_ref {
        return Err(GitError::Other(format!(
            "cannot rebase '{}' onto itself",
            source_branch
        )));
    }
    if service.repo.state() != git2::RepositoryState::Clean {
        return Err(GitError::Other(
            "finish the current repository operation before rebasing".to_string(),
        ));
    }
    if checkout::workdir_has_changes(service)? {
        return Err(GitError::Other(
            "commit or stash your changes before rebasing".to_string(),
        ));
    }

    let current = checkout::current_branch_name(service)
        .ok_or_else(|| GitError::Other("HEAD is detached; cannot rebase".to_string()))?;
    if current != source_branch {
        return Err(GitError::Other(format!(
            "expected '{}' to be checked out, but '{}' is",
            source_branch, current
        )));
    }

    let repo_dir = command_dir(&service.repo)?;

    let output = spawn_git_command(&repo_dir, &["rebase", target_ref], "rebase")?;

    if output.status.success() {
        return Ok(());
    }

    // Conflict (or other failure) — abort so the worktree is left clean, then
    // surface the original stderr to the caller.
    let detail = process_output_detail(&output);

    if service.repo.state() != git2::RepositoryState::Clean {
        let abort = spawn_git_command(&repo_dir, &["rebase", "--abort"], "rebase --abort");
        if let Err(e) = abort {
            return Err(GitError::Other(format!(
                "rebase of '{}' onto '{}' failed ({}); additionally failed to spawn git rebase --abort: {}",
                source_branch, target_ref, detail, e
            )));
        }
    }

    Err(GitError::Other(format!(
        "rebase of '{}' onto '{}' failed: {}",
        source_branch, target_ref, detail
    )))
}

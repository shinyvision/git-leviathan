use git2::BranchType;

use super::helpers::{find_branch_or, spawn_git_command};
use super::{checkout, GitService};
use crate::services::git_error::GitError;

pub(super) enum BranchMergeStatus {
    Merged,
    Conflicted,
}

pub(super) fn merge_branch_into(
    service: &mut GitService,
    source_branch: &str,
    target_branch: &str,
) -> Result<BranchMergeStatus, GitError> {
    let source_branch = source_branch.trim();
    let target_branch = target_branch.trim();

    if source_branch.is_empty() || target_branch.is_empty() {
        return Err(GitError::EmptyBranchName);
    }
    if source_branch == target_branch {
        return Err(GitError::Other(format!(
            "cannot merge '{}' into itself",
            source_branch
        )));
    }
    if service.repo.state() != git2::RepositoryState::Clean {
        return Err(GitError::Other(
            "finish the current repository operation before merging branches".to_string(),
        ));
    }
    if checkout::workdir_has_changes(service)? {
        return Err(GitError::Other(
            "commit or stash your changes before merging branches".to_string(),
        ));
    }

    ensure_local_branch(service, source_branch)?;
    ensure_local_branch(service, target_branch)?;

    checkout::checkout_branch(service, target_branch)?;

    let message = merge_commit_message(source_branch, target_branch);
    let source_ref = format!("refs/heads/{}", source_branch);
    let repo_dir = repo_command_dir_str(service)?;
    let output = spawn_git_command(
        &repo_dir,
        &["merge", "--no-ff", "-m", &message, &source_ref],
        "merge --no-ff",
    )?;

    if output.status.success() {
        return Ok(BranchMergeStatus::Merged);
    }

    if index_has_conflicts(service) {
        return Ok(BranchMergeStatus::Conflicted);
    }

    Err(GitError::Other(format!(
        "merge of '{}' into '{}' failed: {}",
        source_branch,
        target_branch,
        merge_output_detail(&output)
    )))
}

pub(super) fn abort_merge(service: &mut GitService) -> Result<(), GitError> {
    if service.repo.state() != git2::RepositoryState::Merge {
        return Err(GitError::Other("there is no merge to abort".to_string()));
    }

    let repo_dir = repo_command_dir_str(service)?;
    let output = spawn_git_command(&repo_dir, &["merge", "--abort"], "merge --abort")?;

    if output.status.success() {
        Ok(())
    } else {
        Err(GitError::Other(format!(
            "failed to abort merge: {}",
            merge_output_detail(&output)
        )))
    }
}

pub(super) fn merge_commit_message(source_branch: &str, target_branch: &str) -> String {
    format!("Merge branch '{}' into {}", source_branch, target_branch)
}

fn ensure_local_branch(service: &GitService, branch_name: &str) -> Result<(), GitError> {
    find_branch_or(&service.repo, branch_name, BranchType::Local).map(|_| ())
}

fn repo_command_dir_str(service: &GitService) -> Result<String, GitError> {
    service
        .repo
        .workdir()
        .or_else(|| service.repo.path().parent())
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| GitError::Other("repository has no command directory".to_string()))
}

fn index_has_conflicts(service: &GitService) -> bool {
    let Ok(mut index) = service.repo.index() else {
        return false;
    };
    let _ = index.read(true);
    index.has_conflicts()
}

fn merge_output_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }

    format!("git exited with status {}", output.status)
}

#[cfg(test)]
mod tests {
    use super::merge_commit_message;

    #[test]
    fn merge_commit_message_uses_branch_into_target_format() {
        assert_eq!(
            merge_commit_message("feature-x", "main"),
            "Merge branch 'feature-x' into main"
        );
    }

    #[test]
    fn merge_commit_message_preserves_special_chars_verbatim() {
        assert_eq!(
            merge_commit_message("release/2026-04", "release/2026-05"),
            "Merge branch 'release/2026-04' into release/2026-05"
        );
    }
}

use git2::BranchType;

use super::helpers::{command_dir, find_branch_or, index_has_conflicts, process_output_detail};
use super::process::spawn_git_command;
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

    let (source_name, branch_type) =
        if let Some(remote) = source_branch.strip_prefix("refs/remotes/") {
            (remote, BranchType::Remote)
        } else {
            (
                source_branch
                    .strip_prefix("refs/heads/")
                    .unwrap_or(source_branch),
                BranchType::Local,
            )
        };
    let source_ref = find_branch_or(&service.repo, source_name, branch_type)?
        .get()
        .name()
        .ok_or_else(|| GitError::ReferenceBroken(source_branch.into()))?
        .to_owned();
    if source_ref == format!("refs/heads/{target_branch}") {
        return Err(GitError::Other(format!(
            "cannot merge '{target_branch}' into itself"
        )));
    }
    find_branch_or(&service.repo, target_branch, BranchType::Local)?;

    checkout::checkout_branch(service, target_branch)?;

    let message = merge_commit_message(source_name, target_branch);
    let repo_dir = command_dir(&service.repo)?;
    let output = spawn_git_command(
        &repo_dir,
        &["merge", "--no-ff", "-m", &message, &source_ref],
        "merge --no-ff",
    )?;

    if output.status.success() {
        return Ok(BranchMergeStatus::Merged);
    }

    if index_has_conflicts(&service.repo) {
        return Ok(BranchMergeStatus::Conflicted);
    }

    Err(GitError::Other(format!(
        "merge of '{}' into '{}' failed: {}",
        source_branch,
        target_branch,
        process_output_detail(&output)
    )))
}

pub(super) fn abort_merge(service: &mut GitService) -> Result<(), GitError> {
    if service.repo.state() != git2::RepositoryState::Merge {
        return Err(GitError::Other("there is no merge to abort".to_string()));
    }

    let repo_dir = command_dir(&service.repo)?;
    let output = spawn_git_command(&repo_dir, &["merge", "--abort"], "merge --abort")?;

    if output.status.success() {
        Ok(())
    } else {
        Err(GitError::Other(format!(
            "failed to abort merge: {}",
            process_output_detail(&output)
        )))
    }
}

pub(super) fn merge_commit_message(source_branch: &str, target_branch: &str) -> String {
    format!("Merge branch '{}' into {}", source_branch, target_branch)
}

#[cfg(test)]
mod tests {
    use super::merge_commit_message;

    #[test]
    fn remote_merge_can_finish_with_the_original_target_tree() {
        use crate::core::WorkingTreeOperation;
        use crate::services::test_support::*;
        use crate::services::{BranchMergeOutcome, GitService};
        let (temp, repo) = init_test_repo("merge_ours");
        configure_test_signature(&repo);
        let local = repo.head().unwrap().shorthand().unwrap().to_owned();
        create_branch(&repo, "source");
        checkout_branch_for_setup(&repo, "source");
        write_file(&temp.path, "tracked.txt", "incoming\n");
        let incoming = commit_all(&repo, "incoming");
        create_remote_tracking_branch(&repo, "fork/source");
        checkout_branch_for_setup(&repo, &local);
        write_file(&temp.path, "tracked.txt", "local\n");
        let local_oid = commit_all(&repo, "local");
        let mut service = GitService::open(temp.path_str()).unwrap();
        assert!(matches!(
            service
                .merge_branch_into("refs/remotes/fork/source", &local)
                .unwrap(),
            BranchMergeOutcome::Conflicted(_)
        ));
        let resolution = service.load_conflict_resolution("tracked.txt").unwrap();
        write_file(&temp.path, "tracked.txt", "externally edited\n");
        assert!(service
            .save_conflict_resolution("tracked.txt", "obsolete", resolution.source_hash)
            .is_err());
        assert_eq!(
            std::fs::read_to_string(temp.path.join("tracked.txt")).unwrap(),
            "externally edited\n"
        );
        let updated = service.load_conflict_resolution("tracked.txt").unwrap();
        service
            .save_conflict_resolution("tracked.txt", "local\n", updated.source_hash)
            .unwrap();
        let dirty = service.load_dirty_snapshot().unwrap();
        assert!(!dirty.has_changes());
        assert_eq!(dirty.operation, WorkingTreeOperation::Merge);
        let projected =
            crate::services::presenter::projection::project_loaded(service.load_repo(10))
                .projection;
        assert_eq!(projected.commits[0].operation, WorkingTreeOperation::Merge);
        assert_eq!(projected.commits.len(), projected.graph_rows.len());
        service.commit_dirty_changes("", "").unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert!(head.summary().unwrap().contains("fork/source"));
        assert_eq!(head.parent_id(0).unwrap(), local_oid);
        assert_eq!(head.parent_id(1).unwrap(), incoming);
        assert_eq!(head.tree_id(), head.parent(0).unwrap().tree_id());
    }

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

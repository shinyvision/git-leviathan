use super::helpers::{command_dir, index_has_conflicts, process_output_detail};
use super::process::spawn_git_command_with_env;
use super::{checkout, GitService};
use crate::services::git_error::GitError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebaseAction {
    Continue,
    Skip,
    Abort,
}

pub(super) fn in_progress(state: git2::RepositoryState) -> bool {
    use git2::RepositoryState::*;
    matches!(state, Rebase | RebaseInteractive | RebaseMerge)
}

/// Rebase the currently checked-out branch onto `target_ref`.
///
/// `target_ref` is the argument handed to `git rebase` — a local branch
/// shorthand (`"main"`) or a remote tracking ref (`"origin/main"`).
///
/// A conflicting step stays in Git's sequencer so it can be resolved and resumed.
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
    if source_branch == target_ref.strip_prefix("refs/heads/").unwrap_or(target_ref) {
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

    run_rebase(service, &["rebase", "--", target_ref], "rebase", true)
}

pub(super) fn resume_rebase(service: &GitService, action: RebaseAction) -> Result<(), GitError> {
    if !in_progress(service.repo.state()) {
        return Err(GitError::Other("there is no rebase in progress".into()));
    }
    match action {
        RebaseAction::Continue => {
            if index_has_conflicts(&service.repo) {
                return Err(GitError::Other(
                    "resolve and stage the remaining conflicts before continuing the rebase".into(),
                ));
            }
            run_rebase(
                service,
                &["rebase", "--continue"],
                "rebase --continue",
                true,
            )
        }
        RebaseAction::Skip => run_rebase(service, &["rebase", "--skip"], "rebase --skip", true),
        RebaseAction::Abort => run_rebase(service, &["rebase", "--abort"], "rebase --abort", false),
    }
}

fn run_rebase(
    service: &GitService,
    args: &[&str],
    op: &str,
    can_pause: bool,
) -> Result<(), GitError> {
    let output = spawn_git_command_with_env(
        &command_dir(&service.repo)?,
        args,
        op,
        &[("GIT_EDITOR", "true"), ("GIT_SEQUENCE_EDITOR", "true")],
    )?;
    if output.status.success()
        || (can_pause && in_progress(service.repo.state()) && index_has_conflicts(&service.repo))
    {
        Ok(())
    } else {
        Err(GitError::Other(format!(
            "{op} failed: {}",
            process_output_detail(&output)
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::WorkingTreeOperation;
    use crate::services::test_support::{
        checkout_branch_for_setup, commit_all, configure_test_signature, create_branch,
        init_test_repo, write_file, TempRepo,
    };

    fn diverged() -> (TempRepo, GitService, git2::Oid, git2::Oid) {
        let (temp, repo) = init_test_repo("rebase_conflicts");
        configure_test_signature(&repo);
        create_branch(&repo, "target");
        create_branch(&repo, "topic");
        checkout_branch_for_setup(&repo, "topic");
        write_file(&temp.path, "tracked.txt", "topic first\n");
        commit_all(&repo, "first change");
        write_file(&temp.path, "second.txt", "topic second\n");
        let topic = commit_all(&repo, "second change");

        checkout_branch_for_setup(&repo, "target");
        write_file(&temp.path, "tracked.txt", "target first\n");
        write_file(&temp.path, "second.txt", "target second\n");
        let target = commit_all(&repo, "target edits both");
        checkout_branch_for_setup(&repo, "topic");
        let service = GitService::open(temp.path_str()).unwrap();
        (temp, service, target, topic)
    }

    #[test]
    fn resolve_consecutive_steps_after_reopening_the_repository() {
        let (temp, mut service, target, _) = diverged();
        service
            .rebase_current_onto("topic", "refs/heads/target")
            .unwrap();

        let mut service = GitService::open(temp.path_str()).unwrap();
        for path in ["tracked.txt", "second.txt"] {
            let dirty = service.load_dirty_snapshot().unwrap();
            assert_eq!(dirty.operation, WorkingTreeOperation::Rebase);
            assert_eq!(dirty.conflicted_files[0].path, path);
            assert!(service.resume_rebase(RebaseAction::Continue).is_err());
            let original = service.load_conflict_resolution(path).unwrap();
            service
                .save_conflict_resolution(path, "resolution\n", original.source_hash)
                .unwrap();
            assert!(service.commit_dirty_changes("ordinary commit", "").is_err());
            service.resume_rebase(RebaseAction::Continue).unwrap();
        }
        assert_eq!(service.repo.state(), git2::RepositoryState::Clean);
        assert_eq!(service.current_branch_name().as_deref(), Some("topic"));
        let head = service.repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.summary(), Some("second change"));
        let first = head.parent(0).unwrap();
        assert_eq!(first.summary(), Some("first change"));
        assert_eq!(first.parent_id(0).unwrap(), target);
        assert!(service.load_dirty_snapshot().is_none());
    }

    #[test]
    fn skip_moves_to_the_next_conflicted_patch_with_the_apply_backend() {
        let (_temp, mut service, target, _) = diverged();
        service
            .repo
            .config()
            .unwrap()
            .set_str("rebase.backend", "apply")
            .unwrap();
        service.rebase_current_onto("topic", "target").unwrap();
        assert_eq!(
            service.load_dirty_snapshot().unwrap().conflicted_files[0].path,
            "tracked.txt"
        );
        service.resume_rebase(RebaseAction::Skip).unwrap();
        assert_eq!(
            service.load_dirty_snapshot().unwrap().conflicted_files[0].path,
            "second.txt"
        );
        service.resume_rebase(RebaseAction::Skip).unwrap();
        assert_eq!(service.current_branch_name().as_deref(), Some("topic"));
        assert_eq!(service.repo.head().unwrap().target(), Some(target));
        assert!(service.load_dirty_snapshot().is_none());
    }

    #[test]
    fn staging_unrelated_work_does_not_resolve_conflicts_or_allow_a_checkout() {
        let (temp, mut service, _, _) = diverged();
        service.rebase_current_onto("topic", "target").unwrap();
        write_file(&temp.path, "unrelated", "additional change\n");
        service.stage_all_dirty_changes().unwrap();
        let dirty = service.load_dirty_snapshot().unwrap();
        assert_eq!(dirty.conflicted_files[0].path, "tracked.txt");
        assert_eq!(dirty.staged_files[0].path, "unrelated");
        assert!(service.checkout_branch("target").is_err());
        assert!(service.resume_rebase(RebaseAction::Continue).is_err());
    }

    #[test]
    fn unchanged_resolution_remains_visible_and_abort_restores_original_tip() {
        let (_temp, mut service, _, topic) = diverged();
        service.rebase_current_onto("topic", "target").unwrap();
        let resolution = service.load_conflict_resolution("tracked.txt").unwrap();
        service
            .save_conflict_resolution("tracked.txt", "target first\n", resolution.source_hash)
            .unwrap();
        let dirty = service.load_dirty_snapshot().unwrap();
        assert!(!dirty.has_changes());
        assert_eq!(dirty.operation, WorkingTreeOperation::Rebase);
        service.resume_rebase(RebaseAction::Abort).unwrap();
        assert_eq!(service.current_branch_name().as_deref(), Some("topic"));
        assert_eq!(service.repo.head().unwrap().target(), Some(topic));
        assert!(service.load_dirty_snapshot().is_none());
    }
}

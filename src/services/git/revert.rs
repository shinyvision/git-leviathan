use super::helpers::{find_commit_or, spawn_git_command};
use super::squash::maybe_stash_workdir;
use super::GitService;
use crate::services::git_error::GitError;

#[derive(Debug, Clone, Copy)]
pub(crate) enum RevertStatus {
    Committed,
    Applied,
    RevertConflicted,
    StashRestoreConflicted,
}

pub(super) fn revert_commit(
    service: &mut GitService,
    commit_hash: &str,
    immediate_commit: bool,
) -> Result<RevertStatus, GitError> {
    let oid = git2::Oid::from_str(commit_hash)
        .map_err(|e| GitError::Other(format!("invalid commit hash '{}': {}", commit_hash, e)))?;
    let commit = find_commit_or(&service.repo, oid)?;
    let message =
        revert_commit_message(commit.summary().unwrap_or("(no summary)"), &oid.to_string());
    drop(commit);

    if !service.repo.head().is_ok_and(|h| h.is_branch()) {
        return Err(GitError::Other(
            "cannot revert with a detached HEAD".to_string(),
        ));
    }
    let repo_dir = repo_dir_str(service)?;
    if immediate_commit {
        revert_and_commit(service, &repo_dir, commit_hash, &message)
    } else {
        revert_in_place(service, &repo_dir, commit_hash)
    }
}

fn revert_and_commit(
    service: &mut GitService,
    repo_dir: &str,
    commit_hash: &str,
    message: &str,
) -> Result<RevertStatus, GitError> {
    let created_stash = maybe_stash_workdir(service, "before revert")?;

    let revert = spawn_git_command(repo_dir, &["revert", "--no-commit", commit_hash], "revert")?;
    if !revert.status.success() {
        if index_has_conflicts(service) {
            return Ok(RevertStatus::RevertConflicted);
        }
        restore_stash_after_failed_revert(service, repo_dir, created_stash);
        return Err(GitError::Other(format!(
            "revert failed: {}",
            output_detail(&revert)
        )));
    }

    let commit = spawn_git_command(repo_dir, &["commit", "-m", message], "commit revert")?;
    if !commit.status.success() {
        restore_stash_after_failed_revert(service, repo_dir, created_stash);
        return Err(GitError::Other(format!(
            "commit revert failed: {}",
            output_detail(&commit)
        )));
    }

    if pop_created_stash(service, repo_dir, created_stash)? {
        return Ok(RevertStatus::StashRestoreConflicted);
    }

    Ok(RevertStatus::Committed)
}

fn revert_in_place(
    service: &mut GitService,
    repo_dir: &str,
    commit_hash: &str,
) -> Result<RevertStatus, GitError> {
    if service.repo.state() != git2::RepositoryState::Clean {
        return Err(GitError::Other(
            "finish the current repository operation before reverting".to_string(),
        ));
    }

    let output = spawn_git_command(repo_dir, &["revert", "--no-commit", commit_hash], "revert")?;
    if output.status.success() {
        return Ok(RevertStatus::Applied);
    }

    if index_has_conflicts(service) || output_mentions_conflict(&output) {
        return Ok(RevertStatus::RevertConflicted);
    }

    if revert_blocked_by_working_changes(&output) {
        abort_revert(repo_dir);
        return Err(GitError::Other(
            "commit cannot be reverted in-place due to working changes".to_string(),
        ));
    }

    abort_revert(repo_dir);
    Err(GitError::Other(format!(
        "revert failed: {}",
        output_detail(&output)
    )))
}

fn pop_created_stash(
    service: &GitService,
    repo_dir: &str,
    created_stash: bool,
) -> Result<bool, GitError> {
    if !created_stash {
        return Ok(false);
    }

    let output = spawn_git_command(repo_dir, &["stash", "pop"], "stash pop after revert")?;
    if output.status.success() {
        return Ok(false);
    }
    if index_has_conflicts(service) || output_mentions_conflict(&output) {
        return Ok(true);
    }

    Err(GitError::StashFailed(format!(
        "revert commit was created, but restoring stashed changes failed: {}",
        output_detail(&output)
    )))
}

fn restore_stash_after_failed_revert(service: &GitService, repo_dir: &str, created_stash: bool) {
    if !created_stash {
        return;
    }
    if index_has_conflicts(service) {
        return;
    }
    let _ = spawn_git_command(
        repo_dir,
        &["reset", "--hard", "HEAD"],
        "reset after failed revert",
    );
    let _ = spawn_git_command(repo_dir, &["stash", "pop"], "stash pop after failed revert");
}

fn abort_revert(repo_dir: &str) {
    let _ = spawn_git_command(repo_dir, &["revert", "--abort"], "revert --abort");
}

fn index_has_conflicts(service: &GitService) -> bool {
    let Ok(mut index) = service.repo.index() else {
        return false;
    };
    let _ = index.read(true);
    index.has_conflicts()
}

fn repo_dir_str(service: &GitService) -> Result<String, GitError> {
    service
        .repo
        .workdir()
        .or_else(|| service.repo.path().parent())
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| GitError::Other("repository has no command directory".to_string()))
}

fn revert_commit_message(summary: &str, full_sha: &str) -> String {
    format!("Revert \"{}\"\n\nThis reverts commit {}", summary, full_sha)
}

fn output_mentions_conflict(output: &std::process::Output) -> bool {
    String::from_utf8_lossy(&output.stdout).contains("CONFLICT")
        || String::from_utf8_lossy(&output.stderr).contains("CONFLICT")
}

fn revert_blocked_by_working_changes(output: &std::process::Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout.contains("local changes would be overwritten")
        || stderr.contains("local changes would be overwritten")
        || stdout.contains("Your local changes")
        || stderr.contains("Your local changes")
}

fn output_detail(output: &std::process::Output) -> String {
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
    use super::*;
    use crate::services::test_support::{
        commit_all, configure_test_signature, init_test_repo, write_file,
    };
    use std::fs;

    #[test]
    fn immediate_revert_uses_exact_message_and_restores_stash() {
        let (temp, repo) = init_test_repo("revert_immediate");
        configure_test_signature(&repo);

        write_file(&temp.path, "tracked.txt", "base\nfeature\n");
        let original = commit_all(&repo, "Add feature\n\nbody");
        write_file(&temp.path, "notes.txt", "local\n");
        write_file(&temp.path, "staged.txt", "staged\n");
        let mut index = repo.index().expect("open index");
        index
            .add_path(std::path::Path::new("staged.txt"))
            .expect("stage test file");
        index.write().expect("write index");

        let mut service = GitService::open(temp.path_str()).expect("open service");
        let status = revert_commit(&mut service, &original.to_string(), true).expect("revert");

        assert!(matches!(status, RevertStatus::Committed));
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(
            head.message().unwrap().trim_end_matches('\n'),
            format!("Revert \"Add feature\"\n\nThis reverts commit {}", original)
        );
        assert_eq!(
            fs::read_to_string(temp.path.join("tracked.txt")).unwrap(),
            "base\n"
        );
        assert_eq!(
            fs::read_to_string(temp.path.join("notes.txt")).unwrap(),
            "local\n"
        );
        assert_eq!(
            fs::read_to_string(temp.path.join("staged.txt")).unwrap(),
            "staged\n"
        );
    }

    #[test]
    fn in_place_revert_conflict_leaves_hunks_for_resolution() {
        let (temp, repo) = init_test_repo("revert_in_place_conflict");
        configure_test_signature(&repo);

        write_file(&temp.path, "tracked.txt", "from-a\n");
        let original = commit_all(&repo, "A");
        write_file(&temp.path, "tracked.txt", "from-b\n");
        commit_all(&repo, "B");

        let mut service = GitService::open(temp.path_str()).expect("open service");
        let status = revert_commit(&mut service, &original.to_string(), false).expect("revert");

        assert!(matches!(status, RevertStatus::RevertConflicted));
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        assert!(index.has_conflicts());
        let conflicted = fs::read_to_string(temp.path.join("tracked.txt")).unwrap();
        assert!(conflicted.contains("<<<<<<<"));
    }

    #[test]
    fn immediate_revert_keeps_pop_conflict_for_resolution() {
        let (temp, repo) = init_test_repo("revert_pop_conflict");
        configure_test_signature(&repo);

        write_file(&temp.path, "tracked.txt", "feature\n");
        let original = commit_all(&repo, "Feature");
        write_file(&temp.path, "tracked.txt", "local\n");

        let mut service = GitService::open(temp.path_str()).expect("open service");
        let status = revert_commit(&mut service, &original.to_string(), true).expect("revert");

        assert!(matches!(status, RevertStatus::StashRestoreConflicted));
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        assert!(index.has_conflicts());
        let conflicted = fs::read_to_string(temp.path.join("tracked.txt")).unwrap();
        assert!(conflicted.contains("<<<<<<<"));
    }
}

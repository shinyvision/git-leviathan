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
        if index_has_conflicts(service) || output_mentions_conflict(&output) {
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

fn index_has_conflicts(service: &GitService) -> bool {
    let Ok(mut index) = service.repo.index() else {
        return false;
    };
    let _ = index.read(true);
    index.has_conflicts()
}

fn output_mentions_conflict(output: &std::process::Output) -> bool {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout.contains("CONFLICT")
        || stderr.contains("CONFLICT")
        || stdout.contains("after resolving the conflicts")
        || stderr.contains("after resolving the conflicts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::{
        checkout_branch_for_setup, commit_all, configure_test_signature, create_branch,
        init_test_repo, write_file,
    };
    use std::fs;

    #[test]
    fn cherry_pick_conflict_leaves_hunks_for_resolution() {
        let (temp, repo) = init_test_repo("cherry_pick_conflict");
        configure_test_signature(&repo);
        let base_branch = repo
            .head()
            .unwrap()
            .shorthand()
            .expect("head should be a branch")
            .to_string();

        create_branch(&repo, "feature/conflict");
        checkout_branch_for_setup(&repo, "feature/conflict");
        write_file(&temp.path, "tracked.txt", "feature\n");
        let picked = commit_all(&repo, "feature change");

        checkout_branch_for_setup(&repo, &base_branch);
        write_file(&temp.path, "tracked.txt", "main\n");
        commit_all(&repo, "main change");

        let mut service = GitService::open(temp.path_str()).expect("open service");
        let status =
            cherry_pick_commit(&mut service, &picked.to_string(), false).expect("cherry-pick");

        assert!(matches!(status, CherryPickStatus::Conflicted));
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        assert!(index.has_conflicts());
        let conflicted = fs::read_to_string(temp.path.join("tracked.txt")).unwrap();
        assert!(conflicted.contains("<<<<<<<"));
    }
}

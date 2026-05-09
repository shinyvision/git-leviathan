use super::helpers::{find_commit_or, find_reference_or, spawn_git_command, wrap_git2_error};
use super::squash::{maybe_stash_workdir, restore_stash};
use super::GitService;
use crate::services::git_error::GitError;

pub(super) fn reword_commit(
    service: &mut GitService,
    target_hash: &str,
    new_message: &str,
) -> Result<(), GitError> {
    let repo_dir = service
        .repo
        .workdir()
        .unwrap_or_else(|| service.repo.path())
        .to_string_lossy()
        .into_owned();

    let head_oid = service
        .repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .ok_or_else(|| GitError::Other("HEAD has no target".to_string()))?;
    if !service.repo.head().is_ok_and(|h| h.is_branch()) {
        return Err(GitError::Other(
            "cannot reword with a detached HEAD".to_string(),
        ));
    }

    let target_oid = git2::Oid::from_str(target_hash)
        .map_err(|e| GitError::Other(format!("invalid commit hash '{}': {}", target_hash, e)))?;
    {
        let target_commit = find_commit_or(&service.repo, target_oid)?;
        if target_commit.parent_count() == 0 && target_oid != head_oid {
            return Err(GitError::Other(
                "cannot reword the root commit with descendants".to_string(),
            ));
        }
    }

    if !is_reachable_via_first_parent(&service.repo, head_oid, target_oid)? {
        return Err(GitError::Other(
            "selected commit is not on current branch's first-parent chain".to_string(),
        ));
    }

    let created_stash = maybe_stash_workdir(service, "before reword")?;

    let result = reword_commit_inner(service, target_oid, new_message, head_oid, &repo_dir);

    restore_stash(service, created_stash, "reword");

    result
}

fn reword_commit_inner(
    service: &mut GitService,
    target_oid: git2::Oid,
    new_message: &str,
    head_oid: git2::Oid,
    repo_dir: &str,
) -> Result<(), GitError> {
    let target_commit = find_commit_or(&service.repo, target_oid)?;
    let author = target_commit.author().to_owned();
    let committer = service.repo.signature().unwrap_or_else(|_| author.clone());
    let tree = target_commit
        .tree()
        .map_err(|e| wrap_git2_error("read commit tree", e))?;

    let parents: Vec<git2::Commit> = (0..target_commit.parent_count())
        .filter_map(|i| target_commit.parent(i).ok())
        .collect();
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

    if target_oid == head_oid {
        let new_oid = service
            .repo
            .commit(None, &author, &committer, new_message, &tree, &parent_refs)
            .map_err(|e| wrap_git2_error("amend HEAD commit", e))?;
        drop(parents);
        drop(target_commit);

        let head_ref_name = service
            .repo
            .head()
            .ok()
            .and_then(|h| h.name().map(|s| s.to_string()))
            .ok_or_else(|| GitError::Other("HEAD ref has no name".to_string()))?;
        let mut reference = find_reference_or(&service.repo, &head_ref_name)?;
        reference
            .set_target(new_oid, "reword: amend HEAD")
            .map_err(|e| wrap_git2_error("move HEAD ref", e))?;
        service
            .repo
            .checkout_head(Some(&mut git2::build::CheckoutBuilder::new().force()))
            .map_err(|e| wrap_git2_error("checkout after amend", e))?;
        return Ok(());
    }

    if parent_refs.is_empty() {
        return Err(GitError::Other(
            "cannot reword a root commit with descendants".to_string(),
        ));
    }

    let new_oid = service
        .repo
        .commit(None, &author, &committer, new_message, &tree, &parent_refs)
        .map_err(|e| wrap_git2_error("create rewritten commit", e))?;
    drop(parents);
    drop(target_commit);

    // git2 has no first-class rebase-onto. Shell out — the rebase semantics
    // match git's own exactly, including merge driver behaviour.
    let new_oid_str = new_oid.to_string();
    let target_oid_str = target_oid.to_string();
    let mut command = std::process::Command::new("git");
    let output = crate::utils::configure_background_command(&mut command)
        .arg("-C")
        .arg(repo_dir)
        .arg("rebase")
        .arg("--onto")
        .arg(&new_oid_str)
        .arg(&target_oid_str)
        .arg("HEAD")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .output()
        .map_err(|e| GitError::Other(format!("failed to invoke git rebase: {}", e)))?;

    if !output.status.success() {
        let _ = spawn_git_command(
            repo_dir,
            &["rebase", "--abort"],
            "rebase abort after reword",
        );
        return Err(GitError::Other(format!(
            "rebase failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    Ok(())
}

fn is_reachable_via_first_parent(
    repo: &git2::Repository,
    head: git2::Oid,
    target: git2::Oid,
) -> Result<bool, GitError> {
    let mut current = head;
    loop {
        if current == target {
            return Ok(true);
        }
        let commit = find_commit_or(repo, current)?;
        if commit.parent_count() == 0 {
            return Ok(false);
        }
        current = commit
            .parent_id(0)
            .map_err(|e| wrap_git2_error("parent lookup", e))?;
    }
}

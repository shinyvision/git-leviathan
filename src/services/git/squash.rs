use git2::StashFlags;

use super::helpers::{find_commit_or, find_reference_or, spawn_git_command, wrap_git2_error};
use super::GitService;
use crate::services::git_error::GitError;

pub(super) fn maybe_stash_workdir(
    service: &mut GitService,
    context: &str,
) -> Result<bool, GitError> {
    if !workdir_has_changes(service)? {
        return Ok(false);
    }

    let signature = service
        .repo
        .signature()
        .or_else(|_| git2::Signature::now("Git Leviathan", "git-leviathan@example.invalid"))
        .map_err(|e| GitError::StashFailed(format!("failed to create stash signature: {e}")))?;

    service
        .repo
        .stash_save2(&signature, None, Some(StashFlags::INCLUDE_UNTRACKED))
        .map_err(|e| {
            GitError::StashFailed(format!("failed to stash local changes {}: {e}", context))
        })?;

    Ok(true)
}

fn workdir_has_changes(service: &GitService) -> Result<bool, GitError> {
    let mut options = git2::StatusOptions::new();
    options.include_untracked(true).recurse_untracked_dirs(true);

    let statuses = service
        .repo
        .statuses(Some(&mut options))
        .map_err(|e| wrap_git2_error("inspect worktree status", e))?;

    Ok(statuses
        .iter()
        .any(|entry| entry.status() != git2::Status::CURRENT))
}

fn wait_for_clean_workdir(service: &GitService) -> bool {
    const POLL_INTERVAL_MS: u64 = 500;
    const MAX_POLLS: u32 = 10;

    for attempt in 0..MAX_POLLS {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        }
        if let Ok(false) = workdir_has_changes(service) {
            return true;
        }
    }
    false
}

pub(super) fn restore_stash(service: &mut GitService, created_stash: bool, context: &str) {
    if !created_stash {
        return;
    }

    if !wait_for_clean_workdir(service) {
        eprintln!(
            "git_leviathan: skipping stash pop after {}: working tree did not settle",
            context
        );
        return;
    }

    let repo_dir = service
        .repo
        .workdir()
        .unwrap_or_else(|| service.repo.path())
        .to_string_lossy()
        .into_owned();

    let popped = spawn_git_command(&repo_dir, &["stash", "pop"], "stash pop after squash")
        .map(|out| out.status.success())
        .unwrap_or(false);

    if !popped {
        eprintln!(
            "git_leviathan: stash could not be applied after {}: keeping stash",
            context
        );
        let _ = spawn_git_command(
            &repo_dir,
            &["reset", "--hard", "HEAD"],
            "reset after squash stash restore",
        );
    }
}

pub(super) fn squash_commits(
    service: &mut GitService,
    hashes: Vec<String>,
) -> Result<(), GitError> {
    if hashes.len() < 2 {
        return Err(GitError::Other(
            "need at least 2 commits to squash".to_string(),
        ));
    }

    let repo = &service.repo;

    let head_ref_name = repo
        .head()
        .ok()
        .and_then(|h| h.name().map(|s| s.to_string()));
    let head_name = head_ref_name
        .as_deref()
        .ok_or_else(|| GitError::Other("cannot squash with a detached HEAD".to_string()))?;
    if !repo.head().is_ok_and(|h| h.is_branch()) {
        return Err(GitError::Other(
            "cannot squash with a detached HEAD".to_string(),
        ));
    }

    let branch_name = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "HEAD".to_string());

    let created_stash = maybe_stash_workdir(service, "before squash")?;

    let result = squash_commits_inner(service, hashes, head_name);

    restore_stash(
        service,
        created_stash,
        &format!("squash on {}", branch_name),
    );

    result
}

fn squash_commits_inner(
    service: &mut GitService,
    hashes: Vec<String>,
    head_name: &str,
) -> Result<(), GitError> {
    let repo = &service.repo;

    let commits: Vec<git2::Commit> = hashes
        .iter()
        .map(|h| {
            let oid = git2::Oid::from_str(h)
                .map_err(|e| GitError::Other(format!("invalid commit hash '{}': {}", h, e)))?;
            find_commit_or(repo, oid)
        })
        .collect::<Result<Vec<_>, GitError>>()?;

    let oldest = &commits[commits.len() - 1];
    let newest = &commits[0];

    let parent_of_oldest = oldest
        .parent(0)
        .map_err(|e| wrap_git2_error(&format!("read parent of '{}'", oldest.id()), e))?;

    for i in 1..commits.len() {
        let newer = &commits[i - 1];
        let older = &commits[i];
        let older_is_parent_of_newer = (0..newer.parent_count()).any(|p| {
            newer
                .parent(p)
                .map(|pp| pp.id() == older.id())
                .unwrap_or(false)
        });
        if !older_is_parent_of_newer {
            return Err(GitError::Other(format!(
                "selected commits are not in a contiguous chain — '{}' is not a parent of '{}'",
                older.id(),
                newer.id()
            )));
        }
    }

    let merge_base = repo
        .merge_base(parent_of_oldest.id(), newest.id())
        .map_err(|e| wrap_git2_error("find common ancestor", e))?;

    if merge_base != parent_of_oldest.id() {
        return Err(GitError::Other(
            "selected commits do not share a common ancestor with the current branch".to_string(),
        ));
    }

    let original_head = repo
        .head()
        .and_then(|h| {
            h.target()
                .ok_or_else(|| git2::Error::from_str("HEAD has no target"))
        })
        .map_err(|e| wrap_git2_error("read HEAD", e))?;

    let mut reference = find_reference_or(repo, head_name)?;

    reference
        .set_target(
            parent_of_oldest.id(),
            "squash: resetting to parent of oldest selected commit",
        )
        .map_err(|e| wrap_git2_error("move branch ref", e))?;

    let result = (|| -> Result<(), GitError> {
        repo.checkout_head(Some(&mut git2::build::CheckoutBuilder::new().force()))
            .map_err(|e| wrap_git2_error("checkout after squash reset", e))?;

        let newest_tree = newest
            .tree()
            .map_err(|e| wrap_git2_error("read newest commit tree", e))?;

        // Use checkout_tree (not index.read_tree) so files deleted by the
        // squashed commits are actually removed from the workdir. Updating
        // only the index leaves those files on disk; after commit they
        // resurface as untracked WIP entries.
        repo.checkout_tree(
            newest_tree.as_object(),
            Some(
                git2::build::CheckoutBuilder::new()
                    .force()
                    .update_index(true),
            ),
        )
        .map_err(|e| wrap_git2_error("apply newest tree to workdir", e))?;

        let mut index = repo.index().map_err(|e| wrap_git2_error("read index", e))?;
        let tree_oid = index
            .write_tree()
            .map_err(|e| wrap_git2_error("write tree", e))?;
        let tree = repo
            .find_tree(tree_oid)
            .map_err(|e| wrap_git2_error("find tree", e))?;

        let combined_message = format_squash_message(&commits);

        let signature = repo
            .signature()
            .map_err(|e| wrap_git2_error("read git author identity", e))?;

        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            &combined_message,
            &tree,
            &[&parent_of_oldest],
        )
        .map_err(|e| wrap_git2_error("create squash commit", e))?;

        repo.checkout_head(Some(&mut git2::build::CheckoutBuilder::new().force()))
            .map_err(|e| wrap_git2_error("update working tree after squash", e))?;

        Ok(())
    })();

    if result.is_err() {
        let _ = reference.set_target(original_head, "squash: restoring original HEAD");
        let _ = repo.checkout_head(Some(&mut git2::build::CheckoutBuilder::new().force()));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::super::GitService;
    use crate::services::test_support::{commit_all, init_test_repo, write_file};
    use std::fs;

    #[test]
    fn squash_with_deleted_file_removes_it_from_workdir() {
        // Regression: squashing a range whose commits delete a file used to
        // leave that file on disk as untracked, re-adding it into WIP.
        let (temp, repo) = init_test_repo("squash_delete");

        // base commit (from init_test_repo) contains tracked.txt.
        // Commit A: add doomed.txt
        write_file(&temp.path, "doomed.txt", "bye\n");
        let a = commit_all(&repo, "A: add doomed");
        // Commit B: delete doomed.txt
        fs::remove_file(temp.path.join("doomed.txt")).expect("remove doomed");
        let b = commit_all(&repo, "B: delete doomed");

        let mut service = GitService::open(temp.path_str()).expect("open service");
        service
            .squash_commits(vec![b.to_string(), a.to_string()])
            .expect("squash should succeed");

        assert!(
            !temp.path.join("doomed.txt").exists(),
            "deleted file must not resurface in workdir after squash",
        );

        let mut status_opts = git2::StatusOptions::new();
        status_opts.include_untracked(true);
        let statuses = repo
            .statuses(Some(&mut status_opts))
            .expect("read statuses");
        assert!(
            statuses.iter().all(|e| e.status() == git2::Status::CURRENT),
            "workdir must be clean after squash; got {:?}",
            statuses
                .iter()
                .map(|e| (e.path().map(str::to_string), e.status()))
                .collect::<Vec<_>>(),
        );
    }
}

fn format_squash_message(commits: &[git2::Commit]) -> String {
    let max_lines = 5;
    if commits.len() <= max_lines {
        commits
            .iter()
            .map(|c| c.summary().unwrap_or("(no summary)"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        let mut lines: Vec<String> = commits[..max_lines]
            .iter()
            .map(|c| c.summary().unwrap_or("(no summary)").to_string())
            .collect();
        lines.push(format!(
            "Squashed {} more commits",
            commits.len() - max_lines
        ));
        lines.join("\n")
    }
}

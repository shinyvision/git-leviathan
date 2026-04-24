use git2::{build::CheckoutBuilder, BranchType, StashFlags, StatusOptions};

use super::helpers::{
    find_branch_or, find_commit_or, find_reference_or, spawn_git_command, wrap_git2_error,
};
use super::GitService;
use crate::services::git_error::GitError;

pub(super) enum CheckoutRemoteBranchResult {
    Success,
    LocalAheadOfRemote {
        branch_name: String,
        remote_ref: String,
    },
}

pub(super) fn checkout_remote_branch(
    service: &mut GitService,
    branch_name: &str,
) -> Result<CheckoutRemoteBranchResult, GitError> {
    let branch_name = branch_name.trim();

    let (remote_ref, remote_oid) = resolve_remote_checkout_source(service, branch_name)?
        .ok_or_else(|| GitError::RemoteBranchNotFound(branch_name.to_string()))?;

    // If a local branch is already tracking this remote ref (possibly under a
    // different name, e.g. after a remote rename), treat the checkout as if
    // the user had clicked that local's upstream — reuse the tracking local.
    let effective_local_name =
        local_tracking_name_for(service, &remote_ref)?.unwrap_or_else(|| branch_name.to_string());
    let branch_name = effective_local_name.as_str();

    if service
        .repo
        .find_branch(branch_name, BranchType::Local)
        .is_err()
    {
        create_local_tracking_branch(service, branch_name, &remote_ref, remote_oid)?;
        checkout_branch(service, branch_name)?;
        return Ok(CheckoutRemoteBranchResult::Success);
    }

    let local_oid = {
        let local = find_branch_or(&service.repo, branch_name, BranchType::Local)?;
        local.get().target().ok_or_else(|| {
            GitError::ReferenceBroken(format!("local branch '{}' has no target", branch_name))
        })?
    };

    if local_oid == remote_oid {
        checkout_branch(service, branch_name)?;
        return Ok(CheckoutRemoteBranchResult::Success);
    }

    let remote_ahead_of_local = service
        .repo
        .graph_descendant_of(remote_oid, local_oid)
        .map_err(|e| wrap_git2_error("compare branch histories", e))?;

    if remote_ahead_of_local {
        fast_forward_and_checkout(service, branch_name, &remote_ref, remote_oid)?;
        Ok(CheckoutRemoteBranchResult::Success)
    } else {
        Ok(CheckoutRemoteBranchResult::LocalAheadOfRemote {
            branch_name: branch_name.to_string(),
            remote_ref,
        })
    }
}

pub(super) fn create_branch_from_remote_ref_and_checkout(
    service: &mut GitService,
    new_name: &str,
    remote_ref: &str,
) -> Result<(), GitError> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }

    let remote_oid = remote_ref_oid(service, remote_ref)?;
    create_local_tracking_branch(service, new_name, remote_ref, remote_oid)?;
    checkout_branch(service, new_name)
}

pub(super) fn reset_branch_to_remote_and_checkout(
    service: &mut GitService,
    branch_name: &str,
    remote_ref: &str,
) -> Result<(), GitError> {
    let remote_oid = remote_ref_oid(service, remote_ref)?;

    let ref_name = format!("refs/heads/{}", branch_name);
    service
        .repo
        .find_reference(&ref_name)
        .and_then(|mut r| r.set_target(remote_oid, &format!("reset to {}", remote_ref)))
        .map(|_| ())
        .map_err(|e| GitError::ResetFailed {
            branch: branch_name.to_string(),
            reason: format!("failed to reset '{}' to '{}': {e}", branch_name, remote_ref),
        })?;

    service
        .repo
        .set_head(&ref_name)
        .map_err(|e| GitError::ResetFailed {
            branch: branch_name.to_string(),
            reason: format!("failed to update HEAD to '{}': {e}", branch_name),
        })?;

    let mut checkout = CheckoutBuilder::new();
    checkout.force();
    service
        .repo
        .checkout_head(Some(&mut checkout))
        .map_err(|e| GitError::ResetFailed {
            branch: branch_name.to_string(),
            reason: format!("failed to reset working tree to '{}': {e}", branch_name),
        })
}

pub(super) fn create_branch_at_commit(
    service: &mut GitService,
    branch_name: &str,
    commit_hash: &str,
) -> Result<(), GitError> {
    let branch_name = branch_name.trim();
    if branch_name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }

    if branch_name.ends_with('/') {
        return Err(GitError::InvalidBranchName(branch_name.to_string()));
    }

    if branch_name.contains("..")
        || branch_name.contains('~')
        || branch_name.contains('^')
        || branch_name.contains(':')
    {
        return Err(GitError::InvalidBranchName(branch_name.to_string()));
    }

    let oid = git2::Oid::from_str(commit_hash)
        .map_err(|e| GitError::Other(format!("invalid commit hash: {e}")))?;
    let target_oid = find_commit_or(&service.repo, oid)?;

    if service
        .repo
        .find_branch(branch_name, BranchType::Local)
        .is_ok()
    {
        return Err(GitError::BranchAlreadyExists(branch_name.to_string()));
    }

    service
        .repo
        .branch(branch_name, &target_oid, false)
        .map_err(|e| wrap_git2_error(&format!("create branch '{branch_name}'"), e))?;

    Ok(())
}

pub(super) fn delete_branch(
    service: &mut GitService,
    branch_name: &str,
    is_remote: bool,
) -> Result<(), GitError> {
    let branch_name = branch_name.trim();
    if branch_name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }

    if !is_remote && current_branch_name(service).as_deref() == Some(branch_name) {
        return Err(GitError::CantDeleteHead(branch_name.to_string()));
    }

    if is_remote {
        let (remote_ref, _) = resolve_remote_checkout_source(service, branch_name)?
            .ok_or_else(|| GitError::RemoteBranchNotFound(branch_name.to_string()))?;
        delete_remote_branch_ref(service, &remote_ref)
    } else {
        let mut branch = find_branch_or(&service.repo, branch_name, BranchType::Local)?;
        branch
            .delete()
            .map_err(|e| wrap_git2_error(&format!("delete branch '{branch_name}'"), e))
    }
}

pub(super) fn rename_branch(
    service: &mut GitService,
    old_name: &str,
    new_name: &str,
    is_remote: bool,
) -> Result<(), GitError> {
    let old_name = old_name.trim();
    let new_name = new_name.trim();

    if old_name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }
    if new_name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }

    if !is_remote && current_branch_name(service).as_deref() == Some(old_name) {
        return Err(GitError::CantRenameHead(old_name.to_string()));
    }

    if is_remote {
        let (remote_ref, _) = resolve_remote_checkout_source(service, old_name)?
            .ok_or_else(|| GitError::RemoteBranchNotFound(old_name.to_string()))?;

        let branch = find_branch_or(&service.repo, &remote_ref, BranchType::Remote)?;

        let target_oid = branch.get().target().ok_or_else(|| {
            GitError::ReferenceBroken(format!("remote branch '{}' has no target", remote_ref))
        })?;

        let remote_name = remote_ref
            .split_once('/')
            .map(|(remote, _)| remote)
            .unwrap_or("origin");
        let new_remote_ref = format!("{}/{}", remote_name, new_name);
        let new_full_ref = format!("refs/remotes/{}", new_remote_ref);

        service
            .repo
            .reference(
                &new_full_ref,
                target_oid,
                false,
                &format!("rename from {}", old_name),
            )
            .map_err(|e| {
                wrap_git2_error(&format!("create remote branch ref '{new_remote_ref}'"), e)
            })?;

        if let Ok(mut old_ref) = service
            .repo
            .find_reference(&format!("refs/remotes/{}", remote_ref))
        {
            old_ref.delete().map_err(|e| {
                wrap_git2_error(&format!("delete old remote branch ref '{remote_ref}'"), e)
            })?;
        }

        // Repoint any local branch that was tracking the old remote ref so
        // it now tracks the new one. Without this, `git push` would still
        // push to the old remote name using the stale upstream config.
        repoint_local_upstreams(service, &remote_ref, &new_remote_ref)?;
    } else {
        let mut branch = find_branch_or(&service.repo, old_name, BranchType::Local)?;
        branch
            .rename(new_name, false)
            .map_err(|e| wrap_git2_error(&format!("rename '{old_name}' to '{new_name}'"), e))?;
    }

    Ok(())
}

pub(super) fn delete_branch_all(
    service: &mut GitService,
    branch_name: &str,
) -> Result<(), GitError> {
    let branch_name = branch_name.trim();
    if branch_name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }

    if current_branch_name(service).as_deref() == Some(branch_name) {
        return Err(GitError::CantDeleteHead(branch_name.to_string()));
    }

    if service
        .repo
        .find_branch(branch_name, BranchType::Local)
        .is_ok()
    {
        let mut branch = find_branch_or(&service.repo, branch_name, BranchType::Local)?;
        branch
            .delete()
            .map_err(|e| wrap_git2_error(&format!("delete local branch '{branch_name}'"), e))?;
    }

    for remote_ref in matching_remote_branch_refs(service, branch_name)? {
        delete_remote_branch_ref(service, &remote_ref)?;
    }

    Ok(())
}

pub(super) fn checkout_branch(service: &mut GitService, branch_name: &str) -> Result<(), GitError> {
    let branch_name = branch_name.trim();
    if branch_name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }

    let previous_branch = current_branch_name(service);
    if previous_branch.as_deref() == Some(branch_name) {
        return Ok(());
    }

    let previous_head_ref = current_head_ref(service);
    ensure_local_checkout_branch(service, branch_name)?;

    let created_stash = maybe_stash_workdir(service, previous_branch.is_some(), "before checkout")?;

    if let Err(err) = checkout_local_branch(service, branch_name, created_stash) {
        rollback_checkout(service, previous_head_ref.as_deref(), created_stash);
        return Err(err);
    }

    restore_stash(
        service,
        created_stash,
        previous_branch.as_deref(),
        branch_name,
        "checkout",
    );

    Ok(())
}

pub(super) fn current_branch_name(service: &GitService) -> Option<String> {
    service
        .repo
        .head()
        .ok()
        .filter(|head| head.is_branch())
        .and_then(|head| head.shorthand().map(|name| name.to_string()))
}

pub(super) fn workdir_has_changes(service: &GitService) -> Result<bool, GitError> {
    let mut options = StatusOptions::new();
    options.include_untracked(true).recurse_untracked_dirs(true);

    let statuses = service
        .repo
        .statuses(Some(&mut options))
        .map_err(|e| wrap_git2_error("inspect worktree status", e))?;

    Ok(statuses
        .iter()
        .any(|entry| entry.status() != git2::Status::CURRENT))
}

fn current_head_ref(service: &GitService) -> Option<String> {
    service
        .repo
        .head()
        .ok()
        .and_then(|head| head.name().map(|name| name.to_string()))
}

fn maybe_stash_workdir(
    service: &mut GitService,
    has_previous_branch: bool,
    context: &str,
) -> Result<bool, GitError> {
    if !has_previous_branch || !workdir_has_changes(service)? {
        return Ok(false);
    }

    let signature = service
        .repo
        .signature()
        .or_else(|_| git2::Signature::now("Git Leviathan", "git-leviathan@example.invalid"))
        .map_err(|e| GitError::StashFailed(format!("failed to create stash signature: {e}")))?;

    match service
        .repo
        .stash_save2(&signature, None, Some(StashFlags::INCLUDE_UNTRACKED))
    {
        Ok(_) => Ok(true),
        // libgit2 returns ENOTFOUND when its own diff sees no stashable content,
        // even though `workdir_has_changes` flagged the tree (e.g. submodule
        // state, mode-only differences). Treat as "no stash needed".
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(false),
        Err(e) => Err(GitError::StashFailed(format!(
            "failed to stash local changes {}: {e}",
            context
        ))),
    }
}

fn rollback_checkout(
    service: &mut GitService,
    previous_head_ref: Option<&str>,
    created_stash: bool,
) {
    if let Some(previous_head_ref) = previous_head_ref {
        let _ = restore_head(service, previous_head_ref);
    }

    if created_stash {
        let _ = service.repo.stash_pop(0, None);
    }
}

fn wait_for_clean_workdir(service: &GitService) -> bool {
    const POLL_INTERVAL_MS: u64 = 500;
    const MAX_POLLS: u32 = 10; // up to 5 seconds

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

fn restore_stash(
    service: &mut GitService,
    created_stash: bool,
    previous_branch: Option<&str>,
    branch_name: &str,
    action: &str,
) {
    if !created_stash {
        return;
    }

    // After checkout git's working tree is temporarily dirty while files settle.
    // Poll on 500 ms intervals until clean before attempting the pop.
    if !wait_for_clean_workdir(service) {
        eprintln!(
            "git_leviathan: skipping stash pop after {} to {}: working tree did not settle",
            action, branch_name
        );
        return;
    }

    let repo_dir = service
        .repo
        .workdir()
        .unwrap_or_else(|| service.repo.path())
        .to_string_lossy()
        .into_owned();

    // Use git CLI for stash pop: git2's stash_pop skips conflicts silently when
    // the working tree is clean (it fuzzy-applies rather than 3-way merging).
    // The CLI performs a proper 3-way merge and exits non-zero on conflict while
    // keeping the stash entry intact.
    let popped = spawn_git_command(&repo_dir, &["stash", "pop"], "stash pop after checkout")
        .map(|out| out.status.success())
        .unwrap_or(false);

    if !popped {
        let old_branch = previous_branch.unwrap_or("HEAD");
        eprintln!(
            "git_leviathan: stash for WIP on {} could not be applied after {} to {}: keeping stash",
            old_branch, action, branch_name
        );
        // The stash entry is preserved automatically when pop fails.
        // Reset the working tree and index back to HEAD to clear any partial
        // conflict state left by the failed pop.
        let _ = spawn_git_command(
            &repo_dir,
            &["reset", "--hard", "HEAD"],
            "reset after stash pop failure",
        );
    }
}

fn ensure_local_checkout_branch(
    service: &mut GitService,
    branch_name: &str,
) -> Result<(), GitError> {
    if service
        .repo
        .find_branch(branch_name, BranchType::Local)
        .is_ok()
    {
        return Ok(());
    }

    let (remote_ref_name, remote_oid) = resolve_remote_checkout_source(service, branch_name)?
        .ok_or_else(|| {
            GitError::BranchNotFound(format!(
                "branch '{}' does not exist locally or on a remote",
                branch_name
            ))
        })?;

    create_local_tracking_branch(service, branch_name, &remote_ref_name, remote_oid)
}

fn resolve_remote_checkout_source(
    service: &GitService,
    branch_name: &str,
) -> Result<Option<(String, git2::Oid)>, GitError> {
    let mut matches = Vec::new();

    if let Ok(branches) = service.repo.branches(Some(BranchType::Remote)) {
        for branch_result in branches.flatten() {
            let (branch, _) = branch_result;
            let full_name = match branch.name() {
                Ok(Some(name)) if !name.ends_with("/HEAD") => name.to_string(),
                _ => continue,
            };
            let Some((_, short_name)) = full_name.split_once('/') else {
                continue;
            };
            if short_name != branch_name {
                continue;
            }

            let Some(target_oid) = branch.get().target() else {
                continue;
            };
            matches.push((full_name, target_oid));
        }
    }

    if matches.is_empty() {
        return Ok(None);
    }

    if matches.len() == 1 {
        return Ok(matches.into_iter().next());
    }

    let preferred = format!("origin/{}", branch_name);
    let preferred_matches = matches
        .iter()
        .filter(|(full_name, _)| full_name == &preferred)
        .count();

    if preferred_matches == 1 {
        return Ok(matches
            .into_iter()
            .find(|(full_name, _)| full_name == &preferred));
    }

    let options = matches
        .iter()
        .map(|(full_name, _)| full_name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(GitError::Other(format!(
        "branch '{}' matches multiple remote refs: {}",
        branch_name, options
    )))
}

/// Find the name of a local branch whose upstream is `remote_ref` (e.g.
/// "origin/other-branch"). Returns the local short name if found.
fn local_tracking_name_for(
    service: &GitService,
    remote_ref: &str,
) -> Result<Option<String>, GitError> {
    let branches = service
        .repo
        .branches(Some(BranchType::Local))
        .map_err(|e| wrap_git2_error("list local branches", e))?;

    for branch_result in branches {
        let (branch, _) =
            branch_result.map_err(|e| wrap_git2_error("iterate local branches", e))?;

        let upstream = match branch.upstream() {
            Ok(upstream) => upstream,
            Err(_) => continue,
        };
        let upstream_name = match upstream.name() {
            Ok(Some(name)) => name.to_string(),
            _ => continue,
        };
        if upstream_name != remote_ref {
            continue;
        }
        if let Ok(Some(local_name)) = branch.name() {
            return Ok(Some(local_name.to_string()));
        }
    }

    Ok(None)
}

/// Repoint local branches whose upstream was `old_remote_ref` (e.g.
/// "origin/my-branch") to `new_remote_ref`. Used after a client-side remote
/// rename so subsequent `git push` resolves to the new remote branch.
fn repoint_local_upstreams(
    service: &GitService,
    old_remote_ref: &str,
    new_remote_ref: &str,
) -> Result<(), GitError> {
    let branches = service
        .repo
        .branches(Some(BranchType::Local))
        .map_err(|e| wrap_git2_error("list local branches", e))?;

    for branch_result in branches {
        let (mut branch, _) =
            branch_result.map_err(|e| wrap_git2_error("iterate local branches", e))?;

        let upstream = match branch.upstream() {
            Ok(upstream) => upstream,
            Err(_) => continue,
        };
        let upstream_name = match upstream.name() {
            Ok(Some(name)) => name.to_string(),
            _ => continue,
        };
        if upstream_name != old_remote_ref {
            continue;
        }

        branch
            .set_upstream(Some(new_remote_ref))
            .map_err(|e| wrap_git2_error(&format!("repoint upstream to '{new_remote_ref}'"), e))?;
    }

    Ok(())
}

fn matching_remote_branch_refs(
    service: &GitService,
    branch_name: &str,
) -> Result<Vec<String>, GitError> {
    let mut matches = Vec::new();

    if let Ok(branches) = service.repo.branches(Some(BranchType::Remote)) {
        for branch_result in branches {
            let (branch, _) =
                branch_result.map_err(|e| wrap_git2_error("iterate remote branches", e))?;
            let full_name = match branch.name() {
                Ok(Some(name)) if !name.ends_with("/HEAD") => name.to_string(),
                _ => continue,
            };

            if full_name
                .split_once('/')
                .map(|(_, short_name)| short_name == branch_name)
                .unwrap_or(false)
            {
                matches.push(full_name);
            }
        }
    }

    Ok(matches)
}

fn delete_remote_branch_ref(service: &mut GitService, remote_ref: &str) -> Result<(), GitError> {
    let (remote_name, remote_branch_name) = remote_ref.split_once('/').ok_or_else(|| {
        GitError::RemoteBranchNotFound(format!(
            "remote branch '{}' is missing a remote name",
            remote_ref
        ))
    })?;

    if service.repo.find_remote(remote_name).is_ok() {
        push_delete_remote_branch(service, remote_name, remote_branch_name)?;
    }

    delete_remote_tracking_branch_ref(service, remote_ref)
}

fn push_delete_remote_branch(
    service: &GitService,
    remote_name: &str,
    remote_branch_name: &str,
) -> Result<(), GitError> {
    let repo_dir = service
        .repo
        .workdir()
        .unwrap_or_else(|| service.repo.path())
        .to_string_lossy()
        .into_owned();
    let refspec = format!(":refs/heads/{}", remote_branch_name);
    let op = format!("push delete {remote_name}/{remote_branch_name}");
    let output =
        spawn_git_command(&repo_dir, &["push", remote_name, &refspec], &op).map_err(|e| {
            GitError::RemoteDeleteFailed {
                remote: remote_name.to_string(),
                branch: remote_branch_name.to_string(),
                reason: e.to_string(),
            }
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("git exited with status {}", output.status)
    };

    Err(GitError::RemoteDeleteFailed {
        remote: remote_name.to_string(),
        branch: remote_branch_name.to_string(),
        reason: detail,
    })
}

fn delete_remote_tracking_branch_ref(
    service: &GitService,
    remote_ref: &str,
) -> Result<(), GitError> {
    let Ok(mut branch) = service.repo.find_branch(remote_ref, BranchType::Remote) else {
        return Ok(());
    };

    branch
        .delete()
        .map_err(|e| wrap_git2_error(&format!("delete remote branch '{remote_ref}'"), e))
}

fn checkout_local_branch(
    service: &GitService,
    branch_name: &str,
    after_stash: bool,
) -> Result<(), GitError> {
    let branch = service
        .repo
        .find_branch(branch_name, BranchType::Local)
        .map_err(|e| GitError::CheckoutFailed {
            branch: branch_name.to_string(),
            reason: format!("failed to find local branch: {e}"),
        })?;
    let reference = branch.get();
    let reference_name = reference.name().ok_or_else(|| {
        GitError::ReferenceBroken(format!(
            "branch '{}' does not have a valid reference name",
            branch_name
        ))
    })?;

    let target_commit = reference
        .peel_to_commit()
        .map_err(|e| GitError::CheckoutFailed {
            branch: branch_name.to_string(),
            reason: format!("failed to resolve branch: {e}"),
        })?;

    let mut checkout = CheckoutBuilder::new();
    if after_stash {
        // After a stash the working tree is clean, so force is correct.
        // Using safe here is wrong: safe mode compares the working tree to the
        // target commit and sees the old branch's files as "local modifications",
        // causing it to skip updating them and leaving the tree dirty.
        checkout.force();
    } else {
        checkout.safe();
    }
    service
        .repo
        .checkout_tree(target_commit.as_object(), Some(&mut checkout))
        .map_err(|e| GitError::CheckoutFailed {
            branch: branch_name.to_string(),
            reason: format!("failed to checkout: {e}"),
        })?;

    service
        .repo
        .set_head(reference_name)
        .map_err(|e| GitError::CheckoutFailed {
            branch: branch_name.to_string(),
            reason: format!("failed to update HEAD: {e}"),
        })
}

fn create_local_tracking_branch(
    service: &mut GitService,
    branch_name: &str,
    remote_ref: &str,
    remote_oid: git2::Oid,
) -> Result<(), GitError> {
    let remote_commit = find_commit_or(&service.repo, remote_oid)?;
    let mut local_branch = service
        .repo
        .branch(branch_name, &remote_commit, false)
        .map_err(|e| wrap_git2_error(&format!("create local branch '{branch_name}'"), e))?;
    local_branch
        .set_upstream(Some(remote_ref))
        .map_err(|e| wrap_git2_error(&format!("track '{remote_ref}'"), e))
}

fn remote_ref_oid(service: &GitService, remote_ref: &str) -> Result<git2::Oid, GitError> {
    let full = format!("refs/remotes/{}", remote_ref);
    service
        .repo
        .find_reference(&full)
        .or_else(|_| service.repo.find_reference(remote_ref))
        .map_err(|_| GitError::RemoteBranchNotFound(remote_ref.to_string()))?
        .target()
        .ok_or_else(|| {
            GitError::ReferenceBroken(format!("remote ref '{}' has no direct target", remote_ref))
        })
}

fn fast_forward_and_checkout(
    service: &mut GitService,
    branch_name: &str,
    remote_ref: &str,
    remote_oid: git2::Oid,
) -> Result<(), GitError> {
    let previous_branch = current_branch_name(service);
    let previous_head_ref = current_head_ref(service);

    let created_stash = maybe_stash_workdir(
        service,
        previous_branch.is_some(),
        "before fast-forward checkout",
    )?;

    {
        let target_commit =
            service
                .repo
                .find_commit(remote_oid)
                .map_err(|e| GitError::FastForwardFailed {
                    branch: branch_name.to_string(),
                    reason: format!("failed to find remote commit: {e}"),
                })?;
        let mut checkout = CheckoutBuilder::new();
        checkout.safe();
        service
            .repo
            .checkout_tree(target_commit.as_object(), Some(&mut checkout))
            .map_err(|e| GitError::FastForwardFailed {
                branch: branch_name.to_string(),
                reason: format!("failed to checkout fast-forward tree: {e}"),
            })?;
    }

    let ref_name = format!("refs/heads/{}", branch_name);
    let update_result: Result<(), GitError> = service
        .repo
        .find_reference(&ref_name)
        .and_then(|mut r| r.set_target(remote_oid, &format!("fast-forward: {}", remote_ref)))
        .map(|_| ())
        .map_err(|e| GitError::FastForwardFailed {
            branch: branch_name.to_string(),
            reason: format!(
                "failed to advance '{}' to '{}': {e}",
                branch_name, remote_ref
            ),
        });

    if let Err(e) = update_result {
        rollback_checkout(service, previous_head_ref.as_deref(), created_stash);
        return Err(e);
    }

    if let Err(e) = service
        .repo
        .set_head(&ref_name)
        .map_err(|e| GitError::FastForwardFailed {
            branch: branch_name.to_string(),
            reason: format!("failed to update HEAD: {e}"),
        })
    {
        rollback_checkout(service, previous_head_ref.as_deref(), created_stash);
        return Err(e);
    }

    restore_stash(
        service,
        created_stash,
        previous_branch.as_deref(),
        branch_name,
        "fast-forward checkout",
    );

    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

pub(super) fn reset_current_branch_to_commit(
    service: &mut GitService,
    commit_hash: &str,
    mode: ResetMode,
) -> Result<(), GitError> {
    let oid = git2::Oid::from_str(commit_hash)
        .map_err(|e| GitError::Other(format!("invalid commit hash: {e}")))?;
    let commit = find_commit_or(&service.repo, oid)?;
    let current = current_branch_name(service)
        .ok_or_else(|| GitError::Other("no current branch to reset".to_string()))?;

    let reset_type = match mode {
        ResetMode::Soft => git2::ResetType::Soft,
        ResetMode::Mixed => git2::ResetType::Mixed,
        ResetMode::Hard => git2::ResetType::Hard,
    };

    service
        .repo
        .reset(commit.as_object(), reset_type, None)
        .map_err(|e| GitError::ResetFailed {
            branch: current,
            reason: format!("failed to reset: {e}"),
        })
}

pub(super) fn fast_forward_branch_to_branch(
    service: &mut GitService,
    source_branch: &str,
    target_branch: &str,
) -> Result<(), GitError> {
    let source_branch = source_branch.trim();
    let target_branch = target_branch.trim();
    if source_branch.is_empty() || target_branch.is_empty() {
        return Err(GitError::EmptyBranchName);
    }
    if source_branch == target_branch {
        return Err(GitError::Other(format!(
            "cannot fast-forward '{}' to itself",
            source_branch
        )));
    }

    let target_oid = find_branch_or(&service.repo, target_branch, BranchType::Local)?
        .get()
        .target()
        .ok_or_else(|| {
            GitError::ReferenceBroken(format!("branch '{}' has no target", target_branch))
        })?;
    let source_oid = find_branch_or(&service.repo, source_branch, BranchType::Local)?
        .get()
        .target()
        .ok_or_else(|| {
            GitError::ReferenceBroken(format!("branch '{}' has no target", source_branch))
        })?;

    if source_oid == target_oid {
        return Ok(());
    }

    let is_ancestor = service
        .repo
        .graph_descendant_of(target_oid, source_oid)
        .map_err(|e| wrap_git2_error("compare branch histories", e))?;
    if !is_ancestor {
        return Err(GitError::FastForwardFailed {
            branch: source_branch.to_string(),
            reason: format!(
                "'{}' is not an ancestor of '{}'",
                source_branch, target_branch
            ),
        });
    }

    let is_current = current_branch_name(service).as_deref() == Some(source_branch);

    if is_current {
        if workdir_has_changes(service)? {
            return Err(GitError::FastForwardFailed {
                branch: source_branch.to_string(),
                reason: "commit or stash your changes before fast-forwarding".to_string(),
            });
        }

        let commit =
            find_commit_or(&service.repo, target_oid).map_err(|e| GitError::FastForwardFailed {
                branch: source_branch.to_string(),
                reason: format!("failed to load target commit: {e}"),
            })?;
        let mut checkout = CheckoutBuilder::new();
        checkout.safe();
        service
            .repo
            .checkout_tree(commit.as_object(), Some(&mut checkout))
            .map_err(|e| GitError::FastForwardFailed {
                branch: source_branch.to_string(),
                reason: format!("failed to checkout target tree: {e}"),
            })?;
    }

    let ref_name = format!("refs/heads/{}", source_branch);
    service
        .repo
        .find_reference(&ref_name)
        .and_then(|mut r| r.set_target(target_oid, &format!("fast-forward to {}", target_branch)))
        .map(|_| ())
        .map_err(|e| GitError::FastForwardFailed {
            branch: source_branch.to_string(),
            reason: format!("failed to advance '{}': {e}", source_branch),
        })?;

    if is_current {
        service
            .repo
            .set_head(&ref_name)
            .map_err(|e| GitError::FastForwardFailed {
                branch: source_branch.to_string(),
                reason: format!("failed to update HEAD: {e}"),
            })?;
    }

    Ok(())
}

pub(super) fn is_branch_ancestor_of_branch(
    service: &GitService,
    ancestor_branch: &str,
    descendant_branch: &str,
) -> Result<bool, GitError> {
    if ancestor_branch == descendant_branch {
        return Ok(false);
    }
    let Ok(a) = service.repo.find_branch(ancestor_branch, BranchType::Local) else {
        return Ok(false);
    };
    let Ok(d) = service
        .repo
        .find_branch(descendant_branch, BranchType::Local)
    else {
        return Ok(false);
    };
    let Some(a_oid) = a.get().target() else {
        return Ok(false);
    };
    let Some(d_oid) = d.get().target() else {
        return Ok(false);
    };
    if a_oid == d_oid {
        return Ok(false);
    }
    service
        .repo
        .graph_descendant_of(d_oid, a_oid)
        .map_err(|e| wrap_git2_error("compare branch histories", e))
}

fn restore_head(service: &GitService, reference_name: &str) -> Result<(), GitError> {
    let _ = find_reference_or(&service.repo, reference_name)?;
    service
        .repo
        .set_head(reference_name)
        .map_err(|e| wrap_git2_error(&format!("restore HEAD to '{reference_name}'"), e))?;

    let mut checkout = CheckoutBuilder::new();
    checkout.safe();
    service
        .repo
        .checkout_head(Some(&mut checkout))
        .map_err(|e| wrap_git2_error(&format!("restore working tree for '{reference_name}'"), e))
}

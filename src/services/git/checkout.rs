use git2::{build::CheckoutBuilder, BranchType, StatusOptions};

use super::autostash::{maybe_stash_workdir, restore_stash};
use super::helpers::{
    command_dir, find_branch_or, find_commit_or, find_reference_or, process_output_detail,
    wrap_git2_error,
};
use super::hooks::{self, PostHook};
use super::process::spawn_git_command_with_timeout;
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

    let (remote_ref, remote_oid) = resolve_remote_branch_input(service, branch_name)?
        .ok_or_else(|| GitError::RemoteBranchNotFound(branch_name.to_string()))?;
    let remote_branch_name = remote_branch_short_name(&remote_ref)?.to_string();

    // If a local branch is already tracking this remote ref (possibly under a
    // different name, e.g. after a remote rename), treat the checkout as if
    // the user had clicked that local's upstream — reuse the tracking local.
    let effective_local_name =
        local_tracking_name_for(service, &remote_ref)?.unwrap_or(remote_branch_name);
    let branch_name = effective_local_name.as_str();

    if service
        .repo
        .find_branch(branch_name, BranchType::Local)
        .is_err()
    {
        ensure_checkout_available(service, branch_name)?;
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

    let (remote_ref, remote_oid) = resolve_remote_ref_oid(service, remote_ref)?;
    ensure_checkout_available(service, new_name)?;
    create_local_tracking_branch(service, new_name, &remote_ref, remote_oid)?;
    checkout_branch(service, new_name)
}

pub(super) fn reset_branch_to_remote_and_checkout(
    service: &mut GitService,
    branch_name: &str,
    remote_ref: &str,
) -> Result<(), GitError> {
    let (remote_ref, remote_oid) = resolve_remote_ref_oid(service, remote_ref)?;
    let previous = hooks::head_commit(&service.repo);

    ensure_checkout_available(service, branch_name)?;
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
        })?;

    run_post_checkout(service, previous);
    Ok(())
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

    if !git2::Branch::name_is_valid(branch_name).unwrap_or(false) {
        return Err(GitError::InvalidBranchName(branch_name.into()));
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
    force: bool,
) -> Result<(), GitError> {
    let branch_name = branch_name.trim();
    if branch_name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }

    if !is_remote && current_branch_name(service).as_deref() == Some(branch_name) {
        return Err(GitError::CantDeleteHead(branch_name.to_string()));
    }

    if is_remote {
        let (remote_ref, _) = resolve_remote_branch_input(service, branch_name)?
            .ok_or_else(|| GitError::RemoteBranchNotFound(branch_name.to_string()))?;
        delete_remote_branch_ref(service, &remote_ref)
    } else {
        ensure_branch_not_in_other_worktree(service, branch_name)?;
        let mut branch = find_branch_or(&service.repo, branch_name, BranchType::Local)?;
        // git2's `branch.delete()` is unconditional (unlike `git branch -d`),
        // so a non-force delete must first confirm the branch is fully merged
        // — otherwise unmerged commits silently become unreachable.
        if !force && !local_branch_is_merged(service, &branch)? {
            return Err(GitError::Other(format!(
                "branch '{branch_name}' is not fully merged; delete with force to discard its commits",
            )));
        }
        branch
            .delete()
            .map_err(|e| wrap_git2_error(&format!("delete branch '{branch_name}'"), e))
    }
}

/// A local branch is safe to delete when its tip is already reachable from
/// its upstream, falling back to HEAD for a branch without one.
fn local_branch_is_merged(
    service: &GitService,
    branch: &git2::Branch<'_>,
) -> Result<bool, GitError> {
    let Some(tip) = branch.get().target() else {
        return Ok(true);
    };
    let base = branch
        .upstream()
        .ok()
        .and_then(|u| u.get().target())
        .or_else(|| service.repo.head().ok().and_then(|h| h.target()));
    let Some(base) = base else {
        return Ok(false);
    };
    if base == tip {
        return Ok(true);
    }
    service
        .repo
        .graph_descendant_of(base, tip)
        .map_err(|e| wrap_git2_error("check merged branch", e))
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
        let (remote_ref, _) = resolve_remote_branch_input(service, old_name)?
            .ok_or_else(|| GitError::RemoteBranchNotFound(old_name.to_string()))?;

        rename_remote_branch(service, &remote_ref, new_name)?;
    } else {
        ensure_branch_not_in_other_worktree(service, old_name)?;
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

    ensure_branch_not_in_other_worktree(service, branch_name)?;
    for remote_ref in matching_remote_branch_refs(service, branch_name)? {
        delete_remote_branch_ref(service, &remote_ref)?;
    }
    if let Ok(mut branch) = service.repo.find_branch(branch_name, BranchType::Local) {
        branch
            .delete()
            .map_err(|e| wrap_git2_error(&format!("delete local branch '{branch_name}'"), e))?;
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

    ensure_checkout_available(service, branch_name)?;
    let previous_head_ref = current_head_ref(service);
    let previous = hooks::head_commit(&service.repo);
    ensure_local_checkout_branch(service, branch_name)?;

    let created_stash = if previous_branch.is_some() {
        maybe_stash_workdir(service, "before checkout")?
    } else {
        None
    };

    if let Err(err) = checkout_local_branch(service, branch_name, created_stash.is_some()) {
        rollback_checkout(service, previous_head_ref.as_deref(), created_stash);
        return Err(err);
    }

    restore_stash(
        service,
        created_stash,
        &format!("checkout to {branch_name}"),
    );

    run_post_checkout(service, previous);
    Ok(())
}

/// Run the post-checkout hook for a branch checkout that moved HEAD away from
/// `previous`. libgit2 does not run hooks, git would.
fn run_post_checkout(service: &GitService, previous: git2::Oid) {
    let current = hooks::head_commit(&service.repo);
    hooks::run(
        &service.repo,
        PostHook::Checkout {
            previous,
            current,
            branch_checkout: true,
        },
    );
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

fn ensure_checkout_available(service: &GitService, branch_name: &str) -> Result<(), GitError> {
    if service.repo.state() != git2::RepositoryState::Clean {
        return Err(GitError::Other(
            "finish the current repository operation before switching or moving branches".into(),
        ));
    }
    ensure_branch_not_in_other_worktree(service, branch_name)
}

fn ensure_branch_not_in_other_worktree(
    service: &GitService,
    branch_name: &str,
) -> Result<(), GitError> {
    if current_branch_name(service).as_deref() != Some(branch_name) {
        if let Some(owner) = super::worktrees::list_worktrees(service)?
            .into_iter()
            .find(|worktree| worktree.branch_name == branch_name)
        {
            return Err(GitError::CheckoutFailed {
                branch: branch_name.into(),
                reason: format!("already checked out at '{}'", owner.path.display()),
            });
        }
    }
    Ok(())
}

fn current_head_ref(service: &GitService) -> Option<String> {
    service
        .repo
        .head()
        .ok()
        .and_then(|head| head.name().map(|name| name.to_string()))
}

fn rollback_checkout(
    service: &mut GitService,
    previous_head_ref: Option<&str>,
    created_stash: Option<git2::Oid>,
) {
    if let Some(previous_head_ref) = previous_head_ref {
        let _ = restore_head(service, previous_head_ref);
    }

    restore_stash(service, created_stash, "checkout rollback");
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

    let (remote_ref_name, remote_oid) = resolve_remote_branch_by_short_name(service, branch_name)?
        .ok_or_else(|| {
            GitError::BranchNotFound(format!(
                "branch '{}' does not exist locally or on a remote",
                branch_name
            ))
        })?;

    create_local_tracking_branch(service, branch_name, &remote_ref_name, remote_oid)
}

fn resolve_remote_branch_input(
    service: &GitService,
    remote_ref_or_branch_name: &str,
) -> Result<Option<(String, git2::Oid)>, GitError> {
    let input = remote_ref_or_branch_name.trim();
    let (candidate, is_qualified_ref) = remote_ref_candidate(input);

    if let Some(source) = resolve_exact_remote_ref(service, candidate)? {
        return Ok(Some(source));
    }

    if is_qualified_ref {
        return Ok(None);
    }

    resolve_remote_branch_by_short_name(service, candidate)
}

fn resolve_remote_branch_by_short_name(
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

    let options = matches
        .iter()
        .map(|(full_name, _)| full_name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(GitError::Other(format!(
        "branch '{}' matches multiple remote refs: {}; use an exact remote ref",
        branch_name, options
    )))
}

fn resolve_exact_remote_ref(
    service: &GitService,
    remote_ref: &str,
) -> Result<Option<(String, git2::Oid)>, GitError> {
    if remote_ref.is_empty() || !remote_ref.contains('/') || remote_ref.ends_with("/HEAD") {
        return Ok(None);
    }

    let branch = match service.repo.find_branch(remote_ref, BranchType::Remote) {
        Ok(branch) => branch,
        Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(e) => {
            return Err(wrap_git2_error(
                &format!("find remote branch '{remote_ref}'"),
                e,
            ));
        }
    };

    let target_oid = branch.get().target().ok_or_else(|| {
        GitError::ReferenceBroken(format!("remote branch '{}' has no target", remote_ref))
    })?;

    Ok(Some((remote_ref.to_string(), target_oid)))
}

fn resolve_remote_ref_oid(
    service: &GitService,
    remote_ref: &str,
) -> Result<(String, git2::Oid), GitError> {
    resolve_remote_branch_input(service, remote_ref)?
        .ok_or_else(|| GitError::RemoteBranchNotFound(remote_ref.trim().to_string()))
}

fn remote_ref_candidate(input: &str) -> (&str, bool) {
    match input.strip_prefix("refs/remotes/") {
        Some(remote_ref) => (remote_ref, true),
        None => (input, false),
    }
}

fn remote_branch_short_name(remote_ref: &str) -> Result<&str, GitError> {
    remote_ref
        .split_once('/')
        .map(|(_, name)| name)
        .ok_or_else(|| {
            GitError::RemoteBranchNotFound(format!(
                "remote branch '{}' is missing a remote name",
                remote_ref
            ))
        })
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

/// Create the new remote head and remove the old one in one push. Both
/// leases refer to the state the user selected when initiating the rename.
fn rename_remote_branch(
    service: &GitService,
    remote_ref: &str,
    new_name: &str,
) -> Result<(), GitError> {
    if !git2::Branch::name_is_valid(new_name).unwrap_or(false) {
        return Err(GitError::InvalidBranchName(new_name.into()));
    }
    let tracking_ref = format!("refs/remotes/{remote_ref}");
    let reference = find_reference_or(&service.repo, &tracking_ref)?;
    let tip = reference
        .target()
        .ok_or_else(|| GitError::ReferenceBroken(tracking_ref.clone()))?;
    let (remote_name, old_ref) = remote_branch_destination(service, remote_ref)?
        .ok_or_else(|| GitError::RemoteBranchNotFound(remote_ref.into()))?;
    let new_ref = format!("refs/heads/{new_name}");
    if old_ref == new_ref {
        return Ok(());
    }
    let tracking_locals = local_branches_tracking(service, &tracking_ref)?;
    let output = spawn_git_command_with_timeout(
        &command_dir(&service.repo)?,
        &[
            "push",
            "--atomic",
            &format!("--force-with-lease={old_ref}:{tip}"),
            &format!("--force-with-lease={new_ref}:"),
            "--",
            &remote_name,
            &format!("{tip}:{new_ref}"),
            &format!(":{old_ref}"),
        ],
        "rename remote branch",
        std::time::Duration::from_secs(300),
    )?;
    if !output.status.success() {
        return Err(GitError::Other(format!(
            "remote rename failed: {}",
            process_output_detail(&output)
        )));
    }
    let mut config = service
        .repo
        .config()
        .map_err(|e| wrap_git2_error("open config", e))?;
    for local in tracking_locals {
        config
            .set_str(&format!("branch.{local}.merge"), &new_ref)
            .map_err(|e| {
                wrap_git2_error("remote renamed but failed to update local upstream", e)
            })?;
    }
    Ok(())
}

fn local_branches_tracking(
    service: &GitService,
    tracking_ref: &str,
) -> Result<Vec<String>, GitError> {
    let mut locals = Vec::new();
    for entry in service
        .repo
        .branches(Some(BranchType::Local))
        .map_err(|e| wrap_git2_error("list local branches", e))?
    {
        let (branch, _) = entry.map_err(|e| wrap_git2_error("read local branch", e))?;
        let Some(name) = branch.get().name() else {
            continue;
        };
        if service
            .repo
            .branch_upstream_name(name)
            .is_ok_and(|buf| buf.as_str() == Some(tracking_ref))
        {
            locals.push(name.trim_start_matches("refs/heads/").to_owned());
        }
    }
    Ok(locals)
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

/// Reverse the configured fetch refspec to find the real name on the server.
fn remote_branch_destination(
    service: &GitService,
    remote_ref: &str,
) -> Result<Option<(String, String)>, GitError> {
    let tracking_ref = format!("refs/remotes/{remote_ref}");
    let remote_name = match service.repo.branch_remote_name(&tracking_ref) {
        Ok(name) => name,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(error) => return Err(wrap_git2_error("find tracking remote", error)),
    };
    let remote_name = remote_name
        .as_str()
        .ok_or_else(|| GitError::Other("remote name is not UTF-8".into()))?;
    let remote = service
        .repo
        .find_remote(remote_name)
        .map_err(|e| wrap_git2_error("find remote", e))?;
    let old_ref = remote
        .refspecs()
        .find(|spec| spec.direction() == git2::Direction::Fetch && spec.dst_matches(&tracking_ref))
        .ok_or_else(|| GitError::RemoteBranchNotFound(remote_ref.into()))?
        .rtransform(&tracking_ref)
        .map_err(|e| wrap_git2_error("resolve remote branch", e))?;
    let old_ref = old_ref
        .as_str()
        .filter(|name| name.starts_with("refs/heads/"))
        .ok_or_else(|| GitError::RemoteBranchNotFound(remote_ref.into()))?;
    Ok(Some((remote_name.to_owned(), old_ref.to_owned())))
}

fn delete_remote_branch_ref(service: &mut GitService, remote_ref: &str) -> Result<(), GitError> {
    if let Some((remote, head_ref)) = remote_branch_destination(service, remote_ref)? {
        push_delete_remote_branch(service, &remote, &head_ref)?;
    }
    delete_remote_tracking_branch_ref(service, remote_ref)
}

fn push_delete_remote_branch(
    service: &GitService,
    remote_name: &str,
    remote_head_ref: &str,
) -> Result<(), GitError> {
    let repo_dir = command_dir(&service.repo)?;
    let remote_branch_name = remote_head_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(remote_head_ref);
    let refspec = format!(":{remote_head_ref}");
    let op = format!("push delete {remote_name}/{remote_branch_name}");
    let output = spawn_git_command_with_timeout(
        &repo_dir,
        &["push", "--", remote_name, &refspec],
        &op,
        std::time::Duration::from_secs(300),
    )
    .map_err(|e| GitError::RemoteDeleteFailed {
        remote: remote_name.to_string(),
        branch: remote_branch_name.to_string(),
        reason: e.to_string(),
    })?;

    if output.status.success() {
        return Ok(());
    }

    Err(GitError::RemoteDeleteFailed {
        remote: remote_name.to_string(),
        branch: remote_branch_name.to_string(),
        reason: process_output_detail(&output),
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

fn fast_forward_and_checkout(
    service: &mut GitService,
    branch_name: &str,
    remote_ref: &str,
    remote_oid: git2::Oid,
) -> Result<(), GitError> {
    let previous_branch = current_branch_name(service);
    ensure_checkout_available(service, branch_name)?;
    let previous_head_ref = current_head_ref(service);
    let previous = hooks::head_commit(&service.repo);

    let created_stash = if previous_branch.is_some() {
        maybe_stash_workdir(service, "before fast-forward checkout")?
    } else {
        None
    };

    let target_commit = find_commit_or(&service.repo, remote_oid)?;
    let mut checkout = CheckoutBuilder::new();
    checkout.safe();
    if let Err(error) = service
        .repo
        .checkout_tree(target_commit.as_object(), Some(&mut checkout))
    {
        restore_stash(service, created_stash, "failed fast-forward checkout");
        return Err(GitError::FastForwardFailed {
            branch: branch_name.to_owned(),
            reason: format!("failed to checkout fast-forward tree: {error}"),
        });
    }
    drop(target_commit);

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
        &format!("fast-forward checkout to {branch_name}"),
    );

    run_post_checkout(service, previous);
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

    ensure_checkout_available(service, source_branch)?;
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
        // Git runs post-merge for a fast-forward of the current branch.
        hooks::run(&service.repo, PostHook::Merge { squash: false });
    }

    Ok(())
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

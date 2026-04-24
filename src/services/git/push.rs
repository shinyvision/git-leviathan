use git2::BranchType;

use super::helpers::{find_branch_or, spawn_git_command, wrap_git2_error};
use super::GitService;
use crate::services::git_error::GitError;

pub enum PushOutcome {
    Pushed,
    /// Current branch has no upstream; UI should prompt for a remote-tracking
    /// name and call `push_and_set_upstream`. `remote_name` prefers "origin",
    /// else the first remote.
    NeedsUpstream {
        branch_name: String,
        remote_name: String,
    },
    /// Push rejected because local tip is behind the remote.
    BehindRemote {
        branch_name: String,
        remote_name: String,
    },
}

pub(super) fn push_current_branch(service: &GitService) -> Result<PushOutcome, GitError> {
    let branch_name = current_branch_shorthand(service)?;

    let (remote_name, upstream_short) = match resolve_upstream(service, &branch_name)? {
        Some(pair) => pair,
        None => {
            let remote_name = default_remote(service)?;
            return Ok(PushOutcome::NeedsUpstream {
                branch_name,
                remote_name,
            });
        }
    };

    let refspec = format!("HEAD:refs/heads/{}", upstream_short);
    run_git_push(service, false, &remote_name, &refspec)
}

pub(super) fn push_and_set_upstream(
    service: &GitService,
    remote_name: &str,
    remote_branch_name: &str,
) -> Result<(), GitError> {
    let remote_branch_name = remote_branch_name.trim();
    if remote_branch_name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }

    let refspec = format!("HEAD:refs/heads/{}", remote_branch_name);
    match run_git_push(service, true, remote_name, &refspec)? {
        PushOutcome::Pushed => Ok(()),
        PushOutcome::NeedsUpstream { .. } => Ok(()),
        PushOutcome::BehindRemote { branch_name, .. } => Err(GitError::Other(format!(
            "'{}' is behind its remote counterpart; pull first",
            branch_name
        ))),
    }
}

pub(super) fn force_push_current_branch(service: &GitService) -> Result<PushOutcome, GitError> {
    let branch_name = current_branch_shorthand(service)?;

    let (remote_name, upstream_short) = match resolve_upstream(service, &branch_name)? {
        Some(pair) => pair,
        None => {
            let remote_name = default_remote(service)?;
            return Ok(PushOutcome::NeedsUpstream {
                branch_name,
                remote_name,
            });
        }
    };

    let refspec = format!("HEAD:refs/heads/{}", upstream_short);
    let repo_dir = repo_dir_str(service);

    let output = spawn_git_command(
        &repo_dir,
        &["push", "--force", &remote_name, &refspec],
        "push --force",
    )?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(GitError::Other(if stderr.is_empty() {
            "git push --force failed".to_string()
        } else {
            stderr
        }));
    }

    Ok(PushOutcome::Pushed)
}

pub(super) fn pull_current_branch(service: &GitService) -> Result<(), GitError> {
    let repo_dir = repo_dir_str(service);

    let ff_output = spawn_git_command(&repo_dir, &["pull", "--ff-only"], "pull --ff-only")?;

    if ff_output.status.success() {
        return Ok(());
    }

    let merge_output = spawn_git_command(&repo_dir, &["pull", "--no-ff"], "pull --no-ff")?;

    if !merge_output.status.success() {
        let stderr = String::from_utf8_lossy(&merge_output.stderr)
            .trim()
            .to_string();
        return Err(GitError::Other(if stderr.is_empty() {
            "git pull failed".to_string()
        } else {
            stderr
        }));
    }

    Ok(())
}

fn current_branch_shorthand(service: &GitService) -> Result<String, GitError> {
    service
        .repo
        .head()
        .ok()
        .filter(|head| head.is_branch())
        .and_then(|head| head.shorthand().map(|s| s.to_string()))
        .ok_or_else(|| GitError::Other("no branch is currently checked out".to_string()))
}

fn default_remote(service: &GitService) -> Result<String, GitError> {
    let remotes: Vec<String> = service
        .repo
        .remotes()
        .map_err(|e| wrap_git2_error("list remotes", e))?
        .iter()
        .flatten()
        .map(|s| s.to_string())
        .collect();

    if remotes.is_empty() {
        return Err(GitError::Other("no remotes configured".to_string()));
    }

    Ok(remotes
        .iter()
        .find(|r| r.as_str() == "origin")
        .cloned()
        .unwrap_or_else(|| remotes[0].clone()))
}

/// Returns (remote_name, remote_short_branch_name) from the upstream of
/// `branch_name`, if one is configured. The short name is the remote branch
/// without the remote prefix — e.g. upstream "origin/other" → ("origin", "other").
fn resolve_upstream(
    service: &GitService,
    branch_name: &str,
) -> Result<Option<(String, String)>, GitError> {
    let branch = find_branch_or(&service.repo, branch_name, BranchType::Local)?;

    let upstream = match branch.upstream() {
        Ok(upstream) => upstream,
        Err(_) => return Ok(None),
    };

    let upstream_name = match upstream.name() {
        Ok(Some(name)) => name.to_string(),
        _ => return Ok(None),
    };

    let (remote, short) = match upstream_name.split_once('/') {
        Some((remote, short)) => (remote.to_string(), short.to_string()),
        None => return Ok(None),
    };

    Ok(Some((remote, short)))
}

fn repo_dir_str(service: &GitService) -> String {
    service
        .repo
        .workdir()
        .unwrap_or_else(|| service.repo.path())
        .to_string_lossy()
        .into_owned()
}

fn run_git_push(
    service: &GitService,
    set_upstream: bool,
    remote_name: &str,
    refspec: &str,
) -> Result<PushOutcome, GitError> {
    let repo_dir = repo_dir_str(service);

    let mut args: Vec<&str> = vec!["push"];
    if set_upstream {
        args.push("--set-upstream");
    }
    args.push(remote_name);
    args.push(refspec);

    let output = spawn_git_command(&repo_dir, &args, "push")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains("[rejected]") || stderr.contains("non-fast-forward") {
            let branch_name = refspec
                .trim_start_matches("HEAD:")
                .trim_start_matches("refs/heads/")
                .to_string();
            return Ok(PushOutcome::BehindRemote {
                branch_name,
                remote_name: remote_name.to_string(),
            });
        }
        return Err(GitError::Other(if stderr.is_empty() {
            "git push failed".to_string()
        } else {
            stderr
        }));
    }

    Ok(PushOutcome::Pushed)
}

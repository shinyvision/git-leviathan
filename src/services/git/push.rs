use git2::BranchType;

use super::helpers::{command_dir, find_branch_or, spawn_git_command, wrap_git2_error};
use super::GitService;
use crate::services::git_error::GitError;

pub enum PushOutcome {
    Pushed,
    /// Current branch has no upstream configured. UI should prompt for a
    /// remote-tracking name and call `push_and_set_upstream`.
    NeedsUpstream {
        /// Short name of the current local branch.
        branch_name: String,
        /// Remote suggested for the first upstream push.
        remote_name: String,
    },
    /// Current branch tip is behind its remote counterpart; push was rejected.
    BehindRemote {
        branch_name: String,
        remote_name: String,
    },
}

pub(super) fn push_current_branch(service: &GitService) -> Result<PushOutcome, GitError> {
    let branch_name = current_branch_shorthand(service)?;

    let upstream = resolve_upstream(service, &branch_name)?;
    let remote_name = configured_push_remote(
        service,
        &branch_name,
        upstream.as_ref().map(|(remote, _)| remote.as_str()),
    )?;
    match upstream {
        Some(_) => run_git_push_current_branch(service, false, &branch_name, &remote_name),
        None => Ok(PushOutcome::NeedsUpstream {
            branch_name,
            remote_name,
        }),
    }
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
    match run_git_push(service, true, remote_name, &refspec, remote_branch_name)? {
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

    let upstream = resolve_upstream(service, &branch_name)?;
    let remote_name = configured_push_remote(
        service,
        &branch_name,
        upstream.as_ref().map(|(remote, _)| remote.as_str()),
    )?;
    match upstream {
        Some(_) => run_git_push_current_branch(service, true, &branch_name, &remote_name),
        None => Ok(PushOutcome::NeedsUpstream {
            branch_name,
            remote_name,
        }),
    }
}

pub(super) fn push_ref_to_remote(
    service: &GitService,
    remote_name: &str,
    ref_name: &str,
) -> Result<PushOutcome, GitError> {
    ensure_remote_exists(service, remote_name)?;
    let branch_name = resolve_explicit_local_branch(service, ref_name)?;
    let source_ref = format!("refs/heads/{branch_name}");
    let destination_ref = format!("refs/heads/{branch_name}");
    let refspec = format!("{source_ref}:{destination_ref}");
    run_git_push(service, false, remote_name, &refspec, &branch_name)
}

pub(super) fn current_branch_push_remote(service: &GitService) -> Result<String, GitError> {
    let branch_name = current_branch_shorthand(service)?;
    let upstream = resolve_upstream(service, &branch_name)?;
    configured_push_remote(
        service,
        &branch_name,
        upstream.as_ref().map(|(remote, _)| remote.as_str()),
    )
}

pub(super) fn pull_current_branch(service: &GitService) -> Result<(), GitError> {
    let repo_dir = command_dir(&service.repo)?;

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

fn config_string(service: &GitService, key: &str) -> Option<String> {
    service
        .repo
        .config()
        .ok()
        .and_then(|config| config.get_string(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn configured_push_remote(
    service: &GitService,
    branch_name: &str,
    upstream_remote: Option<&str>,
) -> Result<String, GitError> {
    if let Some(remote) = config_string(service, &format!("branch.{branch_name}.pushRemote")) {
        return Ok(remote);
    }
    if let Some(remote) = config_string(service, "remote.pushDefault") {
        return Ok(remote);
    }
    if let Some(remote) = upstream_remote {
        return Ok(remote.to_string());
    }
    default_remote(service)
}

fn ensure_remote_exists(service: &GitService, remote_name: &str) -> Result<(), GitError> {
    if remote_name.trim().is_empty() {
        return Err(GitError::Other("remote name cannot be empty".to_string()));
    }
    service
        .repo
        .find_remote(remote_name)
        .map(|_| ())
        .map_err(|e| wrap_git2_error(&format!("find remote '{remote_name}'"), e))
}

fn resolve_explicit_local_branch(service: &GitService, ref_name: &str) -> Result<String, GitError> {
    let ref_name = ref_name.trim();
    if ref_name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }
    if ref_name.contains(':') {
        return Err(GitError::Other(
            "explicit push ref must be a local branch name, not a refspec".to_string(),
        ));
    }

    let branch_name = ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name);
    if branch_name.is_empty() {
        return Err(GitError::EmptyBranchName);
    }
    if branch_name.starts_with("refs/") || branch_name == "HEAD" {
        return Err(GitError::Other(
            "explicit push ref must name a local branch under refs/heads".to_string(),
        ));
    }

    find_branch_or(&service.repo, branch_name, BranchType::Local)?;
    Ok(branch_name.to_string())
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

fn run_git_push(
    service: &GitService,
    set_upstream: bool,
    remote_name: &str,
    refspec: &str,
    remote_branch_name: &str,
) -> Result<PushOutcome, GitError> {
    let repo_dir = command_dir(&service.repo)?;

    let mut args: Vec<&str> = vec!["push"];
    if set_upstream {
        args.push("--set-upstream");
    }
    args.push(remote_name);
    args.push(refspec);

    let output = spawn_git_command(&repo_dir, &args, "push")?;
    handle_git_push_output(output, remote_name, remote_branch_name, "git push failed")
}

fn run_git_push_current_branch(
    service: &GitService,
    force: bool,
    branch_name: &str,
    remote_name: &str,
) -> Result<PushOutcome, GitError> {
    let repo_dir = command_dir(&service.repo)?;

    let mut args: Vec<&str> = vec!["push"];
    if force {
        args.push("--force");
    }

    let op = if force { "push --force" } else { "push" };
    let output = spawn_git_command(&repo_dir, &args, op)?;
    handle_git_push_output(output, remote_name, branch_name, "git push failed")
}

fn handle_git_push_output(
    output: std::process::Output,
    remote_name: &str,
    remote_branch_name: &str,
    fallback_error: &str,
) -> Result<PushOutcome, GitError> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains("[rejected]") || stderr.contains("non-fast-forward") {
            return Ok(PushOutcome::BehindRemote {
                branch_name: remote_branch_name.to_string(),
                remote_name: remote_name.to_string(),
            });
        }
        return Err(GitError::Other(if stderr.is_empty() {
            fallback_error.to_string()
        } else {
            stderr
        }));
    }

    Ok(PushOutcome::Pushed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::{
        add_remote, checkout_branch_for_setup, commit_all, create_branch, fetch_remote,
        init_bare_test_repo, init_test_repo, push_refspec, write_file,
    };
    use git2::{BranchType, Oid, Repository};

    fn ensure_main_checked_out(repo: &Repository) {
        if repo.find_branch("main", BranchType::Local).is_err() {
            create_branch(repo, "main");
        }
        checkout_branch_for_setup(repo, "main");
    }

    fn add_file_remote(repo: &Repository, name: &str, remote_path: &str) {
        add_remote(repo, name, &format!("file://{remote_path}"));
    }

    fn bare_branch_target(repo_path: &str, branch_name: &str) -> Oid {
        Repository::open_bare(repo_path)
            .expect("failed to open bare remote")
            .find_reference(&format!("refs/heads/{branch_name}"))
            .expect("remote branch should exist")
            .target()
            .expect("remote branch should point at an oid")
    }

    fn setup_tracked_main_with_remotes(
        prefix: &str,
    ) -> (
        crate::services::test_support::TempRepo,
        crate::services::test_support::TempRepo,
        crate::services::test_support::TempRepo,
        Oid,
        Oid,
    ) {
        let (temp_repo, repo) = init_test_repo(prefix);
        let (upstream_temp, _upstream_repo) = init_bare_test_repo(&format!("{prefix}_upstream"));
        let (fork_temp, _fork_repo) = init_bare_test_repo(&format!("{prefix}_fork"));

        ensure_main_checked_out(&repo);
        add_file_remote(&repo, "upstream", upstream_temp.path_str());
        add_file_remote(&repo, "fork", fork_temp.path_str());

        let base_oid = repo
            .head()
            .expect("repo should have head")
            .target()
            .expect("head should point at oid");
        push_refspec(&repo, "upstream", "refs/heads/main:refs/heads/main");
        push_refspec(&repo, "fork", "refs/heads/main:refs/heads/main");
        fetch_remote(&repo, "upstream");
        repo.find_branch("main", BranchType::Local)
            .expect("main branch should exist")
            .set_upstream(Some("upstream/main"))
            .expect("failed to set upstream");

        write_file(&temp_repo.path, "tracked.txt", "local ahead\n");
        let pushed_oid = commit_all(&repo, "local ahead");

        (temp_repo, upstream_temp, fork_temp, base_oid, pushed_oid)
    }

    #[test]
    fn push_ref_to_remote_pushes_local_branch_without_setting_upstream() {
        let (temp_repo, repo) = init_test_repo("push_explicit_branch");
        let (fork_temp, _fork_repo) = init_bare_test_repo("push_explicit_branch_fork");

        ensure_main_checked_out(&repo);
        add_file_remote(&repo, "fork", fork_temp.path_str());
        write_file(&temp_repo.path, "tracked.txt", "explicit push\n");
        let pushed_oid = commit_all(&repo, "explicit push");

        let service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let outcome =
            push_ref_to_remote(&service, "fork", "main").expect("explicit push should succeed");

        assert!(matches!(outcome, PushOutcome::Pushed));
        assert_eq!(bare_branch_target(fork_temp.path_str(), "main"), pushed_oid);
        let repo = Repository::open(temp_repo.path_str()).expect("failed to reopen repo");
        let branch = repo
            .find_branch("main", BranchType::Local)
            .expect("main branch should exist");
        assert!(
            branch.upstream().is_err(),
            "explicit push must not set upstream"
        );
    }

    #[test]
    fn push_current_branch_honors_branch_push_remote() {
        let (temp_repo, upstream_temp, fork_temp, upstream_base_oid, pushed_oid) =
            setup_tracked_main_with_remotes("push_branch_push_remote");
        let repo = Repository::open(temp_repo.path_str()).expect("failed to reopen repo");
        repo.config()
            .expect("failed to open config")
            .set_str("branch.main.pushRemote", "fork")
            .expect("failed to set pushRemote");

        let service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let outcome = push_current_branch(&service).expect("push should succeed");

        assert!(matches!(outcome, PushOutcome::Pushed));
        assert_eq!(bare_branch_target(fork_temp.path_str(), "main"), pushed_oid);
        assert_eq!(
            bare_branch_target(upstream_temp.path_str(), "main"),
            upstream_base_oid
        );
    }

    #[test]
    fn push_current_branch_honors_remote_push_default() {
        let (temp_repo, upstream_temp, fork_temp, upstream_base_oid, pushed_oid) =
            setup_tracked_main_with_remotes("push_remote_push_default");
        let repo = Repository::open(temp_repo.path_str()).expect("failed to reopen repo");
        repo.config()
            .expect("failed to open config")
            .set_str("remote.pushDefault", "fork")
            .expect("failed to set remote.pushDefault");

        let service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let outcome = push_current_branch(&service).expect("push should succeed");

        assert!(matches!(outcome, PushOutcome::Pushed));
        assert_eq!(bare_branch_target(fork_temp.path_str(), "main"), pushed_oid);
        assert_eq!(
            bare_branch_target(upstream_temp.path_str(), "main"),
            upstream_base_oid
        );
    }
}

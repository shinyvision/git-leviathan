use git2::BranchType;
use std::time::Duration;

use super::helpers::{
    command_dir, find_branch_or, index_has_conflicts, process_output_detail, wrap_git2_error,
};
use super::process::{run_git_command, spawn_git_command_with_timeout};
use super::GitService;
use crate::services::git_error::GitError;

const NETWORK_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushTarget {
    pub local_branch: String,
    pub remote_name: String,
    pub remote_branch: String,
}

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
    BehindRemote(PushTarget),
}

pub(super) fn push_current_branch(service: &GitService) -> Result<PushOutcome, GitError> {
    let local_branch = current_branch_shorthand(service)?;
    let tracking_remote = config_string(service, &format!("branch.{local_branch}.remote"));
    let remote_name = configured_push_remote(service, &local_branch, tracking_remote.as_deref())?;
    let merge_ref = config_string(service, &format!("branch.{local_branch}.merge"));
    let Some(upstream) = merge_ref
        .as_deref()
        .and_then(|name| name.strip_prefix("refs/heads/"))
    else {
        return Ok(PushOutcome::NeedsUpstream {
            branch_name: local_branch,
            remote_name,
        });
    };
    let remote_branch = if tracking_remote.as_deref() == Some(&remote_name) {
        upstream.to_owned()
    } else {
        local_branch.clone()
    };
    run_git_push(
        service,
        PushTarget {
            local_branch,
            remote_name,
            remote_branch,
        },
        PushMode::Normal,
    )
}

pub(super) fn push_and_set_upstream(
    service: &GitService,
    target: &PushTarget,
) -> Result<PushOutcome, GitError> {
    require_current_branch(service, &target.local_branch)?;
    run_git_push(service, target.clone(), PushMode::SetUpstream)
}

pub(super) fn force_push_current_branch(
    service: &GitService,
    target: &PushTarget,
) -> Result<PushOutcome, GitError> {
    require_current_branch(service, &target.local_branch)?;
    run_git_push(service, target.clone(), PushMode::Force)
}

fn require_current_branch(service: &GitService, expected: &str) -> Result<(), GitError> {
    if current_branch_shorthand(service)? != expected {
        return Err(GitError::Other(format!(
            "'{expected}' is no longer the checked-out branch"
        )));
    }
    Ok(())
}

pub(super) fn push_ref_to_remote(
    service: &GitService,
    remote_name: &str,
    ref_name: &str,
) -> Result<PushOutcome, GitError> {
    let local_branch = resolve_explicit_local_branch(service, ref_name)?;
    run_git_push(
        service,
        PushTarget {
            remote_branch: local_branch.clone(),
            local_branch,
            remote_name: remote_name.to_owned(),
        },
        PushMode::Normal,
    )
}

pub(super) fn current_branch_push_remote(service: &GitService) -> Result<String, GitError> {
    let branch = current_branch_shorthand(service)?;
    configured_push_remote(
        service,
        &branch,
        config_string(service, &format!("branch.{branch}.remote")).as_deref(),
    )
}

pub(super) fn pull_current_branch(
    service: &GitService,
    target: Option<&PushTarget>,
) -> Result<(), GitError> {
    if service.repo.state() != git2::RepositoryState::Clean || index_has_conflicts(&service.repo) {
        return Err(GitError::Other(
            "finish the current repository operation before pulling".into(),
        ));
    }
    // Prefer a fast-forward and create a merge only for divergent histories.
    let mut args = vec!["pull", "--no-rebase", "--ff", "--no-edit"];
    let ref_name;
    if let Some(target) = target {
        require_current_branch(service, &target.local_branch)?;
        if !has_upstream_config(service, target) {
            args.push("--set-upstream");
        }
        ref_name = format!("refs/heads/{}", target.remote_branch);
        args.extend(["--", &target.remote_name, &ref_name]);
    }
    let output = spawn_git_command_with_timeout(
        &command_dir(&service.repo)?,
        &args,
        "pull",
        NETWORK_TIMEOUT,
    )?;
    if output.status.success()
        || (service.repo.state() == git2::RepositoryState::Merge
            && index_has_conflicts(&service.repo))
    {
        Ok(())
    } else {
        Err(GitError::Other(format!(
            "pull failed: {}",
            process_output_detail(&output)
        )))
    }
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

fn has_upstream_config(service: &GitService, target: &PushTarget) -> bool {
    config_string(service, &format!("branch.{}.merge", target.local_branch)).is_some()
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

#[derive(Clone, Copy)]
enum PushMode {
    Normal,
    SetUpstream,
    Force,
}

fn run_git_push(
    service: &GitService,
    target: PushTarget,
    mode: PushMode,
) -> Result<PushOutcome, GitError> {
    // "." is Git's special name for an upstream in the current repository.
    if target.remote_name != "." {
        ensure_remote_exists(service, &target.remote_name)?;
    }
    if !git2::Branch::name_is_valid(&target.remote_branch).unwrap_or(false) {
        return Err(GitError::Other(format!(
            "invalid remote branch name '{}'",
            target.remote_branch
        )));
    }
    let refspec = format!(
        "refs/heads/{}:refs/heads/{}",
        target.local_branch, target.remote_branch
    );
    let mut args = vec!["push", "--porcelain"];
    match mode {
        PushMode::Normal => (),
        PushMode::Force => {
            args.push("--force");
            if !has_upstream_config(service, &target) {
                args.push("--set-upstream");
            }
        }
        PushMode::SetUpstream => args.push("--set-upstream"),
    }
    args.extend(["--", &target.remote_name, &refspec]);
    // Git translates even the rejection reason in --porcelain output.
    let output = run_git_command(
        &command_dir(&service.repo)?,
        &args,
        "push",
        Some(NETWORK_TIMEOUT),
        &[("LC_ALL", "C")],
    )?;
    if output.status.success() {
        return Ok(PushOutcome::Pushed);
    }
    let porcelain = String::from_utf8_lossy(&output.stdout);
    if !matches!(mode, PushMode::Force)
        && porcelain
            .lines()
            .filter_map(|line| line.strip_prefix("!\t"))
            .filter_map(|line| line.split_once('\t'))
            .any(|(_, reason)| {
                matches!(
                    reason,
                    "[rejected] (non-fast-forward)" | "[rejected] (fetch first)"
                )
            })
    {
        return Ok(PushOutcome::BehindRemote(target));
    }
    Err(GitError::Other(format!(
        "push failed: {}",
        process_output_detail(&output)
    )))
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
    fn custom_upstream_survives_missing_tracking_ref_and_matching_push_configuration() {
        let (temp, repo) = init_test_repo("push_named_upstream");
        ensure_main_checked_out(&repo);
        let (remote, bare) = init_bare_test_repo("push_named_remote");
        add_file_remote(&repo, "fork", remote.path_str());
        let base = repo.head().unwrap().target().unwrap();
        create_branch(&repo, "other");
        push_refspec(&repo, "fork", "refs/heads/other:refs/heads/other");
        let target = PushTarget {
            local_branch: "main".into(),
            remote_name: "fork".into(),
            remote_branch: "review/topic".into(),
        };
        let service = GitService::open(temp.path_str()).unwrap();
        push_and_set_upstream(&service, &target).unwrap();
        repo.find_reference("refs/remotes/fork/review/topic")
            .unwrap()
            .delete()
            .unwrap();
        repo.config()
            .unwrap()
            .set_str("push.default", "matching")
            .unwrap();
        checkout_branch_for_setup(&repo, "other");
        write_file(&temp.path, "other.txt", "do not push\n");
        commit_all(&repo, "other");
        assert!(force_push_current_branch(&service, &target).is_err());
        checkout_branch_for_setup(&repo, "main");
        write_file(&temp.path, "tracked.txt", "new\n");
        let latest = commit_all(&repo, "new");
        assert!(matches!(
            push_current_branch(&service).unwrap(),
            PushOutcome::Pushed
        ));
        assert_eq!(
            bare_branch_target(remote.path_str(), "review/topic"),
            latest
        );
        assert_eq!(bare_branch_target(remote.path_str(), "other"), base);
        assert!(bare.find_reference("refs/heads/main").is_err());
        // The remote has a commit which the local branch no longer contains.
        repo.reset(
            &repo.find_object(base, None).unwrap(),
            git2::ResetType::Hard,
            None,
        )
        .unwrap();
        assert!(
            matches!(push_current_branch(&service).unwrap(), PushOutcome::BehindRemote(rejected) if rejected == target)
        );
        force_push_current_branch(&service, &target).unwrap();
        assert_eq!(bare_branch_target(remote.path_str(), "review/topic"), base);
        assert_eq!(bare_branch_target(remote.path_str(), "other"), base);
    }

    #[test]
    fn pull_reports_its_own_conflict_and_rejects_an_already_unmerged_index() {
        let (temp, repo) = init_test_repo("pull_rejection_target");
        crate::services::test_support::configure_test_signature(&repo);
        ensure_main_checked_out(&repo);
        let (remote, _) = init_bare_test_repo("pull_rejection_remote");
        add_file_remote(&repo, "fork", remote.path_str());
        create_branch(&repo, "remote-side");
        checkout_branch_for_setup(&repo, "remote-side");
        write_file(&temp.path, "tracked.txt", "remote\n");
        commit_all(&repo, "remote");
        push_refspec(
            &repo,
            "fork",
            "refs/heads/remote-side:refs/heads/review/topic",
        );
        checkout_branch_for_setup(&repo, "main");
        write_file(&temp.path, "tracked.txt", "local\n");
        commit_all(&repo, "local");
        repo.config()
            .unwrap()
            .set_bool("pull.rebase", true)
            .unwrap();
        let service = GitService::open(temp.path_str()).unwrap();
        let target = PushTarget {
            local_branch: "main".into(),
            remote_name: "fork".into(),
            remote_branch: "review/topic".into(),
        };
        assert!(
            matches!(push_and_set_upstream(&service, &target).unwrap(), PushOutcome::BehindRemote(rejected) if rejected == target)
        );
        pull_current_branch(&service, Some(&target)).unwrap();
        assert_eq!(
            repo.config()
                .unwrap()
                .get_string("branch.main.merge")
                .unwrap(),
            "refs/heads/review/topic"
        );
        assert_eq!(service.repo.state(), git2::RepositoryState::Merge);
        assert!(index_has_conflicts(&service.repo));
        assert!(pull_current_branch(&service, Some(&target)).is_err());
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

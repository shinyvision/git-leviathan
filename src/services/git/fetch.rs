use std::time::Duration;

use super::helpers::{spawn_git_command_with_timeout, wrap_git2_error};
use super::GitService;
use crate::services::git_error::GitError;

const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn fetch_all_refs(service: &GitService) -> Result<(), GitError> {
    let remotes: Vec<String> = service
        .repo
        .remotes()
        .map_err(|e| wrap_git2_error("list remotes", e))?
        .iter()
        .flatten()
        .map(|s| s.to_string())
        .collect();

    if remotes.is_empty() {
        return Ok(());
    }

    let mut first_error = None;
    for remote_name in remotes {
        if let Err(err) = fetch_remote_refs(service, &remote_name) {
            eprintln!("git_leviathan: fetch error for '{remote_name}': {err}");
            if first_error.is_none() {
                first_error = Some(err);
            }
        }
    }

    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

pub(super) fn fetch_remote_refs(service: &GitService, remote_name: &str) -> Result<(), GitError> {
    if remote_name.is_empty() {
        return Err(GitError::Other("remote name cannot be empty".to_string()));
    }

    {
        let _remote = service
            .repo
            .find_remote(remote_name)
            .map_err(|e| wrap_git2_error(&format!("find remote '{remote_name}'"), e))?;
    }

    fetch_configured_remote(service, remote_name)
}

fn fetch_configured_remote(service: &GitService, remote_name: &str) -> Result<(), GitError> {
    let repo_dir = service
        .repo
        .workdir()
        .unwrap_or_else(|| service.repo.path())
        .to_string_lossy()
        .into_owned();

    let op = format!("fetch '{remote_name}'");
    let output =
        spawn_git_command_with_timeout(&repo_dir, &["fetch", remote_name], &op, FETCH_TIMEOUT)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !stderr.is_empty() {
            eprintln!("git_leviathan: fetch warning for '{remote_name}': {stderr}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::services::{
        test_support::{
            add_remote, create_branch, init_bare_test_repo, init_test_repo, push_refspec,
        },
        GitService,
    };
    use git2::{BranchType, Repository};

    fn has_remote_branch(repo: &Repository, branch_name: &str) -> bool {
        repo.find_branch(branch_name, BranchType::Remote).is_ok()
    }

    fn remove_remote_branch(repo: &Repository, branch_name: &str) {
        let full_ref = format!("refs/remotes/{branch_name}");
        if let Ok(mut reference) = repo.find_reference(&full_ref) {
            reference
                .delete()
                .expect("failed to remove setup remote-tracking ref");
        }
    }

    fn add_file_remote(repo: &Repository, name: &str, remote_path: &str) {
        add_remote(repo, name, &format!("file://{remote_path}"));
    }

    #[test]
    fn fetch_all_refs_fetches_every_configured_remote() {
        let (temp_repo, repo) = init_test_repo("fetch_all_multi_remote");
        let (origin_temp, _origin_repo) = init_bare_test_repo("fetch_all_multi_origin");
        let (backup_temp, _backup_repo) = init_bare_test_repo("fetch_all_multi_backup");

        add_file_remote(&repo, "origin", origin_temp.path_str());
        add_file_remote(&repo, "backup", backup_temp.path_str());

        create_branch(&repo, "origin-only");
        push_refspec(
            &repo,
            "origin",
            "refs/heads/origin-only:refs/heads/origin-only",
        );
        create_branch(&repo, "backup-only");
        push_refspec(
            &repo,
            "backup",
            "refs/heads/backup-only:refs/heads/backup-only",
        );
        remove_remote_branch(&repo, "origin/origin-only");
        remove_remote_branch(&repo, "backup/backup-only");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .fetch_all_refs()
            .expect("fetch all remotes should succeed");

        let repo = Repository::open(temp_repo.path_str()).expect("failed to reopen repo");
        assert!(has_remote_branch(&repo, "origin/origin-only"));
        assert!(has_remote_branch(&repo, "backup/backup-only"));
    }

    #[test]
    fn fetch_remote_refs_fetches_only_requested_remote() {
        let (temp_repo, repo) = init_test_repo("fetch_one_multi_remote");
        let (origin_temp, _origin_repo) = init_bare_test_repo("fetch_one_origin");
        let (backup_temp, _backup_repo) = init_bare_test_repo("fetch_one_backup");

        add_file_remote(&repo, "origin", origin_temp.path_str());
        add_file_remote(&repo, "backup", backup_temp.path_str());

        create_branch(&repo, "origin-only");
        push_refspec(
            &repo,
            "origin",
            "refs/heads/origin-only:refs/heads/origin-only",
        );
        create_branch(&repo, "backup-only");
        push_refspec(
            &repo,
            "backup",
            "refs/heads/backup-only:refs/heads/backup-only",
        );
        remove_remote_branch(&repo, "origin/origin-only");
        remove_remote_branch(&repo, "backup/backup-only");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .fetch_remote_refs("backup")
            .expect("fetch backup should succeed");

        let repo = Repository::open(temp_repo.path_str()).expect("failed to reopen repo");
        assert!(!has_remote_branch(&repo, "origin/origin-only"));
        assert!(has_remote_branch(&repo, "backup/backup-only"));
    }

    #[test]
    fn fetch_all_refs_continues_after_non_fatal_remote_warning() {
        let (temp_repo, repo) = init_test_repo("fetch_all_warning_continues");
        let (good_temp, _good_repo) = init_bare_test_repo("fetch_all_warning_good");

        add_file_remote(
            &repo,
            "aaa_bad",
            &format!("{}/missing", temp_repo.path_str()),
        );
        add_file_remote(&repo, "zzz_good", good_temp.path_str());

        create_branch(&repo, "good-only");
        push_refspec(
            &repo,
            "zzz_good",
            "refs/heads/good-only:refs/heads/good-only",
        );
        remove_remote_branch(&repo, "zzz_good/good-only");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .fetch_all_refs()
            .expect("fetch all should keep going");

        let repo = Repository::open(temp_repo.path_str()).expect("failed to reopen repo");
        assert!(has_remote_branch(&repo, "zzz_good/good-only"));
    }
}

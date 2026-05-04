use std::time::Duration;

use super::helpers::{spawn_git_command_with_timeout, wrap_git2_error};
use super::GitService;
use crate::services::git_error::GitError;

const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn fetch_all_refs(service: &GitService) -> Result<(), GitError> {
    let repo_dir = service
        .repo
        .workdir()
        .unwrap_or_else(|| service.repo.path())
        .to_string_lossy()
        .into_owned();

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

    let remote_name = remotes
        .iter()
        .find(|r| r.as_str() == "origin")
        .cloned()
        .unwrap_or_else(|| remotes[0].clone());

    let output = spawn_git_command_with_timeout(
        &repo_dir,
        &["fetch", &remote_name],
        "fetch",
        FETCH_TIMEOUT,
    )?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !stderr.is_empty() {
            eprintln!("git_leviathan: fetch warning: {stderr}");
        }
    }

    Ok(())
}

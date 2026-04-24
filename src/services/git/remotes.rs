use super::helpers::wrap_git2_error;
use super::GitService;
use crate::services::git_error::GitError;

pub(super) fn add_remote(
    service: &mut GitService,
    name: &str,
    pull_url: &str,
    push_url: &str,
) -> Result<(), GitError> {
    let name = name.trim();
    let pull_url = pull_url.trim();
    let push_url = push_url.trim();

    if name.is_empty() {
        return Err(GitError::Other("remote name cannot be empty".to_string()));
    }
    if pull_url.is_empty() {
        return Err(GitError::Other("pull URL cannot be empty".to_string()));
    }

    if service.repo.find_remote(name).is_ok() {
        return Err(GitError::Other(format!(
            "a remote named '{}' already exists",
            name
        )));
    }

    service
        .repo
        .remote(name, pull_url)
        .map_err(|e| wrap_git2_error(&format!("add remote '{name}'"), e))?;

    if !push_url.is_empty() && push_url != pull_url {
        service
            .repo
            .remote_set_pushurl(name, Some(push_url))
            .map_err(|e| wrap_git2_error("set push URL", e))?;
    }

    Ok(())
}

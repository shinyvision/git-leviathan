use super::helpers::{spawn_git_command, wrap_git2_error};
use super::GitService;
use crate::services::git_error::GitError;

pub(super) fn create_tag(
    service: &GitService,
    tag_name: &str,
    target_hash: &str,
) -> Result<(), GitError> {
    let tag_name = tag_name.trim();
    if tag_name.is_empty() {
        return Err(GitError::Other("tag name is empty".to_string()));
    }
    let oid = git2::Oid::from_str(target_hash)
        .map_err(|e| GitError::Other(format!("invalid commit hash: {e}")))?;
    let obj = service
        .repo
        .find_object(oid, None)
        .map_err(|e| wrap_git2_error(&format!("find commit object {oid}"), e))?;
    service
        .repo
        .tag_lightweight(tag_name, &obj, false)
        .map(|_| ())
        .map_err(|e| wrap_git2_error(&format!("create tag '{tag_name}'"), e))
}

pub(super) fn delete_tag(service: &GitService, tag_name: &str) -> Result<(), GitError> {
    service
        .repo
        .tag_delete(tag_name)
        .map_err(|e| wrap_git2_error(&format!("delete tag '{tag_name}'"), e))
}

fn repo_dir_str(service: &GitService) -> String {
    service
        .repo
        .workdir()
        .unwrap_or_else(|| service.repo.path())
        .to_string_lossy()
        .into_owned()
}

pub(super) fn push_tag(
    service: &GitService,
    remote_name: &str,
    tag_name: &str,
) -> Result<(), GitError> {
    let repo_dir = repo_dir_str(service);
    let refspec = format!("refs/tags/{0}:refs/tags/{0}", tag_name);
    let output = spawn_git_command(&repo_dir, &["push", remote_name, &refspec], "push tag")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(GitError::Other(if stderr.is_empty() {
            "git push tag failed".to_string()
        } else {
            stderr
        }));
    }
    Ok(())
}

pub(super) fn delete_remote_tag(
    service: &GitService,
    remote_name: &str,
    tag_name: &str,
) -> Result<(), GitError> {
    let repo_dir = repo_dir_str(service);
    let refspec = format!(":refs/tags/{}", tag_name);
    let output = spawn_git_command(
        &repo_dir,
        &["push", remote_name, &refspec],
        "push delete tag",
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(GitError::Other(if stderr.is_empty() {
            "git delete remote tag failed".to_string()
        } else {
            stderr
        }));
    }
    Ok(())
}

/// Run `git ls-remote --tags <remote>` and return the set of tag short-names
/// present on that remote. Network call.
pub(super) fn list_remote_tags(
    service: &GitService,
    remote_name: &str,
) -> Result<Vec<String>, GitError> {
    let repo_dir = repo_dir_str(service);
    let output = spawn_git_command(
        &repo_dir,
        &["ls-remote", "--tags", remote_name],
        "ls-remote --tags",
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(GitError::Other(if stderr.is_empty() {
            "git ls-remote failed".to_string()
        } else {
            stderr
        }));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut names = Vec::new();
    for line in stdout.lines() {
        // <hash>\trefs/tags/<name>[^{}]
        let Some((_hash, refname)) = line.split_once('\t') else {
            continue;
        };
        let name = refname.trim_start_matches("refs/tags/");
        let name = name.trim_end_matches("^{}");
        if name.is_empty() {
            continue;
        }
        names.push(name.to_string());
    }
    names.sort();
    names.dedup();
    Ok(names)
}

//! Centralised `find_branch` / `find_commit` / `find_reference` and git-CLI
//! subprocess invocation for the service layer. Uniform error classification
//! via `wrap_git2_error`; callers should not re-roll their own boilerplate.

use std::process::{Command, Output};

use git2::{Branch, BranchType, Commit, ErrorClass, ErrorCode, Oid, Reference, Repository};

use crate::services::git_error::GitError;

/// Classify a `git2::Error` into the narrowest `GitError` variant from its
/// class+code. `op` is a short imperative label (e.g. "rename branch") that
/// flows into "<op> failed: <reason>" messages.
pub(super) fn wrap_git2_error(op: &str, err: git2::Error) -> GitError {
    let reason = err.message().to_string();
    match (err.class(), err.code()) {
        (ErrorClass::Net, _) => GitError::NetworkFailure {
            op: op.to_string(),
            reason,
        },
        (ErrorClass::Http, ErrorCode::Auth) | (ErrorClass::Ssh, ErrorCode::Auth) => {
            GitError::AuthFailed {
                op: op.to_string(),
                reason,
            }
        }
        (ErrorClass::Tree, _) => GitError::TreeAccessFailed {
            op: op.to_string(),
            reason,
        },
        (ErrorClass::Odb, _) | (_, ErrorCode::Invalid) => GitError::CorruptObject {
            op: op.to_string(),
            reason,
        },
        _ => GitError::Other(format!("{op} failed: {reason}")),
    }
}

/// Maps git2's "not-found" to a structured variant; other classes pass
/// through `wrap_git2_error`.
pub(super) fn find_branch_or<'repo>(
    repo: &'repo Repository,
    name: &str,
    branch_type: BranchType,
) -> Result<Branch<'repo>, GitError> {
    match repo.find_branch(name, branch_type) {
        Ok(branch) => Ok(branch),
        Err(e) if e.code() == ErrorCode::NotFound => match branch_type {
            BranchType::Local => Err(GitError::BranchNotFound(name.to_string())),
            BranchType::Remote => Err(GitError::RemoteBranchNotFound(name.to_string())),
        },
        Err(e) => Err(wrap_git2_error(&format!("find branch '{name}'"), e)),
    }
}

pub(super) fn find_commit_or(repo: &Repository, oid: Oid) -> Result<Commit<'_>, GitError> {
    match repo.find_commit(oid) {
        Ok(commit) => Ok(commit),
        Err(e) if e.code() == ErrorCode::NotFound => Err(GitError::CommitNotFound(oid.to_string())),
        Err(e) => Err(wrap_git2_error(&format!("find commit {oid}"), e)),
    }
}

/// Missing refs come back as `ReferenceBroken` ("ref that should be there
/// but isn't").
pub(super) fn find_reference_or<'repo>(
    repo: &'repo Repository,
    full_ref: &str,
) -> Result<Reference<'repo>, GitError> {
    match repo.find_reference(full_ref) {
        Ok(reference) => Ok(reference),
        Err(e) if e.code() == ErrorCode::NotFound => {
            Err(GitError::ReferenceBroken(full_ref.to_string()))
        }
        Err(e) => Err(wrap_git2_error(&format!("find reference '{full_ref}'"), e)),
    }
}

/// Spawn `git <args>` inside `repo_path` and return the completed `Output`
/// regardless of exit status; the caller interprets status/stdout/stderr.
/// Returns `GitError::Other` on spawn failure (fork/exec errors).
pub(super) fn spawn_git_command(
    repo_path: &str,
    args: &[&str],
    op: &str,
) -> Result<Output, GitError> {
    Command::new("git")
        .current_dir(repo_path)
        .args(args)
        .output()
        .map_err(|e| GitError::Other(format!("{op}: failed to spawn git: {e}")))
}

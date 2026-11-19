//! Repository queries and Git error classification.

use crate::services::git_error::GitError;
use git2::{Branch, BranchType, Commit, ErrorClass, ErrorCode, Oid, Reference, Repository};
use std::process::Output;

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
        (ErrorClass::Odb, _) => GitError::CorruptObject {
            op: op.to_string(),
            reason,
        },
        _ => GitError::Other(format!("{op} failed: {reason}")),
    }
}

/// Keep CLI commands inside the opened repository, including for bare remotes.
pub(super) fn command_dir(repo: &Repository) -> Result<String, GitError> {
    let dir = repo.workdir().unwrap_or_else(|| repo.path());
    dir.to_str()
        .map(str::to_owned)
        .ok_or_else(|| GitError::Other(format!("repository path is not UTF-8: {}", dir.display())))
}

/// Whether the repository index currently records merge conflicts. Errors
/// reading the index are treated as "no conflicts".
pub(super) fn index_has_conflicts(repo: &Repository) -> bool {
    let Ok(mut index) = repo.index() else {
        return false;
    };
    let _ = index.read(true);
    index.has_conflicts()
}

/// Whether a git subprocess's stdout/stderr mention a merge conflict.
pub(super) fn output_mentions_conflict(output: &Output) -> bool {
    String::from_utf8_lossy(&output.stdout).contains("CONFLICT")
        || String::from_utf8_lossy(&output.stderr).contains("CONFLICT")
}

/// Best-effort failure detail from a git subprocess: trimmed stderr, else
/// trimmed stdout, else the exit status.
pub(super) fn process_output_detail(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }
    format!("git exited with status {}", output.status)
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

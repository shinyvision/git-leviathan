//! Centralised `find_branch` / `find_commit` / `find_reference` and git-CLI
//! subprocess invocation for the service layer. Uniform error classification
//! via `wrap_git2_error`; callers should not re-roll their own boilerplate.

use std::collections::HashSet;
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use git2::{Branch, BranchType, Commit, ErrorClass, ErrorCode, Oid, Reference, Repository};

use crate::services::git_error::GitError;

/// PIDs of `git` subprocesses currently being awaited by `spawn_git_command`.
/// On app shutdown the close handler calls `kill_running_git_processes` to
/// SIGKILL anything still running so the process can exit immediately instead
/// of blocking on a slow `git ls-remote` / `git push`.
static RUNNING_GIT_PIDS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();

fn pid_set() -> &'static Mutex<HashSet<u32>> {
    RUNNING_GIT_PIDS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn register_pid(pid: u32) {
    if let Ok(mut set) = pid_set().lock() {
        set.insert(pid);
    }
}

fn unregister_pid(pid: u32) {
    if let Ok(mut set) = pid_set().lock() {
        set.remove(&pid);
    }
}

/// Force-kill every git subprocess registered by `spawn_git_command`. Intended
/// to be called once from the shutdown path; safe to call when none are
/// running. Does not wait for the children to be reaped.
pub fn kill_running_git_processes() {
    let pids: Vec<u32> = match pid_set().lock() {
        Ok(set) => set.iter().copied().collect(),
        Err(_) => return,
    };
    for pid in pids {
        kill_pid(pid);
    }
}

#[cfg(unix)]
fn kill_pid(pid: u32) {
    let _ = Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(windows)]
fn kill_pid(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(any(unix, windows)))]
fn kill_pid(_pid: u32) {}

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
    spawn_git_command_inner(repo_path, args, op, None)
}

pub(super) fn spawn_git_command_with_timeout(
    repo_path: &str,
    args: &[&str],
    op: &str,
    timeout: Duration,
) -> Result<Output, GitError> {
    spawn_git_command_inner(repo_path, args, op, Some(timeout))
}

fn spawn_git_command_inner(
    repo_path: &str,
    args: &[&str],
    op: &str,
    timeout: Option<Duration>,
) -> Result<Output, GitError> {
    let child = Command::new("git")
        .current_dir(repo_path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| GitError::Other(format!("{op}: failed to spawn git: {e}")))?;
    let pid = child.id();
    register_pid(pid);
    let result = match timeout {
        Some(timeout) => wait_with_timeout(child, op, timeout),
        None => child
            .wait_with_output()
            .map_err(|e| GitError::Other(format!("{op}: failed to wait on git: {e}"))),
    };
    unregister_pid(pid);
    result
}

fn wait_with_timeout(
    mut child: std::process::Child,
    op: &str,
    timeout: Duration,
) -> Result<Output, GitError> {
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| GitError::Other(format!("{op}: failed to capture stdout")))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| GitError::Other(format!("{op}: failed to capture stderr")))?;
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(GitError::NetworkFailure {
                    op: op.to_string(),
                    reason: format!("timed out after {}s", timeout.as_secs()),
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(GitError::Other(format!("{op}: failed to wait on git: {e}")));
            }
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

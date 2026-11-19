//! Subprocess execution and cancellation for Git and its helpers.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::services::git_error::GitError;
use crate::utils::configure_background_command;

/// PIDs of `git` subprocesses currently being awaited by `spawn_git_command`.
/// On app shutdown the close handler calls `kill_running_git_processes` to
/// SIGKILL anything still running so the process can exit immediately instead
/// of blocking on a slow `git ls-remote` / `git push`.
static RUNNING_GIT_PIDS: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();

fn pid_set() -> &'static Mutex<HashMap<u32, String>> {
    RUNNING_GIT_PIDS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_pid(pid: u32, op: &str) {
    if let Ok(mut set) = pid_set().lock() {
        set.insert(pid, op.to_string());
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
    kill_running_git_processes_matching(|_| true);
}

/// Bumped by `kill_running_fetch_processes`. `fetch_all_refs` snapshots it on
/// entry and stops between remotes once it changes — killing only the
/// currently running subprocess would let the per-remote loop spawn the next
/// fetch (and sit out its full timeout) after the fetch was cancelled.
static FETCH_CANCEL_GENERATION: AtomicU64 = AtomicU64::new(0);

pub(super) fn fetch_cancel_generation() -> u64 {
    FETCH_CANCEL_GENERATION.load(Ordering::SeqCst)
}

/// Force-kill network fetch subprocesses owned by the auto-fetch pipeline.
/// This covers both `git fetch` and the follow-up `git ls-remote --tags`
/// cache refresh, without interrupting unrelated local git subprocesses.
pub fn kill_running_fetch_processes() {
    FETCH_CANCEL_GENERATION.fetch_add(1, Ordering::SeqCst);
    kill_running_git_processes_matching(|op| op.starts_with("fetch '") || op == "ls-remote --tags");
}

fn kill_running_git_processes_matching(predicate: impl Fn(&str) -> bool) {
    if let Ok(set) = pid_set().lock() {
        for (&pid, op) in set.iter() {
            if predicate(op) {
                kill_process_tree(pid);
            }
        }
    }
}

#[cfg(unix)]
fn kill_process_tree(pid: u32) {
    use rustix::process::{kill_process_group, Pid, Signal};
    if let Some(group) = Pid::from_raw(pid as i32) {
        let _ = kill_process_group(group, Signal::KILL);
    }
}

#[cfg(windows)]
fn kill_process_tree(pid: u32) {
    let mut command = Command::new("taskkill");
    let _ = configure_background_command(&mut command)
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(any(unix, windows)))]
fn kill_process_tree(_pid: u32) {}

/// Spawn `git <args>` inside `repo_path` and return the completed `Output`
/// regardless of exit status; the caller interprets status/stdout/stderr.
/// Returns `GitError::Other` on spawn failure (fork/exec errors).
pub(super) fn spawn_git_command(
    repo_path: &str,
    args: &[&str],
    op: &str,
) -> Result<Output, GitError> {
    run_git_command(repo_path, args, op, None, &[])
}

pub(super) fn spawn_git_command_with_timeout(
    repo_path: &str,
    args: &[&str],
    op: &str,
    timeout: Duration,
) -> Result<Output, GitError> {
    run_git_command(repo_path, args, op, Some(timeout), &[])
}

/// Like `spawn_git_command` but with extra environment variables applied to the
/// child (e.g. `GIT_EDITOR`). Still registers the PID so the child is killable
/// on shutdown.
pub(super) fn spawn_git_command_with_env(
    repo_path: &str,
    args: &[&str],
    op: &str,
    envs: &[(&str, &str)],
) -> Result<Output, GitError> {
    run_git_command(repo_path, args, op, None, envs)
}

pub(super) fn run_git_command(
    repo_path: &str,
    args: &[&str],
    op: &str,
    timeout: Option<Duration>,
    envs: &[(&str, &str)],
) -> Result<Output, GitError> {
    let mut command = Command::new("git");
    command
        .current_dir(repo_path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");
    for (key, value) in envs {
        command.env(key, value);
    }
    capture_program(command, op, timeout)
}

fn configure_git_process(command: &mut Command) -> &mut Command {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    configure_background_command(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
}

pub(in crate::services) fn capture_program(
    mut command: Command,
    op: &str,
    timeout: Option<Duration>,
) -> Result<Output, GitError> {
    let child = configure_git_process(&mut command)
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| GitError::Other(format!("{op}: failed to start: {e}")))?;
    wait_registered(child, op, timeout)
}

/// Run a non-git program that belongs to a git operation, such as a git hook,
/// with `stdin` as its input. Like `spawn_git_command` it registers the PID,
/// so shutdown can kill a program that hangs.
pub(super) fn spawn_program(
    mut command: Command,
    stdin: &[u8],
    op: &str,
) -> Result<Output, GitError> {
    let mut child = configure_git_process(&mut command)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| GitError::Other(format!("{op}: failed to start: {e}")))?;
    if let Some(mut pipe) = child.stdin.take() {
        // Write from a thread: a program that does not read its input must not
        // block us while it fills its output pipes.
        let input = stdin.to_vec();
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = pipe.write_all(&input);
        });
    }
    wait_registered(child, op, None)
}

/// Wait for `child` while its PID is registered for `kill_running_git_processes`.
fn wait_registered(
    child: std::process::Child,
    op: &str,
    timeout: Option<Duration>,
) -> Result<Output, GitError> {
    let pid = child.id();
    register_pid(pid, op);
    let result = match timeout {
        Some(timeout) => wait_with_timeout(child, op, timeout),
        None => child
            .wait_with_output()
            .map_err(|e| GitError::Other(format!("{op}: failed to wait: {e}"))),
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
        // A helper can inherit a pipe and outlive Git. Keep the child
        // unreaped until the output drains so its group ID remains ours.
        if stdout_reader.is_finished() && stderr_reader.is_finished() {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => (),
                Err(error) => {
                    terminate_child(&mut child);
                    return Err(GitError::Other(format!(
                        "{op}: failed to wait on git: {error}"
                    )));
                }
            }
        }
        if start.elapsed() >= timeout {
            terminate_child(&mut child);
            return Err(GitError::NetworkFailure {
                op: op.to_owned(),
                reason: format!("timed out after {timeout:?}"),
            });
        }
        std::thread::sleep(Duration::from_millis(50).min(timeout.saturating_sub(start.elapsed())));
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn terminate_child(child: &mut std::process::Child) {
    kill_process_tree(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn timeout_includes_a_helper_holding_the_output_pipe_after_parent_exit() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 5 &"]);
        let start = Instant::now();
        let result = capture_program(command, "test helper", Some(Duration::from_millis(150)));
        assert!(matches!(result, Err(GitError::NetworkFailure { .. })));
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}

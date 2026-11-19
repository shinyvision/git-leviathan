//! Runs git's client-side hooks after operations that go through libgit2.
//!
//! libgit2 never runs hooks, so tools that rely on them (hardy, husky, lefthook
//! and the like) would silently miss every checkout, fast-forward or rewrite
//! made in the app. Operations that shell out to the `git` binary (merge,
//! rebase, revert, cherry-pick, stash) get their hooks from git itself and must
//! not call this module.
//!
//! Like git, a failing post-hook never undoes or fails the operation. What a
//! hook prints, or the fact that it failed, is published as a `HookReport` for
//! the app to show.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex};

use git2::{Oid, Repository};

use super::process::spawn_program;

/// A hook that runs after an operation, with what git would pass to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PostHook {
    /// HEAD moved: a branch was checked out, or a worktree was created.
    Checkout {
        previous: Oid,
        current: Oid,
        /// False for checking out files only; git passes 0 then.
        branch_checkout: bool,
    },
    /// A merge finished, including a fast-forward of the current branch.
    Merge { squash: bool },
    /// Commits were replaced. Each pair is (old commit, new commit).
    Rewrite {
        command: RewriteCommand,
        rewritten: Vec<(Oid, Oid)>,
    },
}

/// What rewrote the commits, as git names it to `post-rewrite`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RewriteCommand {
    Amend,
    Rebase,
}

impl PostHook {
    fn name(&self) -> &'static str {
        match self {
            PostHook::Checkout { .. } => "post-checkout",
            PostHook::Merge { .. } => "post-merge",
            PostHook::Rewrite { .. } => "post-rewrite",
        }
    }

    fn args(&self) -> Vec<String> {
        match self {
            PostHook::Checkout {
                previous,
                current,
                branch_checkout,
            } => vec![
                previous.to_string(),
                current.to_string(),
                if *branch_checkout { "1" } else { "0" }.to_string(),
            ],
            PostHook::Merge { squash } => vec![if *squash { "1" } else { "0" }.to_string()],
            PostHook::Rewrite { command, .. } => vec![match command {
                RewriteCommand::Amend => "amend",
                RewriteCommand::Rebase => "rebase",
            }
            .to_string()],
        }
    }

    fn stdin(&self) -> Vec<u8> {
        match self {
            PostHook::Rewrite { rewritten, .. } => rewritten
                .iter()
                .map(|(old, new)| format!("{old} {new}\n"))
                .collect::<String>()
                .into_bytes(),
            _ => Vec::new(),
        }
    }
}

/// What a hook did, for the user to see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookReport {
    pub hook: &'static str,
    pub succeeded: bool,
    /// The hook's stdout and stderr, without terminal colour codes.
    pub output: String,
}

/// Run `hook` in `repo` if the repository has it, and publish what it printed.
pub(super) fn run(repo: &Repository, hook: PostHook) {
    if let Some(report) = execute(repo, &hook) {
        if !report.succeeded || !report.output.is_empty() {
            publish(report);
        }
    }
}

/// The commit HEAD points at, or the null id for an unborn branch, as git
/// passes it to hooks.
pub(super) fn head_commit(repo: &Repository) -> Oid {
    repo.head()
        .ok()
        .and_then(|head| head.target())
        .unwrap_or_else(Oid::zero)
}

/// Run `hook` and return what happened, or `None` when the repository does
/// not have that hook.
fn execute(repo: &Repository, hook: &PostHook) -> Option<HookReport> {
    let path = hooks_dir(repo).join(hook.name());
    if !is_executable(&path) {
        return None;
    }
    let mut command = hook_command(&path);
    command
        .args(hook.args())
        .current_dir(repo.workdir().unwrap_or_else(|| repo.path()));
    let op = format!("{} hook", hook.name());
    let report = match spawn_program(command, &hook.stdin(), &op) {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            HookReport {
                hook: hook.name(),
                succeeded: output.status.success(),
                output: strip_ansi(text.trim()),
            }
        }
        Err(e) => HookReport {
            hook: hook.name(),
            succeeded: false,
            output: e.to_string(),
        },
    };
    Some(report)
}

/// The folder git runs hooks from: `core.hooksPath` when set (relative to the
/// worktree, as git reads it), else `hooks` in the common git directory, which
/// every worktree of the repository shares.
fn hooks_dir(repo: &Repository) -> PathBuf {
    let configured = repo
        .config()
        .ok()
        .and_then(|config| config.get_path("core.hooksPath").ok());
    match configured {
        Some(path) if path.is_absolute() => path,
        Some(path) => repo.workdir().unwrap_or_else(|| repo.path()).join(path),
        None => common_dir(repo).join("hooks"),
    }
}

/// The git directory shared by all worktrees. A linked worktree's git
/// directory names it in its `commondir` file.
fn common_dir(repo: &Repository) -> PathBuf {
    let git_dir = repo.path();
    match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(common) => git_dir.join(common.trim()),
        Err(_) => git_dir.to_path_buf(),
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(unix)]
fn hook_command(path: &Path) -> Command {
    Command::new(path)
}

/// Git for Windows runs hooks through its own sh, which reads the shebang.
#[cfg(not(unix))]
fn hook_command(path: &Path) -> Command {
    let mut command = Command::new("sh");
    command.arg(path);
    command
}

/// Removes terminal escape sequences, which a toast would show as garbage.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            // A CSI sequence ends with a letter.
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

struct ReportBus {
    tx: tokio::sync::mpsc::UnboundedSender<HookReport>,
    rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<HookReport>>>,
}

static REPORT_BUS: LazyLock<ReportBus> = LazyLock::new(|| {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    ReportBus {
        tx,
        rx: Mutex::new(Some(rx)),
    }
});

fn publish(report: HookReport) {
    let _ = REPORT_BUS.tx.send(report);
}

/// Hand out the receiver of hook reports. Only the first call gets it.
pub fn take_hook_report_receiver() -> Option<tokio::sync::mpsc::UnboundedReceiver<HookReport>> {
    REPORT_BUS.rx.lock().ok().and_then(|mut guard| guard.take())
}

#[cfg(all(test, unix))]
mod tests;

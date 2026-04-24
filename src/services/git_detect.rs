use crate::services::git_error::GitError;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
#[allow(dead_code)] // fields/variants used cross-platform or only in Debug output
pub enum GitStatus {
    Available {
        path: PathBuf,
        version: String,
    },
    NotFound,
    /// macOS-only: `/usr/bin/git` is a stub that triggers the Command Line Tools
    /// installer GUI when executed without CLT present. Detect and report
    /// separately so we can guide the user to `xcode-select --install`.
    MacOsCommandLineToolsMissing,
}

impl GitStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, GitStatus::Available { .. })
    }
}

pub fn detect() -> GitStatus {
    let Some(path) = find_git_in_path() else {
        return GitStatus::NotFound;
    };

    #[cfg(target_os = "macos")]
    {
        if is_macos_clt_stub(&path) && !xcode_clt_installed() {
            return GitStatus::MacOsCommandLineToolsMissing;
        }
    }

    match run_git_version(&path) {
        Some(version) => GitStatus::Available { path, version },
        None => GitStatus::NotFound,
    }
}

fn find_git_in_path() -> Option<PathBuf> {
    let exe_names: &[&str] = if cfg!(windows) {
        &["git.exe", "git.cmd", "git.bat"]
    } else {
        &["git"]
    };

    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for name in exe_names {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && (meta.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn is_macos_clt_stub(path: &Path) -> bool {
    path == Path::new("/usr/bin/git")
}

#[cfg(target_os = "macos")]
fn xcode_clt_installed() -> bool {
    // `xcode-select -p` exits 0 if CLT (or Xcode) is installed. It does NOT
    // trigger the installer popup — only running `/usr/bin/git` itself does.
    match Command::new("/usr/bin/xcode-select").arg("-p").output() {
        Ok(out) => out.status.success() && !out.stdout.is_empty(),
        Err(_) => false,
    }
}

fn run_git_version(path: &Path) -> Option<String> {
    let mut cmd = Command::new(path);
    cmd.arg("--version");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW — prevents console flash when running from a GUI.
        cmd.creation_flags(0x0800_0000);
    }

    let child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    let output = wait_with_timeout(child, Duration::from_secs(3))?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        None
    } else {
        Some(stdout)
    }
}

/// Given any workdir (primary repo dir OR a secondary worktree dir), return
/// `(primary_workdir, active_workdir)`.
///
/// - `primary_workdir` is the original repository's workdir (where `.git` is a
///   directory).
/// - `active_workdir` is the caller-supplied path, canonicalized.
///
/// Both paths are returned canonicalized so hashmap / equality compares line
/// up regardless of how the user typed the path.
pub fn resolve_primary_and_active(
    path: &Path,
) -> Result<(PathBuf, PathBuf), GitError> {
    let canonical_active = path
        .canonicalize()
        .map_err(|e| GitError::Other(format!("canonicalize '{}': {e}", path.display())))?;

    let repo = git2::Repository::open(&canonical_active)
        .map_err(|e| GitError::Other(format!("open repo at '{}': {}", canonical_active.display(), e.message())))?;

    let canonical_primary = if repo.is_worktree() {
        // repo.path() for a secondary worktree is <primary>/.git/worktrees/<name>/.
        // Walk back: strip the worktree-name component, assert the parent is literally
        // named "worktrees", then strip that and assert its parent is literally ".git".
        let git_wt_dir = repo.path();
        let worktrees_dir = git_wt_dir.parent().ok_or_else(|| GitError::Other(format!(
            "worktree git dir '{}' has no parent",
            git_wt_dir.display(),
        )))?;
        if worktrees_dir.file_name().map(|n| n != "worktrees").unwrap_or(true) {
            return Err(GitError::Other(format!(
                "unexpected worktree layout: expected 'worktrees' component at '{}'",
                worktrees_dir.display(),
            )));
        }
        let git_dir = worktrees_dir.parent().ok_or_else(|| GitError::Other(format!(
            "'{}' has no parent",
            worktrees_dir.display(),
        )))?;
        if git_dir.file_name().map(|n| n != ".git").unwrap_or(true) {
            return Err(GitError::Other(format!(
                "unexpected worktree layout: expected '.git' component at '{}'",
                git_dir.display(),
            )));
        }
        let primary_workdir = git_dir.parent().ok_or_else(|| GitError::Other(format!(
            "primary '.git' dir '{}' has no parent workdir",
            git_dir.display(),
        )))?;
        primary_workdir
            .canonicalize()
            .map_err(|e| GitError::Other(format!("canonicalize primary '{}': {e}", primary_workdir.display())))?
    } else {
        // Primary repo: workdir() is the workdir itself.
        let workdir = repo
            .workdir()
            .ok_or_else(|| GitError::Other("bare repository has no workdir".to_string()))?
            .to_path_buf();
        workdir
            .canonicalize()
            .map_err(|e| GitError::Other(format!("canonicalize primary '{}': {e}", workdir.display())))?
    };

    Ok((canonical_primary, canonical_active))
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Option<std::process::Output> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::init_test_repo;

    struct CleanupDir(std::path::PathBuf);
    impl Drop for CleanupDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn resolve_primary_and_active_from_primary_returns_same_for_both() {
        let (temp, _repo) = init_test_repo("resolve_primary");
        let (primary, active) = resolve_primary_and_active(&temp.path)
            .expect("resolving a primary workdir should succeed");
        let expected = temp.path.canonicalize().expect("canonicalize primary");
        assert_eq!(primary, expected);
        assert_eq!(active, expected);
    }

    #[test]
    fn resolve_primary_and_active_from_worktree_path_returns_primary_and_given() {
        use git2::WorktreeAddOptions;

        let (temp, repo) = init_test_repo("resolve_worktree");
        let worktree_dir = std::env::temp_dir().join(format!(
            "git_leviathan_resolve_worktree_dir_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));

        let _cleanup = CleanupDir(worktree_dir.clone());
        let opts = WorktreeAddOptions::new();
        let _wt = repo
            .worktree("test-wt", &worktree_dir, Some(&opts))
            .expect("add worktree for test");

        let (primary, active) = resolve_primary_and_active(&worktree_dir)
            .expect("resolving a secondary workdir should succeed");

        let expected_primary = temp.path.canonicalize().expect("canonicalize primary");
        let expected_active = worktree_dir.canonicalize().expect("canonicalize worktree");
        assert_eq!(primary, expected_primary);
        assert_eq!(active, expected_active);
    }
}

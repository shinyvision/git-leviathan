use crate::services::git::process::capture_program;
use crate::services::git_error::GitError;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
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
        match self {
            GitStatus::Available { path, version } => {
                let _ = (path, version);
                true
            }
            GitStatus::NotFound | GitStatus::MacOsCommandLineToolsMissing => false,
        }
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
    let output = capture_program(cmd, "git --version", Some(Duration::from_secs(3))).ok()?;
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
pub fn resolve_primary_and_active(path: &Path) -> Result<(PathBuf, PathBuf), GitError> {
    let canonical_active = path
        .canonicalize()
        .map_err(|e| GitError::Other(format!("canonicalize '{}': {e}", path.display())))?;

    let repo = git2::Repository::open(&canonical_active).map_err(|e| {
        GitError::Other(format!(
            "open repo at '{}': {}",
            canonical_active.display(),
            e.message()
        ))
    })?;

    Ok((primary_repository_path(&repo)?, canonical_active))
}

/// Use Git's common directory for linked worktrees. Its name and location
/// are configurable and a bare primary has no checked-out working tree.
pub(super) fn primary_repository_path(repo: &git2::Repository) -> Result<PathBuf, GitError> {
    let common_repo;
    let primary = if repo.is_worktree() {
        let common = std::fs::read_to_string(repo.path().join("commondir"))
            .map_err(|e| GitError::Other(format!("read worktree common directory: {e}")))?;
        common_repo =
            git2::Repository::open(repo.path().join(common.trim_end_matches(['\r', '\n'])))
                .map_err(|e| GitError::Other(format!("open worktree common directory: {e}")))?;
        &common_repo
    } else {
        repo
    };
    let path = primary.workdir().unwrap_or_else(|| primary.path());
    path.canonicalize()
        .map_err(|e| GitError::Other(format!("canonicalize primary '{}': {e}", path.display())))
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

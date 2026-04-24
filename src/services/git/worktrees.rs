use std::path::Path;

use crate::core::WorktreeInfo;
use crate::services::git::helpers::wrap_git2_error;
use crate::services::git_error::GitError;

use super::GitService;

pub(super) fn list_worktrees(service: &GitService) -> Result<Vec<WorktreeInfo>, GitError> {
    let mut results = Vec::new();

    // Resolve the PRIMARY workdir independently of what this service happens
    // to have open. When a GitService is opened on a linked worktree,
    // `repo.workdir()` returns that linked workdir — not the primary — and
    // `repo.worktrees()` never lists the primary (git treats it specially).
    // Without deriving it here, branches checked out in the primary would
    // look "unowned" from a secondary, and clicking them would checkout on
    // the current worktree instead of swapping focus.
    //
    // Derivation: `repo.path()` is `.git/` for the primary and
    // `<primary>/.git/worktrees/<name>/` for a linked worktree. Walk up to
    // the common `.git` dir, then up one more for the primary workdir.
    let primary_workdir = if service.repo.is_worktree() {
        service
            .repo
            .path()
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .ok_or_else(|| {
                GitError::Other("cannot derive primary workdir from linked worktree".to_string())
            })?
    } else {
        service
            .repo
            .workdir()
            .ok_or_else(|| GitError::Other("repository has no workdir (bare)".to_string()))?
            .to_path_buf()
    };
    let primary_path = primary_workdir
        .canonicalize()
        .map_err(|e| GitError::Other(format!("canonicalize primary workdir: {e}")))?;
    let (primary_head, primary_branch) = head_info_for_workdir(&primary_path)?;
    results.push(WorktreeInfo {
        path: primary_path,
        branch_name: primary_branch,
        head_hash: primary_head,
        is_primary: true,
        is_locked: false,
        is_prunable: false,
    });

    let names = service
        .repo
        .worktrees()
        .map_err(|e| wrap_git2_error("enumerate worktrees", e))?;
    for i in 0..names.len() {
        let Some(name) = names.get(i) else { continue; };
        let worktree = service
            .repo
            .find_worktree(name)
            .map_err(|e| wrap_git2_error(&format!("find worktree '{name}'"), e))?;
        let is_locked = matches!(
            worktree.is_locked(),
            Ok(git2::WorktreeLockStatus::Locked(_))
        );
        let is_prunable = worktree.validate().is_err();
        let path = worktree.path().to_path_buf();
        let (head_hash, branch_name) = if is_prunable {
            (String::new(), String::new())
        } else {
            head_info_for_workdir(&path)?
        };
        let canonical = path
            .canonicalize()
            .unwrap_or_else(|_| path.clone());
        results.push(WorktreeInfo {
            path: canonical,
            branch_name,
            head_hash,
            is_primary: false,
            is_locked,
            is_prunable,
        });
    }

    Ok(results)
}

fn head_info_for_workdir(path: &Path) -> Result<(String, String), GitError> {
    let repo = git2::Repository::open(path)
        .map_err(|e| wrap_git2_error(&format!("open workdir '{}'", path.display()), e))?;
    let head = repo.head().ok();
    let head_hash = head
        .as_ref()
        .and_then(|h| h.target())
        .map(|oid| oid.to_string())
        .unwrap_or_default();
    let branch_name = head
        .as_ref()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_default();
    Ok((head_hash, branch_name))
}

/// Create a new worktree at `path`.
///
/// - `new_branch_name: Some(name)` — if a local branch with `name` already
///   exists, check it out into the worktree; otherwise create a new local
///   branch at `ref_to_checkout`'s commit. If `ref_to_checkout` is a
///   remote-tracking ref (e.g. `origin/foo`), the new branch's upstream is
///   set accordingly.
/// - `new_branch_name: None` — `ref_to_checkout` MUST be the short name of
///   an existing local branch (e.g. `"main"`); the worktree checks out that
///   branch directly. Passing a tag, remote ref, or SHA in this arm returns
///   a `GitError::Other("find ref for …")`.
pub(super) fn add_worktree(
    service: &GitService,
    path: &Path,
    new_branch_name: Option<&str>,
    ref_to_checkout: &str,
) -> Result<(), GitError> {
    // libgit2's `repo.worktree()` calls mkdir (not mkdir -p) internally, so
    // any missing parent — common when the default dir lives under a
    // `<repo>.worktrees/` sibling that hasn't been created yet — surfaces as
    // "failed to make directory". Pre-create the parent chain.
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                GitError::Other(format!(
                    "create parent '{}' for worktree: {e}",
                    parent.display(),
                ))
            })?;
        }
    }

    // libgit2's `repo.worktree()` calls mkdir internally and fails if the
    // target already exists — even when empty. The UI's default working-dir
    // often points at a pre-created empty dir (user clicked Browse, or the
    // parent `.worktrees` tree was scaffolded), so handle that case here:
    // reject non-empty dirs, remove empty ones so libgit2 can mkdir fresh.
    if path.exists() {
        let is_empty = path
            .read_dir()
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            return Err(GitError::Other(format!(
                "worktree path '{}' already exists and is not empty",
                path.display(),
            )));
        }
        std::fs::remove_dir(path).map_err(|e| {
            GitError::Other(format!(
                "remove empty dir '{}' before worktree add: {e}",
                path.display(),
            ))
        })?;
    }

    let object = service
        .repo
        .revparse_single(ref_to_checkout)
        .map_err(|e| wrap_git2_error(&format!("resolve ref '{ref_to_checkout}'"), e))?;
    let commit = object
        .peel_to_commit()
        .map_err(|e| wrap_git2_error(&format!("peel '{ref_to_checkout}' to commit"), e))?;

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| GitError::Other(format!("invalid worktree path '{}'", path.display())))?;

    // A previous failed `add_worktree` may have left `.git/worktrees/<name>`
    // behind. libgit2 refuses to reuse that slot and surfaces it as
    // "failed to make directory '<git_dir>/worktrees/<name>': directory
    // exists". Prune any matching metadata (valid or not) so the new add
    // can claim the name.
    if let Ok(existing) = service.repo.find_worktree(name) {
        let mut opts = git2::WorktreePruneOptions::new();
        opts.valid(true).working_tree(true).locked(true);
        let _ = existing.prune(Some(&mut opts));
    }
    let stale_meta = service.repo.path().join("worktrees").join(name);
    if stale_meta.exists() {
        std::fs::remove_dir_all(&stale_meta).map_err(|e| {
            GitError::Other(format!(
                "remove stale worktree metadata '{}': {e}",
                stale_meta.display(),
            ))
        })?;
    }

    let branch_reference = match new_branch_name {
        Some(new_name) => {
            let existing = service.repo.find_branch(new_name, git2::BranchType::Local);
            if existing.is_ok() {
                service
                    .repo
                    .find_reference(&format!("refs/heads/{new_name}"))
                    .map_err(|e| wrap_git2_error(&format!("find ref for '{new_name}'"), e))?
            } else {
                service
                    .repo
                    .branch(new_name, &commit, false)
                    .map_err(|e| wrap_git2_error(&format!("create branch '{new_name}'"), e))?;
                if ref_to_checkout.contains('/')
                    && service
                        .repo
                        .find_reference(&format!("refs/remotes/{ref_to_checkout}"))
                        .is_ok()
                {
                    if let Ok(mut b) = service
                        .repo
                        .find_branch(new_name, git2::BranchType::Local)
                    {
                        let _ = b.set_upstream(Some(ref_to_checkout));
                    }
                }
                service
                    .repo
                    .find_reference(&format!("refs/heads/{new_name}"))
                    .map_err(|e| wrap_git2_error(&format!("find ref for '{new_name}'"), e))?
            }
        }
        None => service
            .repo
            .find_reference(&format!("refs/heads/{ref_to_checkout}"))
            .map_err(|e| wrap_git2_error(&format!("find ref for '{ref_to_checkout}'"), e))?,
    };

    let mut opts = git2::WorktreeAddOptions::new();
    opts.reference(Some(&branch_reference));
    service
        .repo
        .worktree(name, path, Some(&opts))
        .map_err(|e| wrap_git2_error(&format!("add worktree '{name}'"), e))?;

    Ok(())
}

pub(super) fn remove_worktree(
    service: &GitService,
    path: &Path,
    force: bool,
) -> Result<(), GitError> {
    let canonical = path
        .canonicalize()
        .map_err(|e| GitError::Other(format!("canonicalize '{}': {e}", path.display())))?;

    let names = service
        .repo
        .worktrees()
        .map_err(|e| wrap_git2_error("enumerate worktrees", e))?;
    let mut found_name: Option<String> = None;
    for i in 0..names.len() {
        let Some(name) = names.get(i) else { continue; };
        let wt = service
            .repo
            .find_worktree(name)
            .map_err(|e| wrap_git2_error(&format!("find worktree '{name}'"), e))?;
        if wt.path().canonicalize().ok().as_deref() == Some(canonical.as_path()) {
            found_name = Some(name.to_string());
            break;
        }
    }
    let name = found_name.ok_or_else(|| {
        GitError::Other(format!(
            "no worktree registered at '{}'",
            path.display(),
        ))
    })?;

    let wt = service
        .repo
        .find_worktree(&name)
        .map_err(|e| wrap_git2_error(&format!("find worktree '{name}'"), e))?;
    let lock_status = wt
        .is_locked()
        .map_err(|e| wrap_git2_error(&format!("check lock status for worktree '{name}'"), e))?;
    if matches!(lock_status, git2::WorktreeLockStatus::Locked(_)) && !force {
        return Err(GitError::Other(format!(
            "worktree '{name}' is locked; unlock it first",
        )));
    }

    if canonical.exists() {
        std::fs::remove_dir_all(&canonical).map_err(|e| {
            GitError::Other(format!("remove worktree dir '{}': {e}", canonical.display()))
        })?;
    }

    let mut prune_opts = git2::WorktreePruneOptions::new();
    prune_opts.valid(true);
    if force {
        prune_opts.locked(true);
    }
    wt.prune(Some(&mut prune_opts))
        .map_err(|e| wrap_git2_error(&format!("prune worktree '{name}'"), e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::git::GitService;
    use crate::services::test_support::init_test_repo;

    struct CleanupDir(std::path::PathBuf);
    impl Drop for CleanupDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn unique_wt_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "git_leviathan_wt_{tag}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ))
    }

    #[test]
    fn list_worktrees_returns_primary_only_when_no_extras() {
        let (temp, _repo) = init_test_repo("worktrees_list_primary");
        let service = GitService::open(temp.path_str()).expect("open service");
        let worktrees = list_worktrees(&service).expect("list worktrees");
        assert_eq!(worktrees.len(), 1);
        let primary = &worktrees[0];
        assert!(primary.is_primary, "expected primary=true for sole entry");
        assert_eq!(
            primary.path,
            temp.path.canonicalize().expect("canonicalize primary"),
        );
    }

    #[test]
    fn list_worktrees_from_secondary_includes_primary() {
        let (temp, repo) = init_test_repo("worktrees_list_from_secondary");
        let default_branch = repo
            .head()
            .unwrap()
            .shorthand()
            .unwrap()
            .to_string();
        let primary_service =
            GitService::open(temp.path_str()).expect("open primary service");
        let wt_path = unique_wt_path("from_secondary");
        let _cleanup = CleanupDir(wt_path.clone());
        add_worktree(&primary_service, &wt_path, Some("feat-sec"), &default_branch)
            .expect("add secondary");

        // Open service FROM the secondary worktree path.
        let secondary_service = GitService::open(wt_path.to_str().unwrap())
            .expect("open secondary service");
        let worktrees = list_worktrees(&secondary_service).expect("list from secondary");

        // Must include an entry flagged is_primary=true whose path is the
        // original repo root — otherwise the UI can't route checkout of the
        // primary's branch into a focus-swap.
        let primary_entry = worktrees
            .iter()
            .find(|w| w.is_primary)
            .expect("primary entry present when opened from secondary");
        assert_eq!(
            primary_entry.path,
            temp.path.canonicalize().expect("canon primary"),
        );
        assert_eq!(primary_entry.branch_name, default_branch);
    }

    #[test]
    fn add_worktree_creates_new_branch_and_dir() {
        let (temp, repo) = init_test_repo("worktrees_add_new_branch");
        let default_branch = repo
            .head()
            .expect("repo should have head")
            .shorthand()
            .expect("head should be utf-8")
            .to_string();
        let service = GitService::open(temp.path_str()).expect("open service");
        let wt_path = unique_wt_path("new");
        let _cleanup = CleanupDir(wt_path.clone());

        add_worktree(&service, &wt_path, Some("feature-x"), &default_branch)
            .expect("add_worktree with new branch should succeed");

        assert!(wt_path.exists(), "worktree directory should exist");
        assert!(wt_path.join(".git").exists(), ".git pointer should exist");

        let worktrees = list_worktrees(&service).expect("list after add");
        assert_eq!(worktrees.len(), 2);
        let added = worktrees
            .iter()
            .find(|w| !w.is_primary)
            .expect("found secondary");
        assert_eq!(added.branch_name, "feature-x");

        let repo = git2::Repository::open(temp.path_str()).unwrap();
        assert!(repo
            .find_branch("feature-x", git2::BranchType::Local)
            .is_ok());
    }

    #[test]
    fn add_worktree_with_existing_branch_checks_it_out() {
        use crate::services::test_support::create_branch;
        let (temp, repo) = init_test_repo("worktrees_add_existing");
        let default_branch = repo
            .head()
            .expect("repo should have head")
            .shorthand()
            .expect("head should be utf-8")
            .to_string();
        create_branch(&repo, "existing-branch");
        let service = GitService::open(temp.path_str()).expect("open service");
        let wt_path = unique_wt_path("existing");
        let _cleanup = CleanupDir(wt_path.clone());

        add_worktree(&service, &wt_path, Some("existing-branch"), &default_branch)
            .expect("add worktree re-using existing branch");

        let worktrees = list_worktrees(&service).expect("list after add");
        let added = worktrees
            .iter()
            .find(|w| !w.is_primary)
            .expect("secondary present");
        assert_eq!(added.branch_name, "existing-branch");

        let repo = git2::Repository::open(temp.path_str()).unwrap();
        let local_count = repo
            .branches(Some(git2::BranchType::Local))
            .unwrap()
            .count();
        assert_eq!(local_count, 2, "{default_branch} + existing-branch");
    }

    #[test]
    fn add_worktree_with_same_branch_and_ref_reuses_branch() {
        use crate::services::test_support::create_branch;
        let (temp, repo) = init_test_repo("worktrees_add_same_name");
        create_branch(&repo, "shared");
        let service = GitService::open(temp.path_str()).expect("open service");
        let wt_path = unique_wt_path("same_name");
        let _cleanup = CleanupDir(wt_path.clone());

        add_worktree(&service, &wt_path, Some("shared"), "shared")
            .expect("same branch_name and ref should reuse existing branch");

        let worktrees = list_worktrees(&service).expect("list after add");
        let added = worktrees.iter().find(|w| !w.is_primary).expect("secondary");
        assert_eq!(added.branch_name, "shared");

        let repo = git2::Repository::open(temp.path_str()).unwrap();
        let local_count = repo
            .branches(Some(git2::BranchType::Local))
            .unwrap()
            .count();
        assert_eq!(local_count, 2, "no duplicate branch should be created");
    }

    #[test]
    fn add_worktree_creates_missing_parent_dirs() {
        let (temp, repo) = init_test_repo("worktrees_add_missing_parent");
        let default_branch = repo
            .head()
            .unwrap()
            .shorthand()
            .unwrap()
            .to_string();
        let service = GitService::open(temp.path_str()).expect("open service");
        // Path whose parent (and grandparent) doesn't exist yet.
        let base = unique_wt_path("missing_parent");
        let wt_path = base.join("nested").join("child");
        let _cleanup = CleanupDir(base.clone());

        add_worktree(&service, &wt_path, Some("feat-missing"), &default_branch)
            .expect("add should create missing parent chain");

        assert!(wt_path.exists(), "worktree dir created");
    }

    #[test]
    fn add_worktree_succeeds_when_path_exists_but_empty() {
        let (temp, repo) = init_test_repo("worktrees_add_empty_existing");
        let default_branch = repo
            .head()
            .unwrap()
            .shorthand()
            .unwrap()
            .to_string();
        let service = GitService::open(temp.path_str()).expect("open service");
        let wt_path = unique_wt_path("empty_existing");
        let _cleanup = CleanupDir(wt_path.clone());
        std::fs::create_dir_all(&wt_path).expect("pre-create empty dir");

        add_worktree(&service, &wt_path, Some("feat-empty"), &default_branch)
            .expect("add should tolerate pre-existing empty dir");

        assert!(wt_path.exists(), "worktree dir should be populated");
    }

    #[test]
    fn add_worktree_clears_stale_metadata_and_retries() {
        let (temp, repo) = init_test_repo("worktrees_stale_meta");
        let default_branch = repo
            .head()
            .unwrap()
            .shorthand()
            .unwrap()
            .to_string();
        let service = GitService::open(temp.path_str()).expect("open service");
        let wt_path = unique_wt_path("stale_meta");
        let _cleanup = CleanupDir(wt_path.clone());
        // Simulate leftover metadata from an aborted earlier add.
        let name = wt_path.file_name().unwrap().to_str().unwrap();
        let meta = service.repo.path().join("worktrees").join(name);
        std::fs::create_dir_all(&meta).expect("seed stale meta");

        add_worktree(&service, &wt_path, Some("stale-recover"), &default_branch)
            .expect("add should recover from stale metadata");

        assert!(wt_path.exists(), "worktree dir created");
    }

    #[test]
    fn add_worktree_fails_when_path_not_empty() {
        let (temp, _repo) = init_test_repo("worktrees_add_conflict");
        let service = GitService::open(temp.path_str()).expect("open service");
        let wt_path = unique_wt_path("conflict");
        let _cleanup = CleanupDir(wt_path.clone());
        std::fs::create_dir_all(&wt_path).expect("create conflict dir");
        std::fs::write(wt_path.join("something"), "nope").unwrap();

        let result = add_worktree(&service, &wt_path, Some("feature-y"), "HEAD");
        assert!(result.is_err(), "add should reject non-empty target");
    }

    #[test]
    fn remove_worktree_deletes_dir_and_metadata() {
        let (temp, repo) = init_test_repo("worktrees_remove");
        let default_branch = repo
            .head()
            .expect("repo should have head")
            .shorthand()
            .expect("head should be utf-8")
            .to_string();
        let service = GitService::open(temp.path_str()).expect("open service");
        let wt_path = unique_wt_path("remove");
        let _cleanup = CleanupDir(wt_path.clone());

        add_worktree(&service, &wt_path, Some("tmp"), &default_branch)
            .expect("add worktree");

        remove_worktree(&service, &wt_path, false).expect("remove worktree");

        assert!(!wt_path.exists(), "worktree dir should be gone");
        let worktrees = list_worktrees(&service).expect("list after remove");
        assert_eq!(worktrees.len(), 1, "only primary should remain");
    }

    #[test]
    fn add_worktree_with_none_branch_checks_out_existing_branch() {
        use crate::services::test_support::{checkout_branch_for_setup, create_branch};
        let (temp, repo) = init_test_repo("worktrees_add_none");
        let default_branch = repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().map(|s| s.to_string()))
            .expect("head shorthand");

        // Move the primary off `default_branch` so the worktree can claim it.
        create_branch(&repo, "other");
        checkout_branch_for_setup(&repo, "other");

        let service = GitService::open(temp.path_str()).expect("open service");
        let wt_path = unique_wt_path("none");
        let _cleanup = CleanupDir(wt_path.clone());

        add_worktree(&service, &wt_path, None, &default_branch)
            .expect("add_worktree with None branch_name should succeed");

        let worktrees = list_worktrees(&service).expect("list after add");
        let secondary = worktrees
            .iter()
            .find(|w| !w.is_primary)
            .expect("secondary");
        assert_eq!(secondary.branch_name, default_branch);
    }

    #[test]
    fn remove_worktree_fails_when_locked() {
        let (temp, repo) = init_test_repo("worktrees_remove_locked");
        let default_branch = repo
            .head()
            .expect("repo should have head")
            .shorthand()
            .expect("head should be utf-8")
            .to_string();
        let service = GitService::open(temp.path_str()).expect("open service");
        let wt_path = unique_wt_path("locked");
        let _cleanup = CleanupDir(wt_path.clone());

        add_worktree(&service, &wt_path, Some("tmp-lock"), &default_branch).expect("add");

        // Lock the worktree through libgit2 so remove_worktree fails.
        let names = service.repo.worktrees().expect("worktrees list");
        let mut locked_name: Option<String> = None;
        for i in 0..names.len() {
            let Some(name) = names.get(i) else { continue; };
            let wt = service.repo.find_worktree(name).expect("find");
            if wt.path().canonicalize().ok().as_deref()
                == wt_path.canonicalize().ok().as_deref()
            {
                wt.lock(Some("test lock")).expect("lock");
                locked_name = Some(name.to_string());
                break;
            }
        }
        locked_name.expect("locked a worktree for the test");

        let result = remove_worktree(&service, &wt_path, false);
        assert!(result.is_err(), "remove should refuse a locked worktree when !force");

        // Unlock for cleanup — CleanupDir will drop the dir; the metadata stays
        // but that's acceptable for test isolation.
    }
}

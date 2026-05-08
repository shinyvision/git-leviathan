use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::services::{
    load_commit_diff, load_merged_commit_diff, load_merged_commit_file_diff, BranchMergeOutcome,
    CherryPickOutcome, CommitDiffResult, ConflictResolutionResult, DirtyDiffSignature,
    DirtySnapshot, GitError, GitService, MergedCommitDiffResult, ModifyDeleteConflict,
    ModifyDeleteConflictChoice, PushOutcome, RefsSnapshot, RemoteCheckoutOutcome, RepoSnapshot,
    ResetMode, RevertOutcome, StashApplyOutcome, WorkingTreeDiffResult, WorktreeInfo,
    COMMIT_LOAD_LIMIT,
};

use super::branch_ops::BranchOps;
use super::commit_ops::CommitOps;
use super::git_worktree_ops::GitWorktreeOps;
use super::read::RepoRead;
use super::remote_ops::{PushGatewayOutcome, RemoteOps};
use super::stash_ops::StashOps;
use super::tag_ops::TagOps;
use super::working_tree_ops::WorkingTreeOps;
use super::Repository;

pub type SharedRepositoryGateway = Arc<dyn Repository>;

#[derive(Debug, Clone)]
pub struct GitRepositoryGateway {
    repo_path: String,
    operation_lock: Arc<Mutex<()>>,
    /// Cache of tag → set of remote names known to hold the tag. Populated by
    /// `fetch_remotes` (via `git ls-remote --tags`) and mutated by `push_tag`
    /// / `delete_remote_tag`. Empty until first fetch.
    tag_remotes: Arc<Mutex<HashMap<String, HashSet<String>>>>,
}

impl GitRepositoryGateway {
    pub fn from_path(repo_path: impl Into<String>) -> SharedRepositoryGateway {
        Arc::new(Self {
            repo_path: repo_path.into(),
            operation_lock: Arc::new(Mutex::new(())),
            tag_remotes: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    #[cfg(test)]
    pub(super) fn from_repo_path(repo_path: impl Into<String>) -> Self {
        Self {
            repo_path: repo_path.into(),
            operation_lock: Arc::new(Mutex::new(())),
            tag_remotes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(super) fn refresh_tag_remotes_cache(&self, service: &GitService) {
        let remotes = service.list_remotes();
        let mut new_cache: HashMap<String, HashSet<String>> = HashMap::new();
        for remote in &remotes {
            match service.list_remote_tags(remote) {
                Ok(tags) => {
                    for tag in tags {
                        new_cache.entry(tag).or_default().insert(remote.clone());
                    }
                }
                Err(e) => {
                    eprintln!("git_leviathan: ls-remote --tags {remote} failed: {e}");
                }
            }
        }
        if let Ok(mut cache) = self.tag_remotes.lock() {
            *cache = new_cache;
        }
    }

    pub(super) fn refresh_tag_remote_cache(&self, service: &GitService, remote_name: &str) {
        let tags = service.list_remote_tags(remote_name);

        if let Ok(mut cache) = self.tag_remotes.lock() {
            cache.retain(|_, remotes| {
                remotes.remove(remote_name);
                !remotes.is_empty()
            });
            match tags {
                Ok(tags) => {
                    for tag in tags {
                        cache
                            .entry(tag)
                            .or_default()
                            .insert(remote_name.to_string());
                    }
                }
                Err(e) => {
                    eprintln!("git_leviathan: ls-remote --tags {remote_name} failed: {e}");
                }
            }
        }
    }

    pub(super) fn add_tag_remote_to_cache(&self, tag_name: &str, remote_name: &str) {
        if let Ok(mut cache) = self.tag_remotes.lock() {
            cache
                .entry(tag_name.to_string())
                .or_default()
                .insert(remote_name.to_string());
        }
    }

    pub(super) fn remove_tag_remote_from_cache(&self, tag_name: &str, remote_name: &str) {
        if let Ok(mut cache) = self.tag_remotes.lock() {
            if let Some(set) = cache.get_mut(tag_name) {
                set.remove(remote_name);
                if set.is_empty() {
                    cache.remove(tag_name);
                }
            }
        }
    }

    pub(super) fn remove_tag_from_cache(&self, tag_name: &str) {
        if let Ok(mut cache) = self.tag_remotes.lock() {
            cache.remove(tag_name);
        }
    }

    pub(super) fn tag_remotes_snapshot(&self, tag_name: &str) -> Vec<String> {
        let cache = match self.tag_remotes.lock() {
            Ok(cache) => cache,
            Err(_) => return Vec::new(),
        };
        let Some(set) = cache.get(tag_name) else {
            return Vec::new();
        };
        let mut names: Vec<String> = set.iter().cloned().collect();
        names.sort();
        names
    }

    pub(super) fn with_service<T>(
        &self,
        f: impl FnOnce(&mut GitService) -> Result<T, GitError>,
    ) -> Result<T, GitError> {
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| GitError::Other("repository operation lock poisoned".to_string()))?;
        self.with_service_unlocked(f)
    }

    /// Opens a `GitService` without acquiring the operation lock.
    ///
    /// Read-only callers (diff loading) must not block on long-running writers
    /// like `fetch_and_reload`. git2 handles concurrent reads safely.
    pub(super) fn with_service_unlocked<T>(
        &self,
        f: impl FnOnce(&mut GitService) -> Result<T, GitError>,
    ) -> Result<T, GitError> {
        let mut service =
            GitService::open(&self.repo_path).map_err(|e| GitError::Other(e.to_string()))?;
        f(&mut service)
    }

    fn write_then_load_dirty(
        &self,
        write: impl FnOnce(&mut GitService) -> Result<(), GitError>,
    ) -> Result<Option<DirtySnapshot>, GitError> {
        self.with_service(write)?;
        self.with_service_unlocked(|service| Ok(service.load_dirty_snapshot()))
    }
}

impl RepoRead for GitRepositoryGateway {
    fn load_repo(&self, commit_limit: usize) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| Ok(service.load_repo(commit_limit)))
    }

    fn load_refs_snapshot(&self, commit_limit: usize) -> Result<RefsSnapshot, GitError> {
        self.with_service(|service| Ok(service.load_refs_snapshot(commit_limit)))
    }

    fn load_commit_diff(
        &self,
        commit_idx: usize,
        hash: &str,
    ) -> Result<CommitDiffResult, GitError> {
        load_commit_diff(&self.repo_path, commit_idx, hash)
            .map_err(|e| GitError::Other(e.to_string()))
    }

    fn load_merged_commit_diff(
        &self,
        hashes: Vec<String>,
    ) -> Result<MergedCommitDiffResult, GitError> {
        load_merged_commit_diff(&self.repo_path, &hashes)
            .map_err(|e| GitError::Other(e.to_string()))
    }

    fn load_working_tree_diff(
        &self,
        file_path: &str,
        is_staged: bool,
    ) -> Result<WorkingTreeDiffResult, GitError> {
        self.with_service_unlocked(|service| service.load_working_tree_diff(file_path, is_staged))
    }

    fn load_commit_file_diff(
        &self,
        commit_hash: &str,
        file_path: &str,
    ) -> Result<WorkingTreeDiffResult, GitError> {
        self.with_service_unlocked(|service| service.load_commit_file_diff(commit_hash, file_path))
    }

    fn load_merged_commit_file_diff(
        &self,
        hashes: &[String],
        file_path: &str,
    ) -> Result<WorkingTreeDiffResult, GitError> {
        load_merged_commit_file_diff(&self.repo_path, hashes, file_path)
    }

    fn load_conflict_resolution(
        &self,
        file_path: &str,
    ) -> Result<ConflictResolutionResult, GitError> {
        self.with_service_unlocked(|service| service.load_conflict_resolution(file_path))
    }

    fn load_modify_delete_conflict(
        &self,
        file_path: &str,
    ) -> Result<Option<ModifyDeleteConflict>, GitError> {
        self.with_service_unlocked(|service| service.load_modify_delete_conflict(file_path))
    }

    fn compute_dirty_file_signature(&self, file_path: &str, is_staged: bool) -> DirtyDiffSignature {
        self.with_service_unlocked(|service| {
            Ok(service.compute_dirty_file_signature(file_path, is_staged))
        })
        .unwrap_or_default()
    }
}

impl BranchOps for GitRepositoryGateway {
    fn checkout_branch(&self, branch_name: &str) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.checkout_branch(branch_name)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn checkout_remote_branch(&self, branch_name: &str) -> Result<RemoteCheckoutOutcome, GitError> {
        self.with_service(|service| service.checkout_remote_branch(branch_name))
    }

    fn create_branch_from_remote(
        &self,
        new_name: &str,
        remote_ref: &str,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.create_branch_from_remote_ref_and_checkout(new_name, remote_ref)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn reset_branch_to_remote(
        &self,
        branch_name: &str,
        remote_ref: &str,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.reset_branch_to_remote_and_checkout(branch_name, remote_ref)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn delete_branch(&self, branch_name: &str, is_remote: bool) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.delete_branch(branch_name, is_remote)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn delete_branch_all(&self, branch_name: &str) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.delete_branch_all(branch_name)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn rename_branch(
        &self,
        old_name: &str,
        new_name: &str,
        is_remote: bool,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.rename_branch(old_name, new_name, is_remote)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn create_branch_at_commit(
        &self,
        branch_name: &str,
        commit_hash: &str,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.create_branch_at_commit(branch_name, commit_hash)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn merge_branch_into(
        &self,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<BranchMergeOutcome, GitError> {
        self.with_service(|service| service.merge_branch_into(source_branch, target_branch))
    }

    fn fast_forward_branch_to_branch(
        &self,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.fast_forward_branch_to_branch(source_branch, target_branch)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn abort_merge(&self) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.abort_merge()?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn rebase_current_onto(
        &self,
        source_branch: &str,
        target_ref: &str,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.rebase_current_onto(source_branch, target_ref)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }
}

impl WorkingTreeOps for GitRepositoryGateway {
    fn stage_file(&self, path: &str) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.stage_file(path)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn stage_all_dirty_changes(&self) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.stage_all_dirty_changes()?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn unstage_file(&self, path: &str) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.unstage_file(path)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn unstage_all_dirty_changes(&self) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.unstage_all_dirty_changes()?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn stage_file_and_load_dirty(&self, path: &str) -> Result<Option<DirtySnapshot>, GitError> {
        self.write_then_load_dirty(|service| service.stage_file(path))
    }

    fn stage_all_dirty_changes_and_load_dirty(&self) -> Result<Option<DirtySnapshot>, GitError> {
        self.write_then_load_dirty(|service| service.stage_all_dirty_changes())
    }

    fn unstage_file_and_load_dirty(&self, path: &str) -> Result<Option<DirtySnapshot>, GitError> {
        self.write_then_load_dirty(|service| service.unstage_file(path))
    }

    fn unstage_all_dirty_changes_and_load_dirty(&self) -> Result<Option<DirtySnapshot>, GitError> {
        self.write_then_load_dirty(|service| service.unstage_all_dirty_changes())
    }

    fn mark_conflict_resolved(&self, path: &str) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.mark_conflict_resolved(path)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn mark_all_conflicts_resolved(&self) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.mark_all_conflicts_resolved()?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn resolve_modify_delete_conflict(
        &self,
        path: &str,
        choice: ModifyDeleteConflictChoice,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.resolve_modify_delete_conflict(path, choice)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn discard_file(&self, path: &str) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.discard_file(path)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn discard_all_dirty_changes(&self) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.discard_all_dirty_changes()?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn commit_dirty_changes(
        &self,
        summary: &str,
        description: &str,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.commit_dirty_changes(summary, description)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn save_conflict_resolution(
        &self,
        file_path: &str,
        content: &str,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.save_conflict_resolution(file_path, content)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }
}

impl GitWorktreeOps for GitRepositoryGateway {
    fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, GitError> {
        self.with_service(|service| service.list_worktrees())
    }

    fn add_worktree(
        &self,
        path: std::path::PathBuf,
        new_branch_name: Option<String>,
        ref_to_checkout: String,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.add_worktree(&path, new_branch_name.as_deref(), &ref_to_checkout)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn remove_worktree(&self, path: std::path::PathBuf) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.remove_worktree(&path, false)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }
}

impl CommitOps for GitRepositoryGateway {
    fn reset_current_branch_to_commit(
        &self,
        commit_hash: &str,
        mode: ResetMode,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.reset_current_branch_to_commit(commit_hash, mode)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn squash_commits(&self, hashes: Vec<String>) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.squash_commits(hashes)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn cherry_pick_commit(
        &self,
        commit_hash: String,
        immediate_commit: bool,
    ) -> Result<CherryPickOutcome, GitError> {
        self.with_service(|service| service.cherry_pick_commit(&commit_hash, immediate_commit))
    }

    fn revert_commit(
        &self,
        commit_hash: String,
        immediate_commit: bool,
    ) -> Result<RevertOutcome, GitError> {
        self.with_service(|service| service.revert_commit(&commit_hash, immediate_commit))
    }

    fn reword_commit(&self, hash: String, new_message: String) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.reword_commit(&hash, &new_message)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }
}

impl RemoteOps for GitRepositoryGateway {
    fn fetch_remotes(&self) -> Result<(), GitError> {
        // resource lifecycle (write-lock): network fetch only.
        // screen routing (no write-lock): `git ls-remote --tags` per remote to refresh
        // the tag→remotes cache. Held under the writer lock in the past, this
        // stretched lock-hold time by (num_remotes × round-trip) and caused
        // file-watcher reload_refs tasks to pile up during the fetch; keep it
        // strictly outside the writer lock so readers aren't starved.
        self.with_service(|service| service.fetch_all_refs())?;
        self.with_service_unlocked(|service| {
            self.refresh_tag_remotes_cache(service);
            Ok(())
        })
    }

    fn fetch_remote(&self, remote_name: &str) -> Result<(), GitError> {
        // Keep the write-lock scoped to the requested fetch. The tag cache is
        // refreshed for only that remote after the fetch completes.
        self.with_service(|service| service.fetch_remote_refs(remote_name))?;
        self.with_service_unlocked(|service| {
            self.refresh_tag_remote_cache(service, remote_name);
            Ok(())
        })
    }

    fn push_current_branch(&self) -> Result<PushGatewayOutcome, GitError> {
        self.with_service(|service| match service.push_current_branch()? {
            PushOutcome::Pushed => Ok(PushGatewayOutcome::Pushed(Box::new(
                service.load_repo(COMMIT_LOAD_LIMIT),
            ))),
            PushOutcome::NeedsUpstream {
                branch_name,
                remote_name,
            } => Ok(PushGatewayOutcome::NeedsUpstream {
                branch_name,
                remote_name,
            }),
            PushOutcome::BehindRemote {
                branch_name,
                remote_name,
            } => Ok(PushGatewayOutcome::BehindRemote {
                branch_name,
                remote_name,
            }),
        })
    }

    fn push_ref_to_remote(
        &self,
        remote_name: &str,
        ref_name: &str,
    ) -> Result<PushGatewayOutcome, GitError> {
        self.with_service(
            |service| match service.push_ref_to_remote(remote_name, ref_name)? {
                PushOutcome::Pushed => Ok(PushGatewayOutcome::Pushed(Box::new(
                    service.load_repo(COMMIT_LOAD_LIMIT),
                ))),
                PushOutcome::NeedsUpstream {
                    branch_name,
                    remote_name,
                } => Ok(PushGatewayOutcome::NeedsUpstream {
                    branch_name,
                    remote_name,
                }),
                PushOutcome::BehindRemote {
                    branch_name,
                    remote_name,
                } => Ok(PushGatewayOutcome::BehindRemote {
                    branch_name,
                    remote_name,
                }),
            },
        )
    }

    fn current_branch_push_remote(&self) -> Result<String, GitError> {
        self.with_service(|service| service.current_branch_push_remote())
    }

    fn push_and_set_upstream(
        &self,
        remote_name: &str,
        remote_branch_name: &str,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.push_and_set_upstream(remote_name, remote_branch_name)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn force_push_current_branch(&self) -> Result<PushGatewayOutcome, GitError> {
        self.with_service(|service| match service.force_push_current_branch()? {
            PushOutcome::Pushed => Ok(PushGatewayOutcome::Pushed(Box::new(
                service.load_repo(COMMIT_LOAD_LIMIT),
            ))),
            PushOutcome::NeedsUpstream {
                branch_name,
                remote_name,
            } => Ok(PushGatewayOutcome::NeedsUpstream {
                branch_name,
                remote_name,
            }),
            PushOutcome::BehindRemote { .. } => {
                unreachable!("force push should not return BehindRemote")
            }
        })
    }

    fn pull_current_branch(&self) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.pull_current_branch()?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn add_remote(
        &self,
        name: &str,
        pull_url: &str,
        push_url: &str,
    ) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.add_remote(name, pull_url, push_url)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }
}

impl StashOps for GitRepositoryGateway {
    fn create_stash(&self) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.create_stash()?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn apply_stash(&self, index: usize) -> Result<StashApplyOutcome, GitError> {
        self.with_service(|service| service.apply_stash(index))
    }

    fn pop_stash(&self, index: usize) -> Result<StashApplyOutcome, GitError> {
        self.with_service(|service| service.pop_stash(index))
    }

    fn drop_stash(&self, index: usize) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.drop_stash(index)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }
}

impl TagOps for GitRepositoryGateway {
    fn create_tag(&self, tag_name: &str, target_hash: &str) -> Result<RepoSnapshot, GitError> {
        self.with_service(|service| {
            service.create_tag(tag_name, target_hash)?;
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn delete_tag(&self, tag_name: &str) -> Result<RepoSnapshot, GitError> {
        let this = self.clone();
        let tag = tag_name.to_string();
        self.with_service(move |service| {
            service.delete_tag(&tag)?;
            this.remove_tag_from_cache(&tag);
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn push_tag(&self, remote_name: &str, tag_name: &str) -> Result<RepoSnapshot, GitError> {
        let this = self.clone();
        let remote = remote_name.to_string();
        let tag = tag_name.to_string();
        self.with_service(move |service| {
            service.push_tag(&remote, &tag)?;
            this.add_tag_remote_to_cache(&tag, &remote);
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn delete_remote_tag(
        &self,
        remote_name: &str,
        tag_name: &str,
    ) -> Result<RepoSnapshot, GitError> {
        let this = self.clone();
        let remote = remote_name.to_string();
        let tag = tag_name.to_string();
        self.with_service(move |service| {
            service.delete_remote_tag(&remote, &tag)?;
            this.remove_tag_remote_from_cache(&tag, &remote);
            Ok(service.load_repo(COMMIT_LOAD_LIMIT))
        })
    }

    fn tag_remotes_for(&self, tag_name: &str) -> Vec<String> {
        self.tag_remotes_snapshot(tag_name)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{BranchOps, RemoteOps, RepoRead};
    use super::GitRepositoryGateway;
    use crate::services::{
        test_support::{
            add_remote, create_branch, init_bare_test_repo, init_test_repo, push_refspec,
        },
        GitService,
    };
    use git2::{BranchType, Repository as Git2Repository};
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread,
        time::Duration,
    };

    fn has_remote_branch(repo: &Git2Repository, branch_name: &str) -> bool {
        repo.find_branch(branch_name, BranchType::Remote).is_ok()
    }

    fn remove_remote_branch(repo: &Git2Repository, branch_name: &str) {
        let full_ref = format!("refs/remotes/{branch_name}");
        if let Ok(mut reference) = repo.find_reference(&full_ref) {
            reference
                .delete()
                .expect("failed to remove setup remote-tracking ref");
        }
    }

    #[test]
    fn gateway_blocks_concurrent_operations_until_active_call_finishes() {
        let (temp_repo, _repo) = init_test_repo("serializes_ops");
        let gateway = GitRepositoryGateway::from_repo_path(temp_repo.path_str());
        let initial_branch = gateway
            .load_repo(10)
            .expect("initial load should succeed")
            .current_branch;

        let guard = gateway
            .operation_lock
            .lock()
            .expect("operation lock should be acquirable");

        let started = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let worker_gateway = gateway.clone();
        let worker_started = started.clone();
        let worker_finished = finished.clone();

        let handle = thread::spawn(move || {
            worker_started.store(true, Ordering::SeqCst);
            let snapshot = worker_gateway
                .load_repo(10)
                .expect("load_repo should succeed once the lock is released");
            assert_eq!(snapshot.current_branch, initial_branch);
            worker_finished.store(true, Ordering::SeqCst);
        });

        thread::sleep(Duration::from_millis(50));

        assert!(started.load(Ordering::SeqCst));
        assert!(
            !finished.load(Ordering::SeqCst),
            "concurrent readers should wait until the active repository call completes"
        );

        drop(guard);
        handle.join().expect("worker thread should finish");
        assert!(finished.load(Ordering::SeqCst));
    }

    #[test]
    fn gateway_checkout_returns_post_checkout_snapshot() {
        let (temp_repo, repo) = init_test_repo("checkout_snapshot");
        let head_commit = repo
            .head()
            .expect("repo should have head")
            .peel_to_commit()
            .expect("head should point to a commit");
        repo.branch("feature", &head_commit, false)
            .expect("failed to create branch");

        let gateway = GitRepositoryGateway::from_repo_path(temp_repo.path_str());
        let snapshot = gateway
            .checkout_branch("feature")
            .expect("checkout should succeed");

        assert_eq!(snapshot.current_branch.as_deref(), Some("feature"));
        let service = GitService::open(temp_repo.path_str()).expect("failed to reopen service");
        assert_eq!(
            service.load_repo(10).current_branch.as_deref(),
            Some("feature")
        );
    }

    #[test]
    fn gateway_fetch_remote_fetches_only_requested_remote() {
        let (temp_repo, repo) = init_test_repo("gateway_fetch_one_remote");
        let (origin_temp, _origin_repo) = init_bare_test_repo("gateway_fetch_one_origin");
        let (backup_temp, _backup_repo) = init_bare_test_repo("gateway_fetch_one_backup");

        add_remote(
            &repo,
            "origin",
            &format!("file://{}", origin_temp.path_str()),
        );
        add_remote(
            &repo,
            "backup",
            &format!("file://{}", backup_temp.path_str()),
        );

        create_branch(&repo, "origin-only");
        push_refspec(
            &repo,
            "origin",
            "refs/heads/origin-only:refs/heads/origin-only",
        );
        create_branch(&repo, "backup-only");
        push_refspec(
            &repo,
            "backup",
            "refs/heads/backup-only:refs/heads/backup-only",
        );
        remove_remote_branch(&repo, "origin/origin-only");
        remove_remote_branch(&repo, "backup/backup-only");

        let gateway = GitRepositoryGateway::from_repo_path(temp_repo.path_str());
        gateway
            .fetch_remote("backup")
            .expect("gateway should fetch selected remote");

        let repo = Git2Repository::open(temp_repo.path_str()).expect("failed to reopen repo");
        assert!(!has_remote_branch(&repo, "origin/origin-only"));
        assert!(has_remote_branch(&repo, "backup/backup-only"));
    }

    /// Compile-time check: a function that takes `&impl BranchOps` must not be
    /// able to call non-BranchOps methods. This is the ISP payoff — the
    /// branch-ops dialogs depend on this trait only.
    fn branch_ops_compile_check<G: BranchOps>(g: &G) {
        let _ = g.abort_merge();
        // The following would fail to compile:
        //   let _ = g.create_stash(); // <- StashOps, not in scope
    }

    #[test]
    fn branch_ops_trait_surface_stays_narrow() {
        let (temp_repo, _repo) = init_test_repo("branch_ops_trait_surface");
        let gateway = GitRepositoryGateway::from_repo_path(temp_repo.path_str());
        branch_ops_compile_check(&gateway);
    }

    #[test]
    fn gateway_list_worktrees_returns_primary() {
        use crate::services::gateway::git_worktree_ops::GitWorktreeOps;
        use crate::services::test_support::init_test_repo;
        let (temp, _repo) = init_test_repo("gateway_list_worktrees");
        let gw = GitRepositoryGateway::from_repo_path(temp.path_str());
        let worktrees = gw.list_worktrees().expect("gateway list");
        assert_eq!(worktrees.len(), 1);
        assert!(worktrees[0].is_primary);
    }
}

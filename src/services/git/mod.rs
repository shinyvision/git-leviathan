mod checkout;
mod cherry_pick;
mod conflict_resolution;
mod diff;
mod fetch;
mod helpers;
mod loader;
mod merge;
mod push;
mod rebase;
mod refs;
mod remotes;
mod reword;
mod squash;
mod stashes;
mod tags;
mod working_tree;
pub mod working_tree_diff;
mod worktrees;

use git2::Repository;

use crate::services::git::working_tree_diff::WorkingTreeDiffResult;
use crate::{core::ChangedFile, services::git_error::GitError, services::RepoSnapshot};

pub use checkout::ResetMode;
pub use conflict_resolution::{ConflictBlock, ConflictResolutionResult};
pub use helpers::kill_running_git_processes;
pub use push::PushOutcome;

pub const COMMIT_LOAD_LIMIT: usize = 500;

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // Success carries RepoSnapshot; boxing would churn gateway call sites.
pub enum RemoteCheckoutOutcome {
    Success(RepoSnapshot),
    LocalAheadOfRemote {
        branch_name: String,
        remote_ref: String,
    },
}

#[derive(Debug, Clone)]
pub enum BranchMergeOutcome {
    Merged(RepoSnapshot),
    Conflicted(RepoSnapshot),
}

#[derive(Debug, Clone)]
pub enum CherryPickOutcome {
    Committed(RepoSnapshot),
    Conflicted(RepoSnapshot),
}

#[derive(Debug, Clone)]
pub enum StashApplyOutcome {
    Applied(RepoSnapshot),
    Conflicted(RepoSnapshot),
}

#[derive(Debug, Clone)]
pub struct CommitDiffResult {
    pub commit_idx: usize,
    pub hash: String,
    pub modified_count: u32,
    pub added_count: u32,
    pub deleted_count: u32,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone)]
pub struct MergedCommitDiffResult {
    pub hashes: Vec<String>,
    pub modified_count: u32,
    pub added_count: u32,
    pub deleted_count: u32,
    pub files: Vec<ChangedFile>,
}

pub fn load_commit_diff(
    repo_path: &str,
    commit_idx: usize,
    hash: &str,
) -> Result<CommitDiffResult, GitError> {
    diff::load_commit_diff(repo_path, commit_idx, hash)
}

pub fn load_merged_commit_diff(
    repo_path: &str,
    hashes: &[String],
) -> Result<MergedCommitDiffResult, GitError> {
    diff::load_merged_commit_diff(repo_path, hashes)
}

pub fn load_merged_commit_file_diff(
    repo_path: &str,
    hashes: &[String],
    file_path: &str,
) -> Result<WorkingTreeDiffResult, GitError> {
    diff::load_merged_commit_file_diff(repo_path, hashes, file_path)
}

impl GitService {
    pub fn load_working_tree_diff(
        &self,
        file_path: &str,
        is_staged: bool,
    ) -> Result<WorkingTreeDiffResult, GitError> {
        working_tree_diff::load_working_tree_diff(self, file_path, is_staged)
    }

    pub fn compute_dirty_file_signature(
        &self,
        file_path: &str,
        is_staged: bool,
    ) -> crate::services::DirtyDiffSignature {
        working_tree_diff::compute_dirty_signature(&self.repo, file_path, is_staged)
    }

    pub fn load_commit_file_diff(
        &self,
        commit_hash: &str,
        file_path: &str,
    ) -> Result<WorkingTreeDiffResult, GitError> {
        working_tree_diff::load_commit_file_diff(self, commit_hash, file_path)
    }

    pub fn load_conflict_resolution(
        &self,
        file_path: &str,
    ) -> Result<ConflictResolutionResult, GitError> {
        conflict_resolution::load_conflict_resolution(self, file_path)
    }
}

pub struct GitService {
    pub(crate) repo: Repository,
}

impl GitService {
    pub fn open(path: &str) -> Result<Self, git2::Error> {
        let repo = Repository::open(path)?;
        Ok(Self { repo })
    }

    pub fn load_repo(&self, commit_limit: usize) -> RepoSnapshot {
        loader::load_repo(self, commit_limit)
    }

    pub fn list_worktrees(&self) -> Result<Vec<crate::core::WorktreeInfo>, GitError> {
        worktrees::list_worktrees(self)
    }

    pub fn add_worktree(
        &self,
        path: &std::path::Path,
        new_branch_name: Option<&str>,
        ref_to_checkout: &str,
    ) -> Result<(), GitError> {
        worktrees::add_worktree(self, path, new_branch_name, ref_to_checkout)
    }

    pub fn remove_worktree(&self, path: &std::path::Path, force: bool) -> Result<(), GitError> {
        worktrees::remove_worktree(self, path, force)
    }

    pub fn load_refs_snapshot(&self, commit_limit: usize) -> crate::services::RefsSnapshot {
        loader::load_refs_snapshot(self, commit_limit)
    }

    pub fn checkout_branch(&mut self, branch_name: &str) -> Result<(), GitError> {
        checkout::checkout_branch(self, branch_name)
    }

    pub fn checkout_remote_branch(
        &mut self,
        branch_name: &str,
    ) -> Result<RemoteCheckoutOutcome, GitError> {
        match checkout::checkout_remote_branch(self, branch_name)? {
            checkout::CheckoutRemoteBranchResult::Success => Ok(RemoteCheckoutOutcome::Success(
                self.load_repo(COMMIT_LOAD_LIMIT),
            )),
            checkout::CheckoutRemoteBranchResult::LocalAheadOfRemote {
                branch_name,
                remote_ref,
            } => Ok(RemoteCheckoutOutcome::LocalAheadOfRemote {
                branch_name,
                remote_ref,
            }),
        }
    }

    pub fn create_branch_from_remote_ref_and_checkout(
        &mut self,
        new_name: &str,
        remote_ref: &str,
    ) -> Result<(), GitError> {
        checkout::create_branch_from_remote_ref_and_checkout(self, new_name, remote_ref)
    }

    pub fn reset_branch_to_remote_and_checkout(
        &mut self,
        branch_name: &str,
        remote_ref: &str,
    ) -> Result<(), GitError> {
        checkout::reset_branch_to_remote_and_checkout(self, branch_name, remote_ref)
    }

    pub fn delete_branch(&mut self, branch_name: &str, is_remote: bool) -> Result<(), GitError> {
        checkout::delete_branch(self, branch_name, is_remote)
    }

    pub fn delete_branch_all(&mut self, branch_name: &str) -> Result<(), GitError> {
        checkout::delete_branch_all(self, branch_name)
    }

    pub fn rename_branch(
        &mut self,
        old_name: &str,
        new_name: &str,
        is_remote: bool,
    ) -> Result<(), GitError> {
        checkout::rename_branch(self, old_name, new_name, is_remote)
    }

    pub fn create_branch_at_commit(
        &mut self,
        branch_name: &str,
        commit_hash: &str,
    ) -> Result<(), GitError> {
        checkout::create_branch_at_commit(self, branch_name, commit_hash)
    }

    pub fn reset_current_branch_to_commit(
        &mut self,
        commit_hash: &str,
        mode: ResetMode,
    ) -> Result<(), GitError> {
        checkout::reset_current_branch_to_commit(self, commit_hash, mode)
    }

    pub fn fast_forward_branch_to_branch(
        &mut self,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<(), GitError> {
        checkout::fast_forward_branch_to_branch(self, source_branch, target_branch)
    }

    pub fn merge_branch_into(
        &mut self,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<BranchMergeOutcome, GitError> {
        match merge::merge_branch_into(self, source_branch, target_branch)? {
            merge::BranchMergeStatus::Merged => Ok(BranchMergeOutcome::Merged(
                self.load_repo(COMMIT_LOAD_LIMIT),
            )),
            merge::BranchMergeStatus::Conflicted => Ok(BranchMergeOutcome::Conflicted(
                self.load_repo(COMMIT_LOAD_LIMIT),
            )),
        }
    }

    pub fn abort_merge(&mut self) -> Result<(), GitError> {
        merge::abort_merge(self)
    }

    pub fn rebase_current_onto(
        &mut self,
        source_branch: &str,
        target_ref: &str,
    ) -> Result<(), GitError> {
        rebase::rebase_current_onto(self, source_branch, target_ref)
    }

    pub fn stage_file(&mut self, path: &str) -> Result<(), GitError> {
        working_tree::stage_file(self, path)
    }

    pub fn stage_all_dirty_changes(&mut self) -> Result<(), GitError> {
        working_tree::stage_all_dirty_changes(self)
    }

    pub fn unstage_file(&mut self, path: &str) -> Result<(), GitError> {
        working_tree::unstage_file(self, path)
    }

    pub fn unstage_all_dirty_changes(&mut self) -> Result<(), GitError> {
        working_tree::unstage_all_dirty_changes(self)
    }

    pub fn mark_conflict_resolved(&mut self, path: &str) -> Result<(), GitError> {
        working_tree::mark_conflict_resolved(self, path)
    }

    pub fn discard_file(&mut self, path: &str) -> Result<(), GitError> {
        working_tree::discard_file(self, path)
    }

    pub fn discard_all_dirty_changes(&mut self) -> Result<(), GitError> {
        working_tree::discard_all_dirty_changes(self)
    }

    pub fn mark_all_conflicts_resolved(&mut self) -> Result<(), GitError> {
        working_tree::mark_all_conflicts_resolved(self)
    }

    pub fn save_conflict_resolution(
        &mut self,
        file_path: &str,
        content: &str,
    ) -> Result<(), GitError> {
        conflict_resolution::save_conflict_resolution(self, file_path, content)
    }

    pub fn commit_dirty_changes(
        &mut self,
        summary: &str,
        description: &str,
    ) -> Result<(), GitError> {
        working_tree::commit_dirty_changes(self, summary, description)
    }

    pub fn fetch_all_refs(&mut self) -> Result<(), GitError> {
        fetch::fetch_all_refs(self)
    }

    pub fn push_current_branch(&mut self) -> Result<PushOutcome, GitError> {
        push::push_current_branch(self)
    }

    pub fn push_and_set_upstream(
        &mut self,
        remote_name: &str,
        remote_branch_name: &str,
    ) -> Result<(), GitError> {
        push::push_and_set_upstream(self, remote_name, remote_branch_name)
    }

    pub fn force_push_current_branch(&mut self) -> Result<PushOutcome, GitError> {
        push::force_push_current_branch(self)
    }

    pub fn pull_current_branch(&mut self) -> Result<(), GitError> {
        push::pull_current_branch(self)
    }

    pub fn add_remote(
        &mut self,
        name: &str,
        pull_url: &str,
        push_url: &str,
    ) -> Result<(), GitError> {
        remotes::add_remote(self, name, pull_url, push_url)
    }

    pub fn create_stash(&mut self) -> Result<(), GitError> {
        stashes::create_stash(self)
    }

    pub fn apply_stash(&mut self, index: usize) -> Result<StashApplyOutcome, GitError> {
        let status = stashes::apply_stash(self, index)?;
        let snapshot = self.load_repo(COMMIT_LOAD_LIMIT);
        Ok(match status {
            stashes::StashApplyStatus::Applied => StashApplyOutcome::Applied(snapshot),
            stashes::StashApplyStatus::Conflicted => StashApplyOutcome::Conflicted(snapshot),
        })
    }

    pub fn pop_stash(&mut self, index: usize) -> Result<StashApplyOutcome, GitError> {
        let status = stashes::pop_stash(self, index)?;
        let snapshot = self.load_repo(COMMIT_LOAD_LIMIT);
        Ok(match status {
            stashes::StashApplyStatus::Applied => StashApplyOutcome::Applied(snapshot),
            stashes::StashApplyStatus::Conflicted => StashApplyOutcome::Conflicted(snapshot),
        })
    }

    pub fn drop_stash(&mut self, index: usize) -> Result<(), GitError> {
        stashes::drop_stash(self, index)
    }

    pub(crate) fn current_branch_name(&self) -> Option<String> {
        checkout::current_branch_name(self)
    }

    pub fn squash_commits(&mut self, hashes: Vec<String>) -> Result<(), GitError> {
        squash::squash_commits(self, hashes)
    }

    pub fn cherry_pick_commit(
        &mut self,
        commit_hash: &str,
        immediate_commit: bool,
    ) -> Result<CherryPickOutcome, GitError> {
        let status = cherry_pick::cherry_pick_commit(self, commit_hash, immediate_commit)?;
        let snapshot = self.load_repo(COMMIT_LOAD_LIMIT);
        Ok(match status {
            cherry_pick::CherryPickStatus::Committed => CherryPickOutcome::Committed(snapshot),
            cherry_pick::CherryPickStatus::Conflicted => CherryPickOutcome::Conflicted(snapshot),
        })
    }

    pub fn reword_commit(&mut self, hash: &str, new_message: &str) -> Result<(), GitError> {
        reword::reword_commit(self, hash, new_message)
    }

    pub fn create_tag(&mut self, tag_name: &str, target_hash: &str) -> Result<(), GitError> {
        tags::create_tag(self, tag_name, target_hash)
    }

    pub fn delete_tag(&mut self, tag_name: &str) -> Result<(), GitError> {
        tags::delete_tag(self, tag_name)
    }

    pub fn push_tag(&mut self, remote_name: &str, tag_name: &str) -> Result<(), GitError> {
        tags::push_tag(self, remote_name, tag_name)
    }

    pub fn delete_remote_tag(&mut self, remote_name: &str, tag_name: &str) -> Result<(), GitError> {
        tags::delete_remote_tag(self, remote_name, tag_name)
    }

    pub fn list_remote_tags(&self, remote_name: &str) -> Result<Vec<String>, GitError> {
        tags::list_remote_tags(self, remote_name)
    }

    pub fn list_remotes(&self) -> Vec<String> {
        self.repo
            .remotes()
            .ok()
            .map(|r| r.iter().flatten().map(|s| s.to_string()).collect())
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn workdir_has_changes(&self) -> Result<bool, GitError> {
        checkout::workdir_has_changes(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::{
        add_remote, checkout_branch_for_setup, commit_all, create_branch,
        create_remote_tracking_branch, fetch_remote, init_bare_test_repo, init_test_repo,
        push_refspec, stash_names, write_file,
    };
    use std::{fs, path::Path};

    fn configure_signature(repo: &Repository) {
        let mut config = repo.config().expect("failed to open repo config");
        config
            .set_str("user.name", "Git Leviathan Test")
            .expect("failed to set user.name");
        config
            .set_str("user.email", "test@example.invalid")
            .expect("failed to set user.email");
    }

    #[test]
    fn delete_branch_removes_local_branch_ref() {
        let (temp_repo, repo) = init_test_repo("delete_local_branch");
        create_branch(&repo, "feature/delete-me");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .delete_branch("feature/delete-me", false)
            .expect("delete should succeed");

        assert!(repo
            .find_branch("feature/delete-me", git2::BranchType::Local)
            .is_err());
    }

    #[test]
    fn delete_branch_rejects_checked_out_local_branch() {
        let (temp_repo, repo) = init_test_repo("delete_current_branch");
        create_branch(&repo, "feature/current");
        checkout_branch_for_setup(&repo, "feature/current");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let err = service
            .delete_branch("feature/current", false)
            .expect_err("current branch delete should fail");

        assert!(err.is_cant_delete_head());
        assert!(repo
            .find_branch("feature/current", git2::BranchType::Local)
            .is_ok());
    }

    #[test]
    fn delete_branch_removes_remote_tracking_branch_ref() {
        let (temp_repo, repo) = init_test_repo("delete_remote_branch");
        create_remote_tracking_branch(&repo, "origin/feature/remote-only");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .delete_branch("feature/remote-only", true)
            .expect("delete should succeed");

        assert!(repo
            .find_branch("origin/feature/remote-only", git2::BranchType::Remote)
            .is_err());
    }

    #[test]
    fn delete_branch_removes_remote_branch_from_configured_remote() {
        let (temp_repo, repo) = init_test_repo("delete_remote_branch_push");
        let (remote_temp, _remote_repo) = init_bare_test_repo("delete_remote_branch_push_origin");
        let remote_url = format!("file://{}", remote_temp.path_str());
        let current_branch = repo
            .head()
            .expect("repo should have head")
            .shorthand()
            .expect("head branch should be utf-8")
            .to_string();

        add_remote(&repo, "origin", &remote_url);

        create_branch(&repo, "feature/remote-only");
        checkout_branch_for_setup(&repo, "feature/remote-only");
        write_file(&temp_repo.path, "tracked.txt", "remote-only branch\n");
        commit_all(&repo, "remote-only branch");
        push_refspec(
            &repo,
            "origin",
            "refs/heads/feature/remote-only:refs/heads/feature/remote-only",
        );

        checkout_branch_for_setup(&repo, &current_branch);
        repo.find_branch("feature/remote-only", git2::BranchType::Local)
            .expect("local feature branch should exist")
            .delete()
            .expect("failed to delete local feature branch during setup");
        fetch_remote(&repo, "origin");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .delete_branch("feature/remote-only", true)
            .expect("remote delete should succeed");

        let repo = Repository::open(temp_repo.path_str()).expect("failed to reopen repo");
        assert!(repo
            .find_branch("origin/feature/remote-only", git2::BranchType::Remote)
            .is_err());

        let remote_repo =
            Repository::open_bare(remote_temp.path_str()).expect("failed to reopen bare remote");
        assert!(remote_repo
            .find_reference("refs/heads/feature/remote-only")
            .is_err());
    }

    #[test]
    fn delete_branch_all_removes_local_and_remote_branch_from_configured_remote() {
        let (temp_repo, repo) = init_test_repo("delete_branch_all_push");
        let (remote_temp, _remote_repo) = init_bare_test_repo("delete_branch_all_push_origin");
        let remote_url = format!("file://{}", remote_temp.path_str());

        add_remote(&repo, "origin", &remote_url);
        create_branch(&repo, "feature/shared");
        push_refspec(
            &repo,
            "origin",
            "refs/heads/feature/shared:refs/heads/feature/shared",
        );
        fetch_remote(&repo, "origin");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .delete_branch_all("feature/shared")
            .expect("delete all should succeed");

        let repo = Repository::open(temp_repo.path_str()).expect("failed to reopen repo");
        assert!(repo
            .find_branch("feature/shared", git2::BranchType::Local)
            .is_err());
        assert!(repo
            .find_branch("origin/feature/shared", git2::BranchType::Remote)
            .is_err());

        let remote_repo =
            Repository::open_bare(remote_temp.path_str()).expect("failed to reopen bare remote");
        assert!(remote_repo
            .find_reference("refs/heads/feature/shared")
            .is_err());
    }

    #[test]
    fn checkout_branch_restores_stashed_changes_when_pop_succeeds() {
        let (temp_repo, repo) = init_test_repo("checkout_clean");
        create_branch(&repo, "feature");
        write_file(&temp_repo.path, "tracked.txt", "work in progress\n");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .checkout_branch("feature")
            .expect("checkout with clean stash apply should succeed");

        assert_eq!(service.current_branch_name().as_deref(), Some("feature"));
        assert_eq!(
            fs::read_to_string(temp_repo.path.join("tracked.txt"))
                .expect("failed to read tracked file"),
            "work in progress\n"
        );

        let mut repo = Repository::open(temp_repo.path_str()).expect("failed to reopen repo");
        assert!(stash_names(&mut repo).is_empty());
    }

    #[test]
    fn checkout_branch_keeps_stash_when_pop_conflicts() {
        let (temp_repo, repo) = init_test_repo("checkout_conflict");
        let old_branch = repo
            .head()
            .expect("repo should have head")
            .shorthand()
            .expect("head branch should be utf-8")
            .to_string();

        create_branch(&repo, "feature");

        // Multi-line file where both branches modify the same middle line to
        // different values. git2's line-level merge cannot resolve this and will
        // return GIT_EAPPLYFAIL, so we can verify the cleanup path.
        checkout_branch_for_setup(&repo, "feature");
        write_file(&temp_repo.path, "tracked.txt", "context\nalpha\nmore\n");
        commit_all(&repo, "feature change");

        checkout_branch_for_setup(&repo, &old_branch);
        write_file(&temp_repo.path, "tracked.txt", "context\nbeta\nmore\n");
        commit_all(&repo, "main change");
        // User WIP: same middle line, third distinct value → guaranteed conflict
        write_file(&temp_repo.path, "tracked.txt", "context\ngamma\nmore\n");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .checkout_branch("feature")
            .expect("checkout should still succeed when stash pop conflicts");

        assert_eq!(service.current_branch_name().as_deref(), Some("feature"));

        let mut repo = Repository::open(temp_repo.path_str()).expect("failed to reopen repo");
        let stash_names = stash_names(&mut repo);
        assert_eq!(stash_names.len(), 1);
        assert!(stash_names[0].starts_with(&format!("WIP on {}:", old_branch)));

        // After a failed stash pop the working tree must be clean — no conflict
        // markers or A/B diff mixed in. restore_stash force-checks out HEAD after
        // a failed pop to restore the target branch's committed state.
        assert!(
            !service
                .workdir_has_changes()
                .expect("failed to inspect status"),
            "failed stash pop must not leave the working tree dirty"
        );
        assert_eq!(
            fs::read_to_string(temp_repo.path.join("tracked.txt"))
                .expect("failed to read tracked file"),
            "context\nalpha\nmore\n",
            "tracked.txt should be the feature branch's committed version, not conflict-marked"
        );
    }

    #[test]
    fn checkout_branch_keeps_worktree_clean_when_switching_clean_branches() {
        let (temp_repo, repo) = init_test_repo("checkout_no_wip");
        let old_branch = repo
            .head()
            .expect("repo should have head")
            .shorthand()
            .expect("head branch should be utf-8")
            .to_string();

        create_branch(&repo, "feature");
        checkout_branch_for_setup(&repo, "feature");
        write_file(&temp_repo.path, "tracked.txt", "feature committed\n");
        commit_all(&repo, "feature change");

        checkout_branch_for_setup(&repo, &old_branch);
        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .checkout_branch("feature")
            .expect("clean checkout should succeed");

        assert_eq!(service.current_branch_name().as_deref(), Some("feature"));
        assert_eq!(
            fs::read_to_string(temp_repo.path.join("tracked.txt"))
                .expect("failed to read tracked file"),
            "feature committed\n"
        );
        assert!(
            !service
                .workdir_has_changes()
                .expect("failed to inspect status"),
            "switching branches with a clean worktree should not create unstaged changes"
        );
    }

    #[test]
    fn checkout_remote_branch_fast_forwards_current_branch_and_keeps_worktree_clean() {
        let (temp_repo, repo) = init_test_repo("checkout_remote_fast_forward");
        let current_branch = repo
            .head()
            .expect("repo should have head")
            .shorthand()
            .expect("head branch should be utf-8")
            .to_string();
        let remote_ref_name = format!("origin/{}", current_branch);

        create_remote_tracking_branch(&repo, &remote_ref_name);
        create_branch(&repo, "remote-source");
        checkout_branch_for_setup(&repo, "remote-source");
        write_file(&temp_repo.path, "tracked.txt", "remote committed\n");
        let remote_oid = commit_all(&repo, "remote change");
        repo.reference(
            &format!("refs/remotes/{}", remote_ref_name),
            remote_oid,
            true,
            "advance remote branch",
        )
        .expect("failed to advance remote tracking ref");

        checkout_branch_for_setup(&repo, &current_branch);

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let outcome = service
            .checkout_remote_branch(&current_branch)
            .expect("remote checkout should succeed");

        assert!(matches!(outcome, RemoteCheckoutOutcome::Success(_)));
        assert_eq!(
            service.current_branch_name().as_deref(),
            Some(current_branch.as_str())
        );
        assert_eq!(
            fs::read_to_string(temp_repo.path.join("tracked.txt"))
                .expect("failed to read tracked file"),
            "remote committed\n"
        );
        assert!(
            !service
                .workdir_has_changes()
                .expect("failed to inspect status"),
            "fast-forward checkout should leave the worktree clean"
        );
    }

    #[test]
    fn merge_branch_into_checks_out_target_and_creates_merge_commit() {
        let (temp_repo, repo) = init_test_repo("merge_branch_clean");
        configure_signature(&repo);

        create_branch(&repo, "develop");
        create_branch(&repo, "feature/add-foo");

        checkout_branch_for_setup(&repo, "feature/add-foo");
        write_file(&temp_repo.path, "foo.txt", "foo\n");
        commit_all(&repo, "add foo");

        checkout_branch_for_setup(&repo, "develop");
        write_file(&temp_repo.path, "develop.txt", "develop\n");
        commit_all(&repo, "develop work");

        checkout_branch_for_setup(&repo, "feature/add-foo");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let outcome = service
            .merge_branch_into("develop", "feature/add-foo")
            .expect("merge should succeed");

        assert!(matches!(outcome, BranchMergeOutcome::Merged(_)));
        assert_eq!(
            service.current_branch_name().as_deref(),
            Some("feature/add-foo")
        );

        let head = service
            .repo
            .head()
            .expect("repo should have head")
            .peel_to_commit()
            .expect("head should point to a commit");
        assert_eq!(
            head.summary(),
            Some("Merge branch 'develop' into feature/add-foo")
        );
        assert_eq!(head.parent_count(), 2);
    }

    #[test]
    fn merge_branch_into_leaves_conflicts_on_checked_out_branch() {
        let (temp_repo, repo) = init_test_repo("merge_branch_conflict");
        configure_signature(&repo);

        create_branch(&repo, "develop");
        create_branch(&repo, "feature/add-foo");

        checkout_branch_for_setup(&repo, "feature/add-foo");
        write_file(&temp_repo.path, "tracked.txt", "feature\n");
        commit_all(&repo, "feature edits tracked");

        checkout_branch_for_setup(&repo, "develop");
        write_file(&temp_repo.path, "tracked.txt", "develop\n");
        commit_all(&repo, "develop edits tracked");

        checkout_branch_for_setup(&repo, "feature/add-foo");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let outcome = service
            .merge_branch_into("develop", "feature/add-foo")
            .expect("merge should report conflict as an outcome");

        assert!(matches!(outcome, BranchMergeOutcome::Conflicted(_)));
        assert_eq!(
            service.current_branch_name().as_deref(),
            Some("feature/add-foo")
        );
        assert_eq!(service.repo.state(), git2::RepositoryState::Merge);

        let dirty = service
            .load_repo(10)
            .dirty
            .expect("conflicted merge should be dirty");
        assert_eq!(dirty.conflicted_files.len(), 1);
        assert_eq!(dirty.conflicted_files[0].path, "tracked.txt");
        let head_hash = service
            .repo
            .head()
            .expect("repo should have head")
            .target()
            .expect("head should point to an oid")
            .to_string();
        let merge_head = fs::read_to_string(service.repo.path().join("MERGE_HEAD"))
            .expect("merge head should exist")
            .trim()
            .to_string();
        assert_eq!(dirty.parent_hashes[0], head_hash);
        assert!(dirty.parent_hashes.iter().any(|hash| hash == &merge_head));
        let resolution = service
            .load_conflict_resolution("tracked.txt")
            .expect("conflict resolution should load");
        assert_eq!(resolution.blocks.len(), 1);
        assert_eq!(resolution.ours_label, "HEAD");
        assert!(resolution.theirs_label.contains("develop"));

        service
            .save_conflict_resolution("tracked.txt", "resolved\n")
            .expect("saving resolution should stage the file");

        let dirty = service
            .load_repo(10)
            .dirty
            .expect("resolved merge should still have staged changes");
        assert!(dirty.conflicted_files.is_empty());
        assert!(dirty
            .staged_files
            .iter()
            .any(|file| file.path == "tracked.txt"));

        service
            .commit_dirty_changes("Merge branch 'develop' into feature/add-foo", "")
            .expect("resolved merge commit should succeed");
        assert_eq!(service.repo.state(), git2::RepositoryState::Clean);

        let head = service
            .repo
            .head()
            .expect("repo should have head")
            .peel_to_commit()
            .expect("head should point to a commit");
        assert_eq!(head.parent_count(), 2);
    }

    #[test]
    fn abort_merge_cleans_conflicted_merge_state() {
        let (temp_repo, repo) = init_test_repo("abort_merge_conflict");
        configure_signature(&repo);

        create_branch(&repo, "develop");
        create_branch(&repo, "feature/add-foo");

        checkout_branch_for_setup(&repo, "feature/add-foo");
        write_file(&temp_repo.path, "tracked.txt", "feature\n");
        commit_all(&repo, "feature edits tracked");

        checkout_branch_for_setup(&repo, "develop");
        write_file(&temp_repo.path, "tracked.txt", "develop\n");
        commit_all(&repo, "develop edits tracked");

        checkout_branch_for_setup(&repo, "feature/add-foo");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let outcome = service
            .merge_branch_into("develop", "feature/add-foo")
            .expect("merge should report conflict as an outcome");
        assert!(matches!(outcome, BranchMergeOutcome::Conflicted(_)));
        assert_eq!(service.repo.state(), git2::RepositoryState::Merge);

        service.abort_merge().expect("abort merge should succeed");

        assert_eq!(service.repo.state(), git2::RepositoryState::Clean);
        assert!(
            service.load_repo(10).dirty.is_none(),
            "aborted merge should leave no dirty snapshot"
        );
        assert_eq!(
            service.current_branch_name().as_deref(),
            Some("feature/add-foo")
        );
        assert_eq!(
            fs::read_to_string(temp_repo.path.join("tracked.txt"))
                .expect("failed to read tracked file"),
            "feature\n"
        );
    }

    #[test]
    fn load_repo_reports_when_more_commits_exist_beyond_limit() {
        let (temp_repo, repo) = init_test_repo("load_repo_paged");
        write_file(&temp_repo.path, "tracked.txt", "second\n");
        commit_all(&repo, "second");
        write_file(&temp_repo.path, "tracked.txt", "third\n");
        commit_all(&repo, "third");

        let service = GitService::open(temp_repo.path_str()).expect("failed to open service");

        let partial = service.load_repo(2);
        assert_eq!(partial.commits.len(), 2);
        assert!(partial.has_more_commits);

        let full = service.load_repo(10);
        assert_eq!(full.commits.len(), 3);
        assert!(!full.has_more_commits);
    }

    #[test]
    fn load_repo_reports_staged_and_unstaged_dirty_files() {
        let (temp_repo, repo) = init_test_repo("load_repo_dirty");
        write_file(&temp_repo.path, "staged.txt", "staged\n");
        let mut index = repo.index().expect("failed to open index");
        index
            .add_path(Path::new("staged.txt"))
            .expect("failed to stage file");
        index.write().expect("failed to write index");

        write_file(&temp_repo.path, "tracked.txt", "modified\n");
        write_file(&temp_repo.path, "untracked.txt", "untracked\n");

        let service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let snapshot = service.load_repo(10);
        let dirty = snapshot.dirty.expect("dirty snapshot should be present");

        assert_eq!(dirty.staged_files.len(), 1);
        assert_eq!(dirty.staged_files[0].path, "staged.txt");
        assert!(dirty
            .unstaged_files
            .iter()
            .any(|file| file.path == "tracked.txt"));
        assert!(dirty
            .unstaged_files
            .iter()
            .any(|file| file.path == "untracked.txt"));
    }

    #[test]
    fn stage_file_moves_only_the_selected_dirty_file_into_the_index() {
        let (temp_repo, _repo) = init_test_repo("stage_file");
        write_file(&temp_repo.path, "selected.txt", "selected\n");
        write_file(&temp_repo.path, "kept.txt", "kept\n");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .stage_file("selected.txt")
            .expect("staging one file should succeed");

        let snapshot = service.load_repo(10);
        let dirty = snapshot.dirty.expect("dirty snapshot should be present");

        assert!(dirty
            .staged_files
            .iter()
            .any(|file| file.path == "selected.txt"));
        assert!(dirty
            .unstaged_files
            .iter()
            .any(|file| file.path == "kept.txt"));
        assert!(!dirty
            .unstaged_files
            .iter()
            .any(|file| file.path == "selected.txt"));
    }

    #[test]
    fn stage_all_dirty_changes_moves_every_dirty_file_into_the_index() {
        let (temp_repo, _repo) = init_test_repo("stage_all_dirty");
        write_file(&temp_repo.path, "tracked.txt", "modified\n");
        write_file(&temp_repo.path, "untracked.txt", "new\n");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .stage_all_dirty_changes()
            .expect("staging all changes should succeed");

        let snapshot = service.load_repo(10);
        let dirty = snapshot.dirty.expect("dirty snapshot should be present");

        assert!(dirty.unstaged_files.is_empty());
        assert!(dirty
            .staged_files
            .iter()
            .any(|file| file.path == "tracked.txt"));
        assert!(dirty
            .staged_files
            .iter()
            .any(|file| file.path == "untracked.txt"));
    }

    #[test]
    fn unstage_file_moves_only_the_selected_staged_file_out_of_the_index() {
        let (temp_repo, _repo) = init_test_repo("unstage_file");
        write_file(&temp_repo.path, "selected.txt", "selected\n");
        write_file(&temp_repo.path, "kept.txt", "kept\n");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .stage_all_dirty_changes()
            .expect("staging all changes should succeed");
        service
            .unstage_file("selected.txt")
            .expect("unstaging one file should succeed");

        let snapshot = service.load_repo(10);
        let dirty = snapshot.dirty.expect("dirty snapshot should be present");

        assert!(dirty
            .staged_files
            .iter()
            .any(|file| file.path == "kept.txt"));
        assert!(!dirty
            .staged_files
            .iter()
            .any(|file| file.path == "selected.txt"));
        assert!(dirty
            .unstaged_files
            .iter()
            .any(|file| file.path == "selected.txt"));
    }

    #[test]
    fn unstage_all_dirty_changes_moves_every_staged_file_out_of_the_index() {
        let (temp_repo, _repo) = init_test_repo("unstage_all_dirty");
        write_file(&temp_repo.path, "tracked.txt", "modified\n");
        write_file(&temp_repo.path, "untracked.txt", "new\n");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .stage_all_dirty_changes()
            .expect("staging all changes should succeed");
        service
            .unstage_all_dirty_changes()
            .expect("unstaging all changes should succeed");

        let snapshot = service.load_repo(10);
        let dirty = snapshot.dirty.expect("dirty snapshot should be present");

        assert!(dirty.staged_files.is_empty());
        assert!(dirty
            .unstaged_files
            .iter()
            .any(|file| file.path == "tracked.txt"));
        assert!(dirty
            .unstaged_files
            .iter()
            .any(|file| file.path == "untracked.txt"));
    }

    #[test]
    fn commit_dirty_changes_requires_staged_files() {
        let (temp_repo, repo) = init_test_repo("commit_dirty_requires_staged");
        configure_signature(&repo);
        write_file(&temp_repo.path, "tracked.txt", "modified\n");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        let err = service
            .commit_dirty_changes("commit dirty", "details")
            .expect_err("commit should require staged files");

        assert_eq!(
            err.to_string(),
            "there are no staged changes to commit".to_string()
        );
    }

    #[test]
    fn commit_dirty_changes_commits_only_staged_changes() {
        let (temp_repo, repo) = init_test_repo("commit_dirty");
        configure_signature(&repo);
        write_file(&temp_repo.path, "staged.txt", "staged\n");
        write_file(&temp_repo.path, "untracked.txt", "untracked\n");
        write_file(&temp_repo.path, "tracked.txt", "modified\n");

        let mut service = GitService::open(temp_repo.path_str()).expect("failed to open service");
        service
            .stage_file("staged.txt")
            .expect("staging one file should succeed");
        service
            .commit_dirty_changes("commit dirty", "details")
            .expect("commit should succeed");

        let head = service
            .repo
            .head()
            .expect("repo should have head")
            .peel_to_commit()
            .expect("head should point to a commit");
        assert_eq!(head.summary(), Some("commit dirty"));
        assert_eq!(head.body(), Some("details"));
        let tree = head.tree().expect("head should have a tree");
        assert!(tree.get_path(Path::new("staged.txt")).is_ok());
        assert!(tree.get_path(Path::new("untracked.txt")).is_err());

        let dirty = service
            .load_repo(10)
            .dirty
            .expect("unstaged changes should remain after committing staged files");
        assert!(dirty.staged_files.is_empty());
        assert!(dirty
            .unstaged_files
            .iter()
            .any(|file| file.path == "tracked.txt"));
        assert!(dirty
            .unstaged_files
            .iter()
            .any(|file| file.path == "untracked.txt"));
    }
}

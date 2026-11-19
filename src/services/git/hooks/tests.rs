use std::os::unix::fs::PermissionsExt;

use git2::{Oid, Repository};

use super::*;
use crate::services::git::GitService;
use crate::services::test_support::{
    checkout_branch_for_setup, commit_all, configure_test_signature, create_branch, init_test_repo,
    install_logging_hook, read_hook_log, write_file, TempRepo,
};

fn current_branch(repo: &Repository) -> String {
    repo.head()
        .expect("repo should have head")
        .shorthand()
        .expect("head should be utf-8")
        .to_string()
}

/// A repository whose `feature` branch is one commit ahead of the default
/// branch, with the default branch checked out.
fn repo_with_feature_branch(prefix: &str) -> (TempRepo, Repository, Oid, Oid) {
    let (temp, repo) = init_test_repo(prefix);
    configure_test_signature(&repo);
    let main = current_branch(&repo);
    let base = head_commit(&repo);
    create_branch(&repo, "feature");
    checkout_branch_for_setup(&repo, "feature");
    write_file(&temp.path, "feature.txt", "feature\n");
    let feature = commit_all(&repo, "feature work");
    checkout_branch_for_setup(&repo, &main);
    (temp, repo, base, feature)
}

#[test]
fn runs_hook_with_git_arguments_and_reports_output() {
    let (_temp, repo) = init_test_repo("hooks_post_checkout");
    let head = head_commit(&repo);
    install_logging_hook(&repo, "post-checkout", "switched", 0);

    let hook = PostHook::Checkout {
        previous: Oid::zero(),
        current: head,
        branch_checkout: true,
    };
    let report = execute(&repo, &hook).expect("hook should run");

    assert!(report.succeeded);
    assert_eq!(report.output, "switched");
    assert_eq!(
        read_hook_log(&repo),
        format!("post-checkout {} {head} 1\n", Oid::zero())
    );
}

#[test]
fn failing_hook_is_reported() {
    let (_temp, repo) = init_test_repo("hooks_failing");
    install_logging_hook(&repo, "post-merge", "broken", 3);

    let report = execute(&repo, &PostHook::Merge { squash: false }).unwrap();

    assert!(!report.succeeded);
    assert_eq!(report.output, "broken");
}

#[test]
fn skips_missing_and_non_executable_hooks() {
    let (_temp, repo) = init_test_repo("hooks_missing");
    assert_eq!(execute(&repo, &PostHook::Merge { squash: false }), None);

    let path = hooks_dir(&repo).join("post-merge");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(execute(&repo, &PostHook::Merge { squash: false }), None);
}

#[test]
fn respects_core_hooks_path_relative_to_worktree() {
    let (_temp, repo) = init_test_repo("hooks_hooks_path");
    repo.config()
        .unwrap()
        .set_str("core.hooksPath", ".githooks")
        .unwrap();
    let workdir = repo.workdir().unwrap().to_path_buf();
    assert_eq!(hooks_dir(&repo), workdir.join(".githooks"));
}

#[test]
fn strips_terminal_colours() {
    assert_eq!(
        strip_ansi("\u{1b}[1;32mhardy:\u{1b}[0m done"),
        "hardy: done"
    );
}

// The operations below go through libgit2, so they must run the hooks git
// would have run.

#[test]
fn checkout_runs_post_checkout() {
    let (temp, repo, base, feature) = repo_with_feature_branch("hooks_checkout");
    install_logging_hook(&repo, "post-checkout", "", 0);
    let mut service = GitService::open(temp.path_str()).unwrap();

    service.checkout_branch("feature").unwrap();

    assert_eq!(
        read_hook_log(&repo),
        format!("post-checkout {base} {feature} 1\n")
    );
}

#[test]
fn checkout_of_current_branch_runs_no_hook() {
    let (temp, repo, _, _) = repo_with_feature_branch("hooks_checkout_current");
    install_logging_hook(&repo, "post-checkout", "", 0);
    let mut service = GitService::open(temp.path_str()).unwrap();

    service.checkout_branch(&current_branch(&repo)).unwrap();

    assert_eq!(read_hook_log(&repo), "");
}

#[test]
fn failing_post_checkout_does_not_fail_the_checkout() {
    let (temp, repo, _, _) = repo_with_feature_branch("hooks_checkout_failing");
    install_logging_hook(&repo, "post-checkout", "nope", 1);
    let mut service = GitService::open(temp.path_str()).unwrap();

    service.checkout_branch("feature").unwrap();

    assert_eq!(current_branch(&repo), "feature");
}

#[test]
fn fast_forward_of_current_branch_runs_post_merge() {
    let (temp, repo, _, _) = repo_with_feature_branch("hooks_fast_forward");
    install_logging_hook(&repo, "post-merge", "", 0);
    let main = current_branch(&repo);
    let mut service = GitService::open(temp.path_str()).unwrap();

    service
        .fast_forward_branch_to_branch(&main, "feature")
        .unwrap();

    assert_eq!(read_hook_log(&repo), "post-merge 0\n");
}

#[test]
fn rewording_head_runs_post_rewrite_amend() {
    let (temp, repo, _, _) = repo_with_feature_branch("hooks_reword");
    let old = head_commit(&repo);
    install_logging_hook(&repo, "post-rewrite", "", 0);
    let mut service = GitService::open(temp.path_str()).unwrap();

    service
        .reword_commit(&old.to_string(), "Better message")
        .unwrap();

    let new = head_commit(&repo);
    assert_ne!(old, new);
    assert_eq!(
        read_hook_log(&repo),
        format!("post-rewrite amend\n{old} {new}\n")
    );
}

#[test]
fn squash_runs_post_rewrite_rebase_with_every_squashed_commit() {
    let (temp, repo) = init_test_repo("hooks_squash");
    configure_test_signature(&repo);
    write_file(&temp.path, "a.txt", "a\n");
    let older = commit_all(&repo, "older");
    write_file(&temp.path, "b.txt", "b\n");
    let newer = commit_all(&repo, "newer");
    install_logging_hook(&repo, "post-rewrite", "", 0);
    let mut service = GitService::open(temp.path_str()).unwrap();

    service
        .squash_commits(vec![newer.to_string(), older.to_string()])
        .unwrap();

    let squashed = head_commit(&repo);
    assert_eq!(
        read_hook_log(&repo),
        format!("post-rewrite rebase\n{older} {squashed}\n{newer} {squashed}\n")
    );
}

#[test]
fn adding_a_worktree_runs_post_checkout_from_the_null_commit() {
    let (temp, repo) = init_test_repo("hooks_worktree");
    configure_test_signature(&repo);
    let head = head_commit(&repo);
    install_logging_hook(&repo, "post-checkout", "", 0);
    let service = GitService::open(temp.path_str()).unwrap();
    let worktree = temp.path.with_extension("wt");

    service
        .add_worktree(&worktree, Some("wt-branch"), &current_branch(&repo))
        .unwrap();

    assert_eq!(
        read_hook_log(&repo),
        format!("post-checkout {} {head} 1\n", Oid::zero())
    );
    let _ = std::fs::remove_dir_all(&worktree);
}

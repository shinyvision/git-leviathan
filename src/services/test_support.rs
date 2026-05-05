use git2::{build::CheckoutBuilder, IndexAddOption, Repository, RepositoryInitOptions};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) struct TempRepo {
    pub(crate) path: PathBuf,
}

impl TempRepo {
    pub(crate) fn path_str(&self) -> &str {
        self.path.to_str().expect("temp repo path should be utf-8")
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn unique_temp_repo_path(prefix: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    path.push(format!("git_leviathan_{}_{}", prefix, nonce));
    path
}

pub(crate) fn init_test_repo(prefix: &str) -> (TempRepo, Repository) {
    let path = unique_temp_repo_path(prefix);
    fs::create_dir_all(&path).expect("failed to create temp repo dir");

    let repo = Repository::init(&path).expect("failed to init temp repo");
    write_file(&path, "tracked.txt", "base\n");
    commit_all(&repo, "base");

    (TempRepo { path }, repo)
}

pub(crate) fn init_bare_test_repo(prefix: &str) -> (TempRepo, Repository) {
    let path = unique_temp_repo_path(prefix);
    fs::create_dir_all(&path).expect("failed to create temp repo dir");

    let mut options = RepositoryInitOptions::new();
    options.bare(true);
    options.initial_head("main");

    let repo = Repository::init_opts(&path, &options).expect("failed to init bare temp repo");
    (TempRepo { path }, repo)
}

pub(crate) fn write_file(repo_path: &Path, relative_path: &str, contents: &str) {
    let full_path = repo_path.join(relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).expect("failed to create parent dir");
    }
    fs::write(full_path, contents).expect("failed to write test file");
}

pub(crate) fn configure_test_signature(repo: &Repository) {
    let mut config = repo.config().expect("failed to open repo config");
    config
        .set_str("user.name", "Git Leviathan Test")
        .expect("failed to set user.name");
    config
        .set_str("user.email", "test@example.invalid")
        .expect("failed to set user.email");
}

pub(crate) fn commit_all(repo: &Repository, message: &str) -> git2::Oid {
    let mut index = repo.index().expect("failed to open index");
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .expect("failed to stage files");
    index.write().expect("failed to write index");

    let tree_id = index.write_tree().expect("failed to write tree");
    let tree = repo.find_tree(tree_id).expect("failed to find tree");
    let signature = git2::Signature::now("Git Leviathan Test", "test@example.invalid")
        .expect("failed to create test signature");
    let parent = repo
        .head()
        .ok()
        .and_then(|head| head.target())
        .and_then(|oid| repo.find_commit(oid).ok());

    match parent {
        Some(parent) => repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &[&parent],
            )
            .expect("failed to create commit"),
        None => repo
            .commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
            .expect("failed to create initial commit"),
    }
}

pub(crate) fn create_branch(repo: &Repository, branch_name: &str) {
    let head_commit = repo
        .head()
        .expect("repo should have head")
        .peel_to_commit()
        .expect("head should point to a commit");
    repo.branch(branch_name, &head_commit, false)
        .expect("failed to create branch");
}

pub(crate) fn checkout_branch_for_setup(repo: &Repository, branch_name: &str) {
    let reference_name = format!("refs/heads/{}", branch_name);
    repo.set_head(&reference_name)
        .expect("failed to update HEAD during setup");

    let mut checkout = CheckoutBuilder::new();
    checkout.force();
    repo.checkout_head(Some(&mut checkout))
        .expect("failed to checkout branch during setup");
}

pub(crate) fn create_remote_tracking_branch(repo: &Repository, full_name: &str) {
    let head_commit = repo
        .head()
        .expect("repo should have head")
        .peel_to_commit()
        .expect("head should point to a commit");
    let reference_name = format!("refs/remotes/{}", full_name);
    repo.reference(
        reference_name.as_str(),
        head_commit.id(),
        true,
        "test remote branch",
    )
    .expect("failed to create remote tracking branch");
}

pub(crate) fn add_remote(repo: &Repository, name: &str, url: &str) {
    repo.remote(name, url).expect("failed to add remote");
}

pub(crate) fn push_refspec(repo: &Repository, remote_name: &str, refspec: &str) {
    let mut remote = repo
        .find_remote(remote_name)
        .expect("failed to find remote for push");
    remote
        .push(&[refspec], None)
        .expect("failed to push refspec");
}

pub(crate) fn fetch_remote(repo: &Repository, remote_name: &str) {
    let mut remote = repo
        .find_remote(remote_name)
        .expect("failed to find remote for fetch");
    remote
        .fetch(&[] as &[&str], None, None)
        .expect("failed to fetch remote");
}

pub(crate) fn stash_names(repo: &mut Repository) -> Vec<String> {
    let mut names = Vec::new();
    repo.stash_foreach(|_, name, _| {
        names.push(name.to_string());
        true
    })
    .expect("failed to iterate stash");
    names
}

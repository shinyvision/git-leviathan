use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::Subscription;
use notify::Watcher;

use crate::core::TabId;

const DEBOUNCE_MS: u64 = 300;

/// Git directories to watch for `repo_path`.
///
/// For a primary repo: `<repo>/.git/`. For a secondary worktree, `.git` is a
/// pointer file and `repo.path()` returns `<primary>/.git/worktrees/<name>/`;
/// that directory carries per-worktree HEAD/index/FETCH_HEAD, while the
/// primary `<primary>/.git/` carries the shared refs and packed-refs the
/// worktree reads. Both are needed to catch every relevant change.
fn resolve_git_watch_paths(repo_path: &Path) -> Vec<PathBuf> {
    let Ok(repo) = git2::Repository::open(repo_path) else {
        return Vec::new();
    };
    let gitdir = repo.path().to_path_buf();
    let mut paths = Vec::new();
    if gitdir.exists() {
        paths.push(gitdir.clone());
    }
    if repo.is_worktree() {
        // `<primary>/.git/worktrees/<name>/` → `<primary>/.git/`
        if let Some(primary_git) = gitdir.parent().and_then(|p| p.parent()) {
            if primary_git.exists() && primary_git != gitdir {
                paths.push(primary_git.to_path_buf());
            }
        }
    }
    paths
}

/// Reject churn from `.git/objects/` (every fetch packfile lands here,
/// producing hundreds of events that never change refs the UI renders) and
/// `.git/logs/` (reflog-only). All other paths are relevant.
fn is_relevant_event_path(path: &std::path::Path) -> bool {
    let p_str = path.to_string_lossy();
    const NOISE_PREFIXES: &[&str] = &[
        "/.git/objects/",
        "\\.git\\objects\\",
        "/.git/logs/",
        "\\.git\\logs\\",
    ];
    !NOISE_PREFIXES.iter().any(|prefix| p_str.contains(prefix))
}

pub fn watch_repo_files(tab_id: TabId, repo_path: PathBuf) -> Subscription<TabId> {
    #[derive(Hash)]
    struct Id {
        tag: &'static str,
        tab_id: TabId,
        path: PathBuf,
    }
    fn build(id: &Id) -> impl iced::futures::Stream<Item = TabId> {
        let repo_path = id.path.clone();
        let tab_id = id.tab_id;
        iced::stream::channel(
            1,
            move |mut sender: iced::futures::channel::mpsc::Sender<TabId>| {
                let repo = repo_path.clone();
                async move {
                    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<()>(16);

                    let repo_for_watcher = repo.clone();
                    let sender_for_watcher = notify_tx.clone();
                    let watcher_result = notify::RecommendedWatcher::new(
                        move |res: Result<notify::Event, notify::Error>| {
                            if let Ok(event) = res {
                                match event.kind {
                                    notify::EventKind::Create(_)
                                    | notify::EventKind::Modify(_)
                                    | notify::EventKind::Remove(_) => {
                                        if event.paths.iter().any(|p| is_relevant_event_path(p)) {
                                            let _ = sender_for_watcher.try_send(());
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        },
                        notify::Config::default(),
                    );

                    match watcher_result {
                        Ok(mut watcher) => {
                            for git_path in resolve_git_watch_paths(&repo_for_watcher) {
                                let _ = watcher
                                    .watch(&git_path, notify::RecursiveMode::Recursive);
                            }
                            let _ =
                                watcher.watch(&repo_for_watcher, notify::RecursiveMode::Recursive);

                            let watcher_handle = WatcherHandle::new(watcher);

                            // Trailing-edge debounce. Leading-edge would fire on
                            // the first event of a write burst and observe
                            // intermediate index/workdir state (a file classified
                            // as staged before its workdir modification is
                            // re-statted), with no tail fire to correct it.
                            let debounce = Duration::from_millis(DEBOUNCE_MS);
                            let sleep = tokio::time::sleep(Duration::from_secs(0));
                            tokio::pin!(sleep);
                            let mut pending = false;

                            loop {
                                tokio::select! {
                                    biased;

                                    _ = tokio::signal::ctrl_c() => {
                                        break;
                                    }
                                    maybe = notify_rx.recv() => {
                                        if maybe.is_none() {
                                            break;
                                        }
                                        sleep.as_mut().reset(tokio::time::Instant::now() + debounce);
                                        pending = true;
                                    }
                                    _ = &mut sleep, if pending => {
                                        pending = false;
                                        let _ = sender.try_send(tab_id);
                                    }
                                }
                            }

                            drop(watcher_handle);
                        }
                        Err(e) => {
                            eprintln!("git_leviathan: could not create file watcher: {e}");
                        }
                    }
                }
            },
        )
    }
    Subscription::run_with(
        Id {
            tag: "repo-file-watcher",
            tab_id,
            path: repo_path,
        },
        build,
    )
}

/// Moves `Drop` of a `notify::RecommendedWatcher` onto a background thread.
/// The destructor joins the watcher's event-loop thread and calls
/// `inotify_rm_watch` / `FSEventStreamStop` for every registered path, which
/// can block for hundreds of milliseconds on large repos and would otherwise
/// freeze the UI during shutdown.
struct WatcherHandle {
    inner: Option<notify::RecommendedWatcher>,
}

impl WatcherHandle {
    fn new(watcher: notify::RecommendedWatcher) -> Self {
        Self {
            inner: Some(watcher),
        }
    }
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        if let Some(watcher) = self.inner.take() {
            std::thread::spawn(move || drop(watcher));
        }
    }
}

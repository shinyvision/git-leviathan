//! File watcher that emits one debounced reload event per plugin
//! directory whenever its `init.lua` (or any tracked file) changes on
//! disk. Multi-write bursts coalesce into a single event.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{event::EventKind, Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

const DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadEventKind {
    Modified,
}

#[derive(Debug, Clone)]
pub struct ReloadEvent {
    pub plugin_dir: PathBuf,
    pub kind: ReloadEventKind,
}

pub struct ReloadWatcher {
    _watcher: RecommendedWatcher,
    _debouncer: tokio::task::JoinHandle<()>,
}

impl ReloadWatcher {
    /// Watch `plugin_dir` recursively. Coalesce file events within
    /// `DEBOUNCE` and forward one `ReloadEvent` to `tx` per quiet
    /// period.
    pub fn new(plugin_dir: &Path, tx: mpsc::Sender<ReloadEvent>) -> notify::Result<Self> {
        let dir_owned = plugin_dir.to_path_buf();
        let pending: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let pending_for_watcher = Arc::clone(&pending);

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else {
                return;
            };
            if !is_relevant(&event) {
                return;
            }
            *pending_for_watcher.lock().unwrap() = Some(Instant::now());
        })?;
        watcher.watch(plugin_dir, RecursiveMode::Recursive)?;

        let debouncer = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                let due = {
                    let mut g = pending.lock().unwrap();
                    match *g {
                        Some(when) if when.elapsed() >= DEBOUNCE => {
                            *g = None;
                            true
                        }
                        _ => false,
                    }
                };
                if due {
                    let evt = ReloadEvent {
                        plugin_dir: dir_owned.clone(),
                        kind: ReloadEventKind::Modified,
                    };
                    if tx.send(evt).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Self {
            _watcher: watcher,
            _debouncer: debouncer,
        })
    }
}

impl Drop for ReloadWatcher {
    fn drop(&mut self) {
        self._debouncer.abort();
    }
}

fn is_relevant(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    fn basic_manifest() -> &'static str {
        r#"id = "smoke"
name = "Smoke"
version = "0.1.0"
api_version = "1.0"
"#
    }

    #[tokio::test]
    async fn watcher_fires_on_init_lua_change() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("plugin.toml"), basic_manifest()).unwrap();
        std::fs::write(dir.path().join("init.lua"), "-- v1").unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _watcher = ReloadWatcher::new(dir.path(), tx).unwrap();

        // Give notify a beat to register before mutation.
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(dir.path().join("init.lua"), "-- updated").unwrap();
        let evt = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .expect("watcher channel should produce within 3s")
            .expect("channel should not be closed");
        assert_eq!(evt.plugin_dir, dir.path().to_path_buf());
    }

    #[tokio::test]
    async fn watcher_debounces_burst() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("plugin.toml"), basic_manifest()).unwrap();
        std::fs::write(dir.path().join("init.lua"), "-- v1").unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let _watcher = ReloadWatcher::new(dir.path(), tx).unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        // Burst of 5 writes within debounce window
        for i in 0..5 {
            std::fs::write(dir.path().join("init.lua"), format!("-- v{i}")).unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // Wait past debounce. We expect ONE reload event coalesced from the burst.
        tokio::time::sleep(Duration::from_millis(400)).await;
        let mut count = 0;
        while let Ok(Some(_)) = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await {
            count += 1;
        }
        assert!(
            count >= 1 && count <= 2,
            "expected debounced 1-2 events, got {count}"
        );
    }
}

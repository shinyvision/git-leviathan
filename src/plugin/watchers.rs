//! Phase 12 file-watcher registry. `leviathan.fs.watch(path, opts, cb)`
//! creates one entry; the host's `tick()` drains buffered events from
//! the underlying `notify` watcher and invokes the Lua callback on
//! the main thread with a serialised event payload.
//!
//! Cross-thread safety: the `notify::RecommendedWatcher` runs its
//! callback on a `notify`-owned thread; we cross the boundary by
//! pushing into an `mpsc::Sender<WatchEvent>`. The host main thread
//! owns the receivers and re-hydrates each event into a Lua table on
//! drain.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use mlua::RegistryKey;
use notify::{event::EventKind, Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::plugin::resources::{GenerationId, PluginId, ResourceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WatchId(u64);

impl WatchId {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct WatchEvent {
    pub kind: String,
    pub paths: Vec<String>,
}

impl WatchEvent {
    pub fn from_notify(event: Event) -> Option<Self> {
        let kind = match event.kind {
            EventKind::Create(_) => "create",
            EventKind::Modify(_) => "modify",
            EventKind::Remove(_) => "remove",
            EventKind::Access(_) => "access",
            EventKind::Other => "other",
            EventKind::Any => "any",
        };
        let paths = event
            .paths
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        Some(Self {
            kind: kind.to_string(),
            paths,
        })
    }
}

struct WatchRecord {
    plugin_id: PluginId,
    generation_id: GenerationId,
    watch_id: WatchId,
    resource_id: ResourceId,
    path: PathBuf,
    recursive: bool,
    rx: Mutex<Receiver<WatchEvent>>,
    /// `Drop` on the watcher releases the OS handle. Held in
    /// `Mutex<Option<>>` so `cancel_for_generation` can take it
    /// without `&mut self`.
    watcher: Mutex<Option<RecommendedWatcher>>,
}

/// Cheap-clone (Arc-backed) per-host registry.
#[derive(Clone, Default)]
pub struct FileWatcherRegistry {
    inner: Arc<Mutex<WatcherInner>>,
}

#[derive(Default)]
struct WatcherInner {
    next_id: u64,
    watchers: HashMap<WatchId, WatchRecord>,
}

/// One drained event ready to dispatch to a plugin's Lua callback.
#[derive(Debug, Clone)]
pub struct DrainedWatchEvent {
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
    pub watch_id: WatchId,
    pub resource_id: ResourceId,
    pub event: WatchEvent,
}

impl FileWatcherRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allocate(&self) -> WatchId {
        let mut inner = self.inner.lock().expect("watcher registry poisoned");
        inner.next_id += 1;
        WatchId(inner.next_id)
    }

    /// Spawn the underlying `notify` watcher and register the record.
    /// Returns an error string if the OS rejects the watch (path
    /// missing, permission denied, etc.). Caller is expected to
    /// surface the error as a `PluginDiagnostic`.
    pub fn register(
        &self,
        plugin_id: PluginId,
        generation_id: GenerationId,
        watch_id: WatchId,
        resource_id: ResourceId,
        path: PathBuf,
        recursive: bool,
    ) -> Result<(), String> {
        let (tx, rx) = channel::<WatchEvent>();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                if let Some(payload) = WatchEvent::from_notify(event) {
                    let _ = tx.send(payload);
                }
            }
        })
        .map_err(|e| format!("notify watcher init failed: {e}"))?;
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        watcher
            .watch(&path, mode)
            .map_err(|e| format!("notify watch failed for {}: {e}", path.display()))?;
        let record = WatchRecord {
            plugin_id,
            generation_id,
            watch_id,
            resource_id,
            path,
            recursive,
            rx: Mutex::new(rx),
            watcher: Mutex::new(Some(watcher)),
        };
        let mut inner = self.inner.lock().expect("watcher registry poisoned");
        inner.watchers.insert(watch_id, record);
        Ok(())
    }

    /// Drop the OS watcher and remove the record.
    pub fn cancel(&self, watch_id: WatchId) {
        let mut inner = self.inner.lock().expect("watcher registry poisoned");
        if let Some(record) = inner.watchers.remove(&watch_id) {
            // Drop the underlying watcher.
            drop(record.watcher.lock().expect("watcher mutex").take());
        }
    }

    pub fn cancel_for_generation(
        &self,
        plugin_id: &PluginId,
        generation_id: GenerationId,
    ) -> Vec<WatchId> {
        let mut inner = self.inner.lock().expect("watcher registry poisoned");
        let to_remove: Vec<WatchId> = inner
            .watchers
            .iter()
            .filter(|(_, r)| &r.plugin_id == plugin_id && r.generation_id == generation_id)
            .map(|(id, _)| *id)
            .collect();
        for id in &to_remove {
            if let Some(record) = inner.watchers.remove(id) {
                drop(record.watcher.lock().expect("watcher mutex").take());
            }
        }
        to_remove
    }

    /// Drain every queued event across every active watcher. Events
    /// are returned in receive order per-watcher. A watcher that
    /// drops its sender (e.g. notify thread terminated) is silently
    /// reaped on the next drain.
    pub fn drain_events(&self) -> Vec<DrainedWatchEvent> {
        let mut out: Vec<DrainedWatchEvent> = Vec::new();
        let inner = self.inner.lock().expect("watcher registry poisoned");
        for record in inner.watchers.values() {
            let rx_guard = record.rx.lock().expect("rx mutex");
            loop {
                match rx_guard.try_recv() {
                    Ok(event) => out.push(DrainedWatchEvent {
                        plugin_id: record.plugin_id.clone(),
                        generation_id: record.generation_id,
                        watch_id: record.watch_id,
                        resource_id: record.resource_id,
                        event,
                    }),
                    Err(_) => break,
                }
            }
        }
        out
    }

    pub fn is_alive(&self, watch_id: WatchId) -> bool {
        let inner = self.inner.lock().expect("watcher registry poisoned");
        inner.watchers.contains_key(&watch_id)
    }

    pub fn summaries(&self) -> Vec<WatcherSummary> {
        let inner = self.inner.lock().expect("watcher registry poisoned");
        let mut out: Vec<WatcherSummary> = inner
            .watchers
            .values()
            .map(|r| WatcherSummary {
                plugin_id: r.plugin_id.to_string(),
                generation_id: r.generation_id.get(),
                watch_id: r.watch_id.get(),
                path: r.path.display().to_string(),
                recursive: r.recursive,
            })
            .collect();
        out.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.generation_id.cmp(&b.generation_id))
                .then(a.watch_id.cmp(&b.watch_id))
        });
        out
    }
}

/// Shape exposed in the devtools snapshot.
#[derive(Debug, Clone)]
pub struct WatcherSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub watch_id: u64,
    pub path: String,
    pub recursive: bool,
}

/// Per-plugin Lua callback table. Same model as `PluginTimerCallbacks`.
#[derive(Default)]
pub struct PluginWatcherCallbacks {
    inner: HashMap<WatchId, RegistryKey>,
}

impl PluginWatcherCallbacks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, watch_id: WatchId, key: RegistryKey) {
        self.inner.insert(watch_id, key);
    }

    pub fn remove(&mut self, watch_id: WatchId) -> Option<RegistryKey> {
        self.inner.remove(&watch_id)
    }

    pub fn get(&self, watch_id: WatchId) -> Option<&RegistryKey> {
        self.inner.get(&watch_id)
    }
}

//! async-job registry. Each plugin gets a cheap-clone
//! `AsyncJobRegistry` handle. `leviathan.async.spawn(fn)` records a
//! `JobHandle`, spawns a `std::thread`, and the body runs off-thread
//! while the main thread keeps ticking. Cancellation, status, and the
//! result payload travel back through the registry; the host's
//! `tick()` drains finished jobs and runs their on-complete callbacks
//! on the main Lua state.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::SystemTime;

use crate::plugin::resources::{GenerationId, PluginId, ResourceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(u64);

impl JobId {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Job lifecycle. `Running` stays true while the worker thread is
/// alive; the host transitions to one of the terminal states when it
/// drains the result channel during `tick()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Finished,
    Cancelled,
    Failed,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

/// Cancellation token. Cloned into the worker thread; the body checks
/// `is_cancelled()` cooperatively. The Lua `JobHandle:cancel()` method
/// flips this flag; the host's `tick()` reads it when processing the
/// result.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// Outcome of a worker thread. Either a JSON payload to feed to the
/// completion callback, an error message, or a marker that the body
/// observed cancellation.
pub enum JobOutcome {
    Ok(serde_json::Value),
    Cancelled,
    Failed(String),
}

#[derive(Debug)]
struct JobRecord {
    plugin_id: PluginId,
    generation_id: GenerationId,
    job_id: JobId,
    resource_id: ResourceId,
    status: JobStatus,
    started_at: SystemTime,
    cancel: CancellationToken,
    /// Joined when the host drains the channel; held in `Mutex<Option<>>`
    /// so `tick()` can take it without `&mut self` on the registry.
    join: Mutex<Option<JoinHandle<()>>>,
    /// One-shot result channel. The worker thread sends on this when it
    /// finishes (or observes cancellation). The host pumps the receiver
    /// during `tick()`.
    rx: Mutex<Option<Receiver<JobOutcome>>>,
}

/// Per-plugin async-job registry. Cheap-clone (Arc-backed). Owns every
/// job record keyed by `(plugin_id, generation_id, JobId)`. Generation
/// ownership is enforced at drop time: `cancel_for_generation` flips
/// every cancel token belonging to the dropped generation, joins
/// straggler threads, and removes the records.
#[derive(Debug, Clone, Default)]
pub struct AsyncJobRegistry {
    inner: Arc<Mutex<RegistryInner>>,
}

#[derive(Debug, Default)]
struct RegistryInner {
    next_job_id: u64,
    jobs: HashMap<JobId, JobRecord>,
}

/// One drained completion ready for the main-thread Lua callback.
pub struct DrainedJob {
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
    pub job_id: JobId,
    pub resource_id: ResourceId,
    pub outcome: JobOutcome,
}

pub struct AsyncJobRegistration {
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
    pub job_id: JobId,
    pub resource_id: ResourceId,
    pub cancel: CancellationToken,
    pub join: JoinHandle<()>,
    pub rx: Receiver<JobOutcome>,
}

impl AsyncJobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allocate_job_id(&self) -> JobId {
        let mut inner = self.inner.lock().expect("async registry poisoned");
        inner.next_job_id += 1;
        JobId(inner.next_job_id)
    }

    /// Record a freshly spawned job so the host can later drain its
    /// completion. `join` is the worker thread; `rx` is the receiver
    /// the worker sends its `JobOutcome` on.
    pub fn register(&self, registration: AsyncJobRegistration) {
        let record = JobRecord {
            plugin_id: registration.plugin_id,
            generation_id: registration.generation_id,
            job_id: registration.job_id,
            resource_id: registration.resource_id,
            status: JobStatus::Running,
            started_at: SystemTime::now(),
            cancel: registration.cancel,
            join: Mutex::new(Some(registration.join)),
            rx: Mutex::new(Some(registration.rx)),
        };
        let mut inner = self.inner.lock().expect("async registry poisoned");
        inner.jobs.insert(registration.job_id, record);
    }

    /// Trip the cancel token for `job_id`. The worker thread's body
    /// observes the flag through its own `CancellationToken` clone.
    /// No-op when the job is already removed.
    pub fn cancel(&self, job_id: JobId) {
        let inner = self.inner.lock().expect("async registry poisoned");
        if let Some(record) = inner.jobs.get(&job_id) {
            record.cancel.cancel();
        }
    }

    /// Drain every job whose worker has finished. Removes the record
    /// from the registry and returns the outcome so the host can
    /// invoke the Lua on-complete callback on the main thread.
    pub fn drain_finished(&self) -> Vec<DrainedJob> {
        let mut drained = Vec::new();
        let mut inner = self.inner.lock().expect("async registry poisoned");
        let mut finished_ids: Vec<JobId> = Vec::new();
        for (job_id, record) in inner.jobs.iter_mut() {
            // try_recv on the channel; non-blocking.
            let outcome_opt = {
                let mut rx_guard = record.rx.lock().expect("rx mutex");
                if let Some(rx) = rx_guard.as_ref() {
                    match rx.try_recv() {
                        Ok(outcome) => {
                            *rx_guard = None;
                            Some(outcome)
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            // Worker thread panicked or dropped the
                            // sender without sending. Surface as a
                            // failure so the callback path still fires.
                            *rx_guard = None;
                            Some(JobOutcome::Failed(
                                "async worker dropped before sending result".to_string(),
                            ))
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    }
                } else {
                    None
                }
            };
            if let Some(outcome) = outcome_opt {
                record.status = match &outcome {
                    JobOutcome::Ok(_) => JobStatus::Finished,
                    JobOutcome::Cancelled => JobStatus::Cancelled,
                    JobOutcome::Failed(_) => JobStatus::Failed,
                };
                finished_ids.push(*job_id);
                drained.push(DrainedJob {
                    plugin_id: record.plugin_id.clone(),
                    generation_id: record.generation_id,
                    job_id: *job_id,
                    resource_id: record.resource_id,
                    outcome,
                });
                // Best-effort join: we already received; the worker is
                // about to (or has) returned. join is non-blocking in
                // practice because the worker sent before exiting.
                if let Some(handle) = record.join.lock().expect("join mutex").take() {
                    let _ = handle.join();
                }
            }
        }
        for id in finished_ids {
            inner.jobs.remove(&id);
        }
        drained
    }

    /// Cancel + remove every job for `(plugin_id, generation_id)`.
    /// Called from `unload_plugin` and `commit_staging`. After this
    /// returns, no further completion will fire for the dropped
    /// generation (the in-flight thread may still be running, but its
    /// receiver is gone — `drain_finished` skips orphan records).
    pub fn cancel_for_generation(
        &self,
        plugin_id: &PluginId,
        generation_id: GenerationId,
    ) -> Vec<JobId> {
        let mut inner = self.inner.lock().expect("async registry poisoned");
        let to_remove: Vec<JobId> = inner
            .jobs
            .iter()
            .filter(|(_, r)| &r.plugin_id == plugin_id && r.generation_id == generation_id)
            .map(|(id, _)| *id)
            .collect();
        for id in &to_remove {
            if let Some(record) = inner.jobs.get(id) {
                record.cancel.cancel();
            }
        }
        for id in &to_remove {
            if let Some(record) = inner.jobs.remove(id) {
                // Drop the receiver so the worker's send is a no-op.
                if let Some(handle) = record.join.lock().expect("join mutex").take() {
                    // Don't block waiting for a long-running job —
                    // the cancel token signals it to exit; we detach.
                    drop(handle);
                }
            }
        }
        to_remove
    }

    /// Cheap-clone snapshot used by devtools.
    pub fn summaries(&self) -> Vec<AsyncJobSummary> {
        let inner = self.inner.lock().expect("async registry poisoned");
        let mut out: Vec<AsyncJobSummary> = inner
            .jobs
            .values()
            .map(|r| AsyncJobSummary {
                plugin_id: r.plugin_id.to_string(),
                generation_id: r.generation_id.get(),
                job_id: r.job_id.get(),
                started_at_unix_ms: r
                    .started_at
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0),
                status: r.status.as_str().to_string(),
            })
            .collect();
        out.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.generation_id.cmp(&b.generation_id))
                .then(a.job_id.cmp(&b.job_id))
        });
        out
    }

    pub fn is_alive(&self, job_id: JobId) -> bool {
        let inner = self.inner.lock().expect("async registry poisoned");
        inner.jobs.contains_key(&job_id)
    }

    pub fn has_jobs(&self) -> bool {
        let inner = self.inner.lock().expect("async registry poisoned");
        !inner.jobs.is_empty()
    }
}

/// Shape exposed in the devtools snapshot. Mirrored by
/// `crate::plugin::devtools::AsyncJobSummary` (the public projection).
#[derive(Debug, Clone)]
pub struct AsyncJobSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub job_id: u64,
    pub started_at_unix_ms: u128,
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn cancel_for_generation_removes_records() {
        let reg = AsyncJobRegistry::new();
        let plugin = PluginId::from("p");
        let gen = GenerationId::new(1);
        let job_id = reg.allocate_job_id();
        let cancel = CancellationToken::new();
        let (tx, rx) = channel();
        let handle = std::thread::spawn(move || {
            let _ = tx.send(JobOutcome::Ok(serde_json::Value::Null));
        });
        reg.register(AsyncJobRegistration {
            plugin_id: plugin.clone(),
            generation_id: gen,
            job_id,
            resource_id: ResourceId::new(1),
            cancel,
            join: handle,
            rx,
        });
        let removed = reg.cancel_for_generation(&plugin, gen);
        assert_eq!(removed, vec![job_id]);
        assert!(!reg.is_alive(job_id));
    }
}

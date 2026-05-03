//! async runtime timer registry. `leviathan.timer.after(ms, fn)` records a
//! one-shot timer; `leviathan.timer.every(ms, fn)` records a repeating
//! timer. Both fire on the host's `tick()` loop using a virtual clock
//! (`Instant::now()`); cancellation is cooperative — `:cancel()` on
//! the Lua handle removes the record so the next tick skips it.
//!
//! Timers re-use the existing `DeferredQueue::delayed` infrastructure
//! for one-shots; for `every`, the host re-enqueues a fresh deadline
//! after each fire while the registry record is still alive.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mlua::RegistryKey;

use crate::plugin::resources::{GenerationId, PluginId, ResourceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(u64);

impl TimerId {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerKind {
    After,
    Every,
}

impl TimerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::After => "after",
            Self::Every => "every",
        }
    }
}

/// Internal record; the Lua callback is parked through a
/// `RegistryKey` separately on the per-plugin Lua state. The registry
/// only tracks lifecycle.
#[derive(Debug)]
struct TimerRecord {
    plugin_id: PluginId,
    generation_id: GenerationId,
    timer_id: TimerId,
    resource_id: ResourceId,
    kind: TimerKind,
    interval: Duration,
    next_fire: Instant,
    fires: u64,
    cancelled: bool,
}

/// Cheap-clone (Arc-backed) per-host registry. Each `LoadedPlugin`
/// also holds the `RegistryKey` table separately so cancellation can
/// drop the Lua callback without touching the timer registry's data
/// structures.
#[derive(Debug, Clone, Default)]
pub struct TimerRegistry {
    inner: Arc<Mutex<TimerInner>>,
}

#[derive(Debug, Default)]
struct TimerInner {
    next_id: u64,
    timers: HashMap<TimerId, TimerRecord>,
}

/// One due timer the host should fire on the next tick. The host
/// looks up the Lua callback by `(plugin_id, timer_id)` against the
/// per-plugin callback table.
#[derive(Debug, Clone)]
pub struct DueTimer {
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
    pub timer_id: TimerId,
    pub kind: TimerKind,
    pub resource_id: ResourceId,
}

impl TimerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allocate(&self) -> TimerId {
        let mut inner = self.inner.lock().expect("timer registry poisoned");
        inner.next_id += 1;
        TimerId(inner.next_id)
    }

    pub fn register(
        &self,
        plugin_id: PluginId,
        generation_id: GenerationId,
        timer_id: TimerId,
        resource_id: ResourceId,
        kind: TimerKind,
        interval_ms: u64,
    ) {
        let interval = Duration::from_millis(interval_ms);
        let record = TimerRecord {
            plugin_id,
            generation_id,
            timer_id,
            resource_id,
            kind,
            interval,
            next_fire: Instant::now() + interval,
            fires: 0,
            cancelled: false,
        };
        let mut inner = self.inner.lock().expect("timer registry poisoned");
        inner.timers.insert(timer_id, record);
    }

    /// Mark a timer cancelled. The next `drain_due` skips it; the
    /// record is reaped on the same call.
    pub fn cancel(&self, timer_id: TimerId) {
        let mut inner = self.inner.lock().expect("timer registry poisoned");
        if let Some(record) = inner.timers.get_mut(&timer_id) {
            record.cancelled = true;
        }
    }

    pub fn cancel_for_generation(
        &self,
        plugin_id: &PluginId,
        generation_id: GenerationId,
    ) -> Vec<TimerId> {
        let mut inner = self.inner.lock().expect("timer registry poisoned");
        let to_remove: Vec<TimerId> = inner
            .timers
            .iter()
            .filter(|(_, r)| &r.plugin_id == plugin_id && r.generation_id == generation_id)
            .map(|(id, _)| *id)
            .collect();
        for id in &to_remove {
            inner.timers.remove(id);
        }
        to_remove
    }

    /// Pop every due timer at `now`. One-shots are removed; repeating
    /// timers have their `next_fire` advanced by `interval` and their
    /// `fires` counter incremented before staying in the registry.
    /// Cancelled timers are reaped without firing.
    pub fn drain_due(&self, now: Instant) -> Vec<DueTimer> {
        let mut inner = self.inner.lock().expect("timer registry poisoned");
        let mut due: Vec<DueTimer> = Vec::new();
        let mut to_remove: Vec<TimerId> = Vec::new();
        for (id, record) in inner.timers.iter_mut() {
            if record.cancelled {
                to_remove.push(*id);
                continue;
            }
            if record.next_fire <= now {
                due.push(DueTimer {
                    plugin_id: record.plugin_id.clone(),
                    generation_id: record.generation_id,
                    timer_id: record.timer_id,
                    kind: record.kind,
                    resource_id: record.resource_id,
                });
                record.fires += 1;
                match record.kind {
                    TimerKind::After => to_remove.push(*id),
                    TimerKind::Every => record.next_fire = now + record.interval,
                }
            }
        }
        for id in to_remove {
            inner.timers.remove(&id);
        }
        due
    }

    pub fn is_alive(&self, timer_id: TimerId) -> bool {
        let inner = self.inner.lock().expect("timer registry poisoned");
        inner
            .timers
            .get(&timer_id)
            .map(|r| !r.cancelled)
            .unwrap_or(false)
    }

    pub fn summaries(&self) -> Vec<TimerSummary> {
        let inner = self.inner.lock().expect("timer registry poisoned");
        let mut out: Vec<TimerSummary> = inner
            .timers
            .values()
            .map(|r| TimerSummary {
                plugin_id: r.plugin_id.to_string(),
                generation_id: r.generation_id.get(),
                timer_id: r.timer_id.get(),
                kind: r.kind.as_str().to_string(),
                interval_ms: r.interval.as_millis() as u64,
                fires: r.fires,
            })
            .collect();
        out.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.generation_id.cmp(&b.generation_id))
                .then(a.timer_id.cmp(&b.timer_id))
        });
        out
    }
}

/// Shape exposed in the devtools snapshot.
#[derive(Debug, Clone)]
pub struct TimerSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub timer_id: u64,
    pub kind: String,
    pub interval_ms: u64,
    pub fires: u64,
}

/// Per-plugin Lua callback table. The registry's `TimerId` keys this
/// map; `tick()` looks the callback up and invokes it. Held in the
/// `LoadedPlugin` (one per generation) so reload drops the keys with
/// the Lua state.
#[derive(Default)]
pub struct PluginTimerCallbacks {
    inner: HashMap<TimerId, RegistryKey>,
}

impl PluginTimerCallbacks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, timer_id: TimerId, key: RegistryKey) {
        self.inner.insert(timer_id, key);
    }

    pub fn remove(&mut self, timer_id: TimerId) -> Option<RegistryKey> {
        self.inner.remove(&timer_id)
    }

    pub fn get(&self, timer_id: TimerId) -> Option<&RegistryKey> {
        self.inner.get(&timer_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shot_fires_once_then_disappears() {
        let reg = TimerRegistry::new();
        let plugin = PluginId::from("p");
        let gen = GenerationId::new(1);
        let id = reg.allocate();
        reg.register(
            plugin.clone(),
            gen,
            id,
            ResourceId::new(1),
            TimerKind::After,
            0,
        );
        let due = reg.drain_due(Instant::now() + Duration::from_millis(1));
        assert_eq!(due.len(), 1);
        assert!(!reg.is_alive(id));
    }

    #[test]
    fn repeating_keeps_firing() {
        let reg = TimerRegistry::new();
        let plugin = PluginId::from("p");
        let gen = GenerationId::new(1);
        let id = reg.allocate();
        reg.register(
            plugin.clone(),
            gen,
            id,
            ResourceId::new(1),
            TimerKind::Every,
            1,
        );
        let due1 = reg.drain_due(Instant::now() + Duration::from_millis(50));
        let due2 = reg.drain_due(Instant::now() + Duration::from_millis(100));
        assert_eq!(due1.len(), 1);
        assert_eq!(due2.len(), 1);
        assert!(reg.is_alive(id));
        reg.cancel(id);
        let due3 = reg.drain_due(Instant::now() + Duration::from_millis(200));
        assert_eq!(due3.len(), 0);
        assert!(!reg.is_alive(id));
    }

    #[test]
    fn cancel_for_generation_removes_records() {
        let reg = TimerRegistry::new();
        let plugin = PluginId::from("p");
        let gen = GenerationId::new(1);
        let id = reg.allocate();
        reg.register(
            plugin.clone(),
            gen,
            id,
            ResourceId::new(1),
            TimerKind::Every,
            50,
        );
        reg.cancel_for_generation(&plugin, gen);
        assert!(!reg.is_alive(id));
    }
}

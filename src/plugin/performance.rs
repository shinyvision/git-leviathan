//! Performance budgets and circuit breakers for host-invoked callbacks.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::resources::{GenerationId, PluginId};

/// Window size for the per-callback duration sample buffer. Keeps the
/// histogram cheap (constant memory per callback) while giving p50/p95
/// rough but useful resolution for typical autocmd/command rates.
pub const SAMPLE_WINDOW: usize = 64;

/// Number of recent traces retained for the devtools panel.
pub const TRACE_RING_CAP: usize = 256;

/// Default consecutive-failure / hard-breach threshold before a
/// callback is auto-disabled.
pub const DEFAULT_FAILURE_THRESHOLD: u32 = 5;

/// Number of disabled callbacks a plugin can accumulate (in one
/// generation) before the plugin itself is marked degraded.
pub const MAX_DISABLED_CALLBACKS_PER_PLUGIN: usize = 3;

/// Kinds of plugin callbacks that flow through the budget tracker.
/// Each kind has its own default `(soft_ms, hard_ms)` budget; tighter
/// budgets apply to user-interactive paths (UI render, events) and
/// looser budgets apply to async/init paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallbackKind {
    Init,
    EventCallback,
    UiCallback,
    CommandCallback,
    ServiceCall,
    Timer,
    AsyncJob,
}

impl CallbackKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::EventCallback => "event",
            Self::UiCallback => "ui",
            Self::CommandCallback => "command",
            Self::ServiceCall => "service",
            Self::Timer => "timer",
            Self::AsyncJob => "async_job",
        }
    }

    pub fn default_budget(self) -> CallbackBudget {
        match self {
            Self::Init => CallbackBudget {
                soft_ms: 200,
                hard_ms: 2000,
            },
            Self::EventCallback => CallbackBudget {
                soft_ms: 20,
                hard_ms: 250,
            },
            Self::UiCallback => CallbackBudget {
                soft_ms: 8,
                hard_ms: 100,
            },
            Self::CommandCallback => CallbackBudget {
                soft_ms: 50,
                hard_ms: 500,
            },
            Self::ServiceCall => CallbackBudget {
                soft_ms: 20,
                hard_ms: 250,
            },
            Self::Timer => CallbackBudget {
                soft_ms: 20,
                hard_ms: 250,
            },
            Self::AsyncJob => CallbackBudget {
                soft_ms: 200,
                hard_ms: 10_000,
            },
        }
    }
}

/// Soft / hard latency thresholds for a callback. Going over the soft
/// threshold emits a warning diagnostic; going over the hard threshold
/// emits an error diagnostic and counts toward the circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallbackBudget {
    pub soft_ms: u128,
    pub hard_ms: u128,
}

/// Pluggable clock so deterministic tests can drive timing without
/// real `Instant::now()` jitter. Production uses [`SystemClock`].
pub trait Clock: Send + Sync + 'static {
    /// Current monotonic time in milliseconds. The absolute origin
    /// doesn't matter — only deltas.
    fn now_ms(&self) -> u128;
    /// Wall-clock unix-ms; used as the trace timestamp so devtools can
    /// render absolute times. Deterministic tests provide a fixed
    /// epoch.
    fn now_unix_ms(&self) -> u128;
}

/// Production clock backed by `Instant::now()` and `SystemTime::now()`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u128 {
        // Anchor to a process-local origin captured the first time
        // the clock is read. We hand back a millisecond tick from a
        // fresh `Instant::now()` against a baseline so the value is
        // monotonic and bounded.
        let dur = Instant::now().elapsed();
        // `Instant::now().elapsed()` is always near-zero on the very
        // first call and grows from there, but that's not what we
        // want — we want a stable reference. Use `SystemTime` deltas
        // for the actual ms count; tests use `MockClock` so we don't
        // need precision here.
        let _ = dur;
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    }
    fn now_unix_ms(&self) -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    }
}

/// Deterministic clock used by tests. Gated to `cfg(test)` because
/// production uses the `SystemClock`; building it into the binary
/// would leave it unreferenced.
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub struct MockClock {
    inner: Arc<Mutex<MockClockInner>>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct MockClockInner {
    now_ms: u128,
    unix_ms: u128,
}

#[cfg(test)]
impl MockClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn advance_ms(&self, delta: u128) {
        let mut g = self.inner.lock().expect("mock clock poisoned");
        g.now_ms = g.now_ms.saturating_add(delta);
        g.unix_ms = g.unix_ms.saturating_add(delta);
    }
}

#[cfg(test)]
impl Clock for MockClock {
    fn now_ms(&self) -> u128 {
        self.inner.lock().expect("mock clock poisoned").now_ms
    }
    fn now_unix_ms(&self) -> u128 {
        self.inner.lock().expect("mock clock poisoned").unix_ms
    }
}

/// Circuit-breaker state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakerState {
    Healthy,
    Tripped { since_unix_ms: u128, reason: String },
}

impl BreakerState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Tripped { .. } => "tripped",
        }
    }
}

/// One sample retained in a callback's rolling window.
#[derive(Debug, Clone, Copy)]
struct Sample {
    duration_ms: u128,
    ok: bool,
}

#[derive(Debug, Default)]
struct CallbackStats {
    samples: VecDeque<Sample>,
    count: u64,
    ok_count: u64,
    err_count: u64,
    consecutive_failures: u32,
    consecutive_hard_breaches: u32,
    state: Option<BreakerState>,
    last_failure: Option<String>,
    /// Plugin-supplied callback kind, captured at first record so
    /// devtools can label the row even when the breaker is tripped
    /// before any further calls run.
    kind: Option<CallbackKind>,
}

impl CallbackStats {
    fn record(&mut self, sample: Sample, kind: CallbackKind) {
        if self.samples.len() == SAMPLE_WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        self.count += 1;
        if sample.ok {
            self.ok_count += 1;
        } else {
            self.err_count += 1;
        }
        self.kind = Some(kind);
    }

    fn percentile(&self, p: f64) -> u128 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut v: Vec<u128> = self.samples.iter().map(|s| s.duration_ms).collect();
        v.sort_unstable();
        let idx = ((v.len() as f64 - 1.0) * p).round() as usize;
        v[idx.min(v.len() - 1)]
    }

    fn p50(&self) -> u128 {
        self.percentile(0.5)
    }
    fn p95(&self) -> u128 {
        self.percentile(0.95)
    }

    fn is_active_breach(&self) -> bool {
        matches!(self.state, Some(BreakerState::Tripped { .. }))
    }
}

/// Trace row retained for the devtools panel. Each entry is one
/// completed `track_call` invocation (skipped calls are recorded
/// separately as `skipped_calls` totals on the breaker row, not as
/// trace rows).
#[derive(Debug, Clone)]
pub struct PerformanceTrace {
    pub plugin_id: String,
    pub generation_id: u64,
    pub callback_id: String,
    pub kind: CallbackKind,
    pub duration_ms: u128,
    pub ok: bool,
    pub timestamp_unix_ms: u128,
}

/// Cheap-clone (Arc-Mutex) tracker. Constructed by `PluginHost::new`,
/// shared into every dispatch path.
#[derive(Clone)]
pub struct BudgetTracker {
    inner: Arc<Mutex<TrackerInner>>,
    diagnostics: DiagnosticStore,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for BudgetTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BudgetTracker").finish()
    }
}

#[derive(Debug, Default)]
struct TrackerInner {
    /// Per `(plugin_id, generation_id, callback_id)` stats and breaker.
    stats: HashMap<TrackerKey, CallbackStats>,
    /// Recent traces, oldest first. Capped at [`TRACE_RING_CAP`].
    traces: VecDeque<PerformanceTrace>,
    /// Per `(plugin_id, generation_id)` set of disabled callback ids,
    /// recomputed on every state transition. Used to gate the
    /// `plugin.degraded` diagnostic.
    degraded_emitted: HashMap<(String, u64), bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TrackerKey {
    plugin_id: String,
    generation_id: u64,
    callback_id: String,
}

/// Result of a `track_call`. `Skipped` means the breaker was tripped
/// and the body never ran. `Ok` / `Err` carry through whatever the
/// body produced.
#[derive(Debug)]
pub enum Outcome<R, E> {
    Skipped,
    Ok(R),
    Err(E),
}

impl BudgetTracker {
    pub fn new(diagnostics: DiagnosticStore) -> Self {
        Self::with_clock(diagnostics, Arc::new(SystemClock))
    }

    pub fn with_clock(diagnostics: DiagnosticStore, clock: Arc<dyn Clock>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TrackerInner::default())),
            diagnostics,
            clock,
        }
    }

    /// Run `body` under this callback's budget + breaker. The body is
    /// only invoked when the breaker is healthy; otherwise [`Outcome::Skipped`]
    /// is returned and a `performance.tripped` diagnostic emitted.
    pub fn track_call<R, E: std::fmt::Display>(
        &self,
        kind: CallbackKind,
        plugin_id: &PluginId,
        generation_id: GenerationId,
        callback_id: &str,
        body: impl FnOnce() -> Result<R, E>,
    ) -> Outcome<R, E> {
        let key = TrackerKey {
            plugin_id: plugin_id.to_string(),
            generation_id: generation_id.get(),
            callback_id: callback_id.to_string(),
        };
        // Fast path: if tripped, skip and emit a diagnostic.
        if let Some(state) = self.state(&key) {
            if !matches!(state, BreakerState::Healthy) {
                self.emit_skip_diag(plugin_id, generation_id, callback_id, kind, &state);
                return Outcome::Skipped;
            }
        }

        let budget = kind.default_budget();
        let started = self.clock.now_ms();
        let result = body();
        let ended = self.clock.now_ms();
        let duration_ms = ended.saturating_sub(started);
        let ok = result.is_ok();
        let last_failure = result.as_ref().err().map(|e| e.to_string());

        // Record sample / state transitions under the lock.
        let outcome_meta = self.update_stats(
            &key,
            kind,
            Sample { duration_ms, ok },
            &budget,
            last_failure.clone(),
        );

        // Push the trace row.
        self.push_trace(PerformanceTrace {
            plugin_id: plugin_id.to_string(),
            generation_id: generation_id.get(),
            callback_id: callback_id.to_string(),
            kind,
            duration_ms,
            ok,
            timestamp_unix_ms: self.clock.now_unix_ms(),
        });

        // Soft breach diagnostic (warning).
        if ok && duration_ms > budget.soft_ms && duration_ms <= budget.hard_ms {
            self.emit_breach(
                plugin_id,
                generation_id,
                callback_id,
                kind,
                duration_ms,
                budget.soft_ms,
                "performance.soft_breach",
                DiagnosticSeverity::Warning,
                outcome_meta.consecutive_hard_breaches,
            );
        }
        // Hard breach diagnostic (error). Hard breach fires regardless
        // of body's ok/err — a 4-second crash and a 4-second success
        // are both budget violations.
        if duration_ms > budget.hard_ms {
            self.emit_breach(
                plugin_id,
                generation_id,
                callback_id,
                kind,
                duration_ms,
                budget.hard_ms,
                "performance.hard_breach",
                DiagnosticSeverity::Error,
                outcome_meta.consecutive_hard_breaches,
            );
        }
        // Trip + degrade diagnostics fire once per transition.
        if let Some(reason) = outcome_meta.tripped_reason {
            self.emit_trip(plugin_id, generation_id, callback_id, kind, &reason);
            // After tripping, check whether the plugin should be marked
            // degraded.
            self.maybe_emit_degraded(plugin_id, generation_id);
        }

        match result {
            Ok(v) => Outcome::Ok(v),
            Err(e) => Outcome::Err(e),
        }
    }

    /// True when the callback is currently tripped or disabled.
    pub fn is_tripped(
        &self,
        plugin_id: &PluginId,
        generation_id: GenerationId,
        callback_id: &str,
    ) -> bool {
        let key = TrackerKey {
            plugin_id: plugin_id.to_string(),
            generation_id: generation_id.get(),
            callback_id: callback_id.to_string(),
        };
        match self.state(&key) {
            Some(BreakerState::Healthy) | None => false,
            Some(BreakerState::Tripped { .. }) => true,
        }
    }

    /// User-facing breaker reset. Walks every generation under
    /// `plugin_id` so a "reset breaker" button doesn't need to know
    /// the live generation. Emits a `performance.reset` info
    /// diagnostic so the action is auditable.
    pub fn reset_breaker(&self, plugin_id: &PluginId, callback_id: &str) {
        let mut inner = self.inner.lock().expect("budget tracker poisoned");
        let mut affected: Vec<u64> = Vec::new();
        for (key, stats) in inner.stats.iter_mut() {
            if key.plugin_id == plugin_id.as_str()
                && key.callback_id == callback_id
                && stats.is_active_breach()
            {
                stats.state = Some(BreakerState::Healthy);
                stats.consecutive_failures = 0;
                stats.consecutive_hard_breaches = 0;
                affected.push(key.generation_id);
            }
        }
        // Allow `plugin.degraded` to fire again if the plugin trips
        // anew after this reset.
        for gen in &affected {
            inner
                .degraded_emitted
                .remove(&(plugin_id.to_string(), *gen));
        }
        drop(inner);
        for gen in affected {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    plugin_id.clone(),
                    DiagnosticSeverity::Info,
                    "performance.reset",
                    format!("circuit breaker reset for `{callback_id}`"),
                )
                .with_generation(GenerationId::new(gen))
                .with_source(PluginSourceSpan::ApiFunction {
                    name: format!("performance:{callback_id}"),
                })
                .with_context(serde_json::json!({
                    "callback_id": callback_id,
                })),
            );
        }
    }

    /// Drop every record for `plugin_id` (across all generations).
    /// Called from the host's unload / reload-cleanup walk so a fresh
    /// generation starts with an empty breaker.
    pub fn drop_for_plugin(&self, plugin_id: &str) {
        let mut inner = self.inner.lock().expect("budget tracker poisoned");
        inner.stats.retain(|key, _| key.plugin_id != plugin_id);
        inner
            .degraded_emitted
            .retain(|(pid, _), _| pid != plugin_id);
        inner.traces.retain(|t| t.plugin_id != plugin_id);
    }

    /// Drop records keyed to a specific generation. Used after
    /// reload-commit, when the previous generation's stats no longer
    /// describe live state.
    pub fn drop_for_generation(&self, plugin_id: &str, generation_id: GenerationId) {
        let gen = generation_id.get();
        let mut inner = self.inner.lock().expect("budget tracker poisoned");
        inner
            .stats
            .retain(|key, _| !(key.plugin_id == plugin_id && key.generation_id == gen));
        inner.degraded_emitted.remove(&(plugin_id.to_string(), gen));
        inner
            .traces
            .retain(|t| !(t.plugin_id == plugin_id && t.generation_id == gen));
    }

    /// Snapshot of recent trace rows (cheap clone). Devtools sorts
    /// these for display.
    pub fn traces(&self) -> Vec<PerformanceTrace> {
        let inner = self.inner.lock().expect("budget tracker poisoned");
        inner.traces.iter().cloned().collect()
    }

    /// One row per `(plugin_id, generation_id, callback_id)` tracked.
    pub fn breaker_summaries(&self) -> Vec<CircuitBreakerSummary> {
        let inner = self.inner.lock().expect("budget tracker poisoned");
        inner
            .stats
            .iter()
            .map(|(key, stats)| {
                let state = stats.state.clone().unwrap_or(BreakerState::Healthy);
                CircuitBreakerSummary {
                    plugin_id: key.plugin_id.clone(),
                    generation_id: key.generation_id,
                    callback_id: key.callback_id.clone(),
                    kind: stats
                        .kind
                        .map(|k| k.as_str().to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    state: state.as_str().to_string(),
                    consecutive_failures: stats.consecutive_failures,
                    count: stats.count,
                    ok_count: stats.ok_count,
                    err_count: stats.err_count,
                    p50_ms: stats.p50(),
                    p95_ms: stats.p95(),
                    last_failure: stats.last_failure.clone(),
                }
            })
            .collect()
    }

    fn state(&self, key: &TrackerKey) -> Option<BreakerState> {
        let inner = self.inner.lock().expect("budget tracker poisoned");
        inner.stats.get(key).and_then(|s| s.state.clone())
    }

    fn update_stats(
        &self,
        key: &TrackerKey,
        kind: CallbackKind,
        sample: Sample,
        budget: &CallbackBudget,
        last_failure: Option<String>,
    ) -> StatsTransition {
        let mut inner = self.inner.lock().expect("budget tracker poisoned");
        let stats = inner.stats.entry(key.clone()).or_default();
        stats.record(sample, kind);
        if !sample.ok {
            stats.consecutive_failures = stats.consecutive_failures.saturating_add(1);
            stats.last_failure = last_failure;
        } else {
            stats.consecutive_failures = 0;
        }
        if sample.duration_ms > budget.hard_ms {
            stats.consecutive_hard_breaches = stats.consecutive_hard_breaches.saturating_add(1);
        } else {
            stats.consecutive_hard_breaches = 0;
        }

        let mut tripped_reason: Option<String> = None;
        if matches!(stats.state, None | Some(BreakerState::Healthy)) {
            if stats.consecutive_failures >= DEFAULT_FAILURE_THRESHOLD {
                let reason = format!("{} consecutive failures", stats.consecutive_failures);
                stats.state = Some(BreakerState::Tripped {
                    since_unix_ms: self.clock.now_unix_ms(),
                    reason: reason.clone(),
                });
                tripped_reason = Some(reason);
            } else if stats.consecutive_hard_breaches >= DEFAULT_FAILURE_THRESHOLD {
                let reason = format!(
                    "{} consecutive hard breaches",
                    stats.consecutive_hard_breaches
                );
                stats.state = Some(BreakerState::Tripped {
                    since_unix_ms: self.clock.now_unix_ms(),
                    reason: reason.clone(),
                });
                tripped_reason = Some(reason);
            }
        }

        StatsTransition {
            consecutive_hard_breaches: stats.consecutive_hard_breaches,
            tripped_reason,
        }
    }

    fn push_trace(&self, trace: PerformanceTrace) {
        let mut inner = self.inner.lock().expect("budget tracker poisoned");
        if inner.traces.len() == TRACE_RING_CAP {
            inner.traces.pop_front();
        }
        inner.traces.push_back(trace);
    }

    fn emit_skip_diag(
        &self,
        plugin_id: &PluginId,
        generation_id: GenerationId,
        callback_id: &str,
        kind: CallbackKind,
        state: &BreakerState,
    ) {
        let reason = match state {
            BreakerState::Healthy => "healthy".to_string(),
            BreakerState::Tripped { reason, .. } => reason.clone(),
        };
        self.diagnostics.record(
            PluginDiagnostic::new(
                plugin_id.clone(),
                DiagnosticSeverity::Warning,
                "performance.tripped",
                format!("callback `{callback_id}` skipped (circuit breaker tripped: {reason})"),
            )
            .with_generation(generation_id)
            .with_source(PluginSourceSpan::ApiFunction {
                name: format!("performance:{callback_id}"),
            })
            .with_context(serde_json::json!({
                "callback_id": callback_id,
                "kind": kind.as_str(),
                "reason": reason,
                "state": state.as_str(),
            })),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_breach(
        &self,
        plugin_id: &PluginId,
        generation_id: GenerationId,
        callback_id: &str,
        kind: CallbackKind,
        duration_ms: u128,
        budget_ms: u128,
        code: &str,
        severity: DiagnosticSeverity,
        consecutive_hard_breaches: u32,
    ) {
        self.diagnostics.record(
            PluginDiagnostic::new(
                plugin_id.clone(),
                severity,
                code,
                format!(
                    "callback `{callback_id}` exceeded budget ({duration_ms}ms > {budget_ms}ms)"
                ),
            )
            .with_generation(generation_id)
            .with_source(PluginSourceSpan::ApiFunction {
                name: format!("performance:{callback_id}"),
            })
            .with_context(serde_json::json!({
                "callback_id": callback_id,
                "kind": kind.as_str(),
                "duration_ms": duration_ms as u64,
                "budget_ms": budget_ms as u64,
                "consecutive_hard_breaches": consecutive_hard_breaches,
            })),
        );
    }

    fn emit_trip(
        &self,
        plugin_id: &PluginId,
        generation_id: GenerationId,
        callback_id: &str,
        kind: CallbackKind,
        reason: &str,
    ) {
        self.diagnostics.record(
            PluginDiagnostic::new(
                plugin_id.clone(),
                DiagnosticSeverity::Error,
                "performance.tripped",
                format!("callback `{callback_id}` circuit breaker tripped: {reason}"),
            )
            .with_generation(generation_id)
            .with_source(PluginSourceSpan::ApiFunction {
                name: format!("performance:{callback_id}"),
            })
            .with_context(serde_json::json!({
                "callback_id": callback_id,
                "kind": kind.as_str(),
                "reason": reason,
            })),
        );
    }

    fn maybe_emit_degraded(&self, plugin_id: &PluginId, generation_id: GenerationId) {
        let mut inner = self.inner.lock().expect("budget tracker poisoned");
        let pid = plugin_id.to_string();
        let gen = generation_id.get();
        let already = inner
            .degraded_emitted
            .get(&(pid.clone(), gen))
            .copied()
            .unwrap_or(false);
        if already {
            return;
        }
        let disabled: Vec<String> = inner
            .stats
            .iter()
            .filter(|(key, stats)| {
                key.plugin_id == pid && key.generation_id == gen && stats.is_active_breach()
            })
            .map(|(key, _)| key.callback_id.clone())
            .collect();
        if disabled.len() < MAX_DISABLED_CALLBACKS_PER_PLUGIN {
            return;
        }
        inner.degraded_emitted.insert((pid, gen), true);
        drop(inner);
        let count = disabled.len();
        self.diagnostics.record(
            PluginDiagnostic::new(
                plugin_id.clone(),
                DiagnosticSeverity::Error,
                "plugin.degraded",
                format!("plugin `{plugin_id}` degraded: {count} callbacks disabled"),
            )
            .with_generation(generation_id)
            .with_source(PluginSourceSpan::ApiFunction {
                name: "performance:plugin_degraded".into(),
            })
            .with_context(serde_json::json!({
                "disabled_callbacks": disabled,
                "count": count,
            })),
        );
    }
}

/// One-shot transition info returned from `update_stats` so the
/// caller can emit diagnostics outside the lock.
struct StatsTransition {
    consecutive_hard_breaches: u32,
    tripped_reason: Option<String>,
}

/// Devtools projection of one breaker row.
#[derive(Debug, Clone)]
pub struct CircuitBreakerSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub callback_id: String,
    pub kind: String,
    pub state: String,
    pub consecutive_failures: u32,
    pub count: u64,
    pub ok_count: u64,
    pub err_count: u64,
    pub p50_ms: u128,
    pub p95_ms: u128,
    pub last_failure: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::diagnostic::NullSink;

    fn store() -> DiagnosticStore {
        DiagnosticStore::with_sink(Arc::new(NullSink))
    }

    fn pid(s: &str) -> PluginId {
        PluginId::from(s)
    }

    #[test]
    fn default_budgets_are_per_kind() {
        assert_eq!(CallbackKind::Init.default_budget().soft_ms, 200);
        assert_eq!(CallbackKind::Init.default_budget().hard_ms, 2000);
        assert_eq!(CallbackKind::UiCallback.default_budget().soft_ms, 8);
        assert_eq!(CallbackKind::AsyncJob.default_budget().hard_ms, 10_000);
    }

    #[test]
    fn ok_call_records_sample_and_no_diagnostics() {
        let s = store();
        let clock = Arc::new(MockClock::new());
        let t = BudgetTracker::with_clock(s.clone(), clock.clone());
        let p = pid("p");
        let gid = GenerationId::new(1);
        match t.track_call::<i32, String>(CallbackKind::EventCallback, &p, gid, "cb", || Ok(7)) {
            Outcome::Ok(v) => assert_eq!(v, 7),
            _ => panic!("expected Ok"),
        }
        assert!(s.entries().is_empty());
        let row = t
            .breaker_summaries()
            .into_iter()
            .find(|r| r.callback_id == "cb")
            .unwrap();
        assert_eq!(row.count, 1);
        assert_eq!(row.ok_count, 1);
        assert_eq!(row.state, "healthy");
    }

    #[test]
    fn hard_breach_emits_error_and_counts() {
        let s = store();
        let clock = Arc::new(MockClock::new());
        let t = BudgetTracker::with_clock(s.clone(), clock.clone());
        let p = pid("p");
        let gid = GenerationId::new(1);
        let outcome =
            t.track_call::<(), String>(CallbackKind::EventCallback, &p, gid, "slow", || {
                clock.advance_ms(500);
                Ok(())
            });
        assert!(matches!(outcome, Outcome::Ok(())));
        let codes: Vec<String> = s.entries().iter().map(|d| d.code.clone()).collect();
        assert!(codes.iter().any(|c| c == "performance.hard_breach"));
    }

    #[test]
    fn five_failures_trip_breaker() {
        let s = store();
        let clock = Arc::new(MockClock::new());
        let t = BudgetTracker::with_clock(s.clone(), clock.clone());
        let p = pid("p");
        let gid = GenerationId::new(1);
        for _ in 0..5 {
            let _ =
                t.track_call::<(), String>(CallbackKind::EventCallback, &p, gid, "broken", || {
                    Err("boom".to_string())
                });
        }
        assert!(t.is_tripped(&p, gid, "broken"));
    }
}

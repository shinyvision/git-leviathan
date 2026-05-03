//! Phase 18 acceptance tests: performance budgets and circuit breakers.
//!
//! Acceptance gates exercised here:
//! - a slow-but-successful callback produces a `performance.soft_breach`
//!   warning;
//! - a very slow callback produces a `performance.hard_breach` error;
//! - five consecutive failures trip the per-callback circuit breaker;
//! - subsequent invocations of a tripped callback are skipped (a
//!   `performance.tripped` diagnostic fires; the body never runs);
//! - `host.reset_breaker(plugin, callback)` restores the callback
//!   (`performance.reset` info diagnostic);
//! - three disabled callbacks on one plugin generation emit a
//!   `plugin.degraded` error diagnostic;
//! - `InspectorSnapshot::performance_traces` and
//!   `InspectorSnapshot::circuit_breakers` project through verbatim;
//! - unload drops every breaker / trace row for the plugin.

use std::sync::Arc;

use crate::plugin::diagnostic::{DiagnosticStore, NullSink};
use crate::plugin::performance::{BudgetTracker, CallbackKind, MockClock, Outcome as PerfOutcome};
use crate::plugin::resources::{GenerationId, PluginId};
use crate::plugin::tests::harness::MockHost;

fn store() -> DiagnosticStore {
    DiagnosticStore::with_sink(Arc::new(NullSink))
}

#[test]
fn slow_callback_emits_soft_breach() {
    let s = store();
    let clock = Arc::new(MockClock::new());
    let t = BudgetTracker::with_clock(s.clone(), clock.clone());
    let pid = PluginId::from("p_soft");
    let gen = GenerationId::new(1);
    // Event budget = soft 20ms / hard 250ms. Advance 30ms so it
    // breaches soft but not hard.
    let outcome = t.track_call::<(), String>(CallbackKind::EventCallback, &pid, gen, "cb", || {
        clock.advance_ms(30);
        Ok(())
    });
    assert!(matches!(outcome, PerfOutcome::Ok(())));
    let codes: Vec<String> = s.entries().iter().map(|d| d.code.clone()).collect();
    assert!(
        codes.iter().any(|c| c == "performance.soft_breach"),
        "expected soft breach, got: {codes:?}"
    );
    assert!(
        !codes.iter().any(|c| c == "performance.hard_breach"),
        "should NOT hard-breach"
    );
}

#[test]
fn very_slow_callback_emits_hard_breach() {
    let s = store();
    let clock = Arc::new(MockClock::new());
    let t = BudgetTracker::with_clock(s.clone(), clock.clone());
    let pid = PluginId::from("p_hard");
    let gen = GenerationId::new(1);
    let outcome = t.track_call::<(), String>(CallbackKind::EventCallback, &pid, gen, "cb", || {
        clock.advance_ms(500);
        Ok(())
    });
    assert!(matches!(outcome, PerfOutcome::Ok(())));
    let codes: Vec<String> = s.entries().iter().map(|d| d.code.clone()).collect();
    assert!(
        codes.iter().any(|c| c == "performance.hard_breach"),
        "expected hard breach, got: {codes:?}"
    );
}

#[test]
fn repeated_failures_trip_breaker() {
    let s = store();
    let clock = Arc::new(MockClock::new());
    let t = BudgetTracker::with_clock(s.clone(), clock.clone());
    let pid = PluginId::from("p_trip");
    let gen = GenerationId::new(1);
    for _ in 0..5 {
        let _ =
            t.track_call::<(), String>(CallbackKind::EventCallback, &pid, gen, "broken", || {
                Err("boom".to_string())
            });
    }
    assert!(t.is_tripped(&pid, gen, "broken"));
    let codes: Vec<String> = s.entries().iter().map(|d| d.code.clone()).collect();
    assert!(
        codes.iter().any(|c| c == "performance.tripped"),
        "expected trip diagnostic, got: {codes:?}"
    );
    let breakers = t.breaker_summaries();
    let row = breakers
        .iter()
        .find(|b| b.callback_id == "broken")
        .expect("breaker row");
    assert_eq!(row.state, "tripped");
    assert_eq!(row.consecutive_failures, 5);
    assert_eq!(row.err_count, 5);
    assert_eq!(row.last_failure.as_deref(), Some("boom"));
}

#[test]
fn tripped_breaker_skips_subsequent_calls() {
    let s = store();
    let clock = Arc::new(MockClock::new());
    let t = BudgetTracker::with_clock(s.clone(), clock.clone());
    let pid = PluginId::from("p_skip");
    let gen = GenerationId::new(1);
    // Trip the breaker.
    for _ in 0..5 {
        let _ = t.track_call::<(), String>(CallbackKind::EventCallback, &pid, gen, "cb", || {
            Err("nope".to_string())
        });
    }
    assert!(t.is_tripped(&pid, gen, "cb"));
    // Counter should NOT advance for the skipped call.
    let count_before = t
        .breaker_summaries()
        .iter()
        .find(|b| b.callback_id == "cb")
        .map(|b| b.count)
        .unwrap_or(0);
    let mut ran = false;
    let outcome = t.track_call::<(), String>(CallbackKind::EventCallback, &pid, gen, "cb", || {
        ran = true;
        Ok(())
    });
    assert!(matches!(outcome, PerfOutcome::Skipped));
    assert!(!ran, "tripped breaker must skip body");
    let count_after = t
        .breaker_summaries()
        .iter()
        .find(|b| b.callback_id == "cb")
        .map(|b| b.count)
        .unwrap_or(0);
    assert_eq!(
        count_before, count_after,
        "skipped call must not record a sample"
    );
}

#[test]
fn reset_breaker_restores_callback() {
    let s = store();
    let clock = Arc::new(MockClock::new());
    let t = BudgetTracker::with_clock(s.clone(), clock.clone());
    let pid = PluginId::from("p_reset");
    let gen = GenerationId::new(1);
    for _ in 0..5 {
        let _ = t.track_call::<(), String>(CallbackKind::EventCallback, &pid, gen, "cb", || {
            Err("x".to_string())
        });
    }
    assert!(t.is_tripped(&pid, gen, "cb"));
    t.reset_breaker(&pid, "cb");
    assert!(!t.is_tripped(&pid, gen, "cb"));
    let codes: Vec<String> = s.entries().iter().map(|d| d.code.clone()).collect();
    assert!(
        codes.iter().any(|c| c == "performance.reset"),
        "expected reset diagnostic, got: {codes:?}"
    );
    // After reset, body runs again.
    let outcome =
        t.track_call::<(), String>(CallbackKind::EventCallback, &pid, gen, "cb", || Ok(()));
    assert!(matches!(outcome, PerfOutcome::Ok(())));
}

#[test]
fn multiple_disabled_callbacks_degrade_plugin() {
    let s = store();
    let clock = Arc::new(MockClock::new());
    let t = BudgetTracker::with_clock(s.clone(), clock.clone());
    let pid = PluginId::from("p_degrade");
    let gen = GenerationId::new(1);
    // Trip three different callbacks.
    for cb in &["cb_a", "cb_b", "cb_c"] {
        for _ in 0..5 {
            let _ = t.track_call::<(), String>(CallbackKind::EventCallback, &pid, gen, cb, || {
                Err("boom".to_string())
            });
        }
    }
    let codes: Vec<String> = s.entries().iter().map(|d| d.code.clone()).collect();
    assert!(
        codes.iter().any(|c| c == "plugin.degraded"),
        "expected plugin.degraded, got: {codes:?}"
    );
    // Trip a fourth — degraded fires once per generation only.
    for _ in 0..5 {
        let _ = t.track_call::<(), String>(CallbackKind::EventCallback, &pid, gen, "cb_d", || {
            Err("boom".to_string())
        });
    }
    let codes2: Vec<String> = s.entries().iter().map(|d| d.code.clone()).collect();
    let deg_count = codes2.iter().filter(|c| *c == "plugin.degraded").count();
    assert_eq!(deg_count, 1, "plugin.degraded fires once per generation");
}

#[test]
fn performance_traces_appear_in_devtools() {
    let mut host = MockHost::new();
    let _clock = host.install_mock_clock();
    host.load_inline(
        "p_trace",
        r#"
id = "p_trace"
name = "p_trace"
version = "0.1.0"
api_version = "1.0"
"#,
        r#"
        leviathan.autocmd.create("BranchChanged", {
            callback = function(_) _G.fired = true end,
        })
        "#,
    )
    .expect("load");
    // Fire the event so the autocmd dispatches.
    host.dispatch_test_event(
        "BranchChanged",
        serde_json::json!({"old": "main", "new": "dev"}),
    );
    let snap = host.introspect();
    // At least the init.lua trace + the event callback trace should
    // be visible.
    assert!(
        snap.performance_traces
            .iter()
            .any(|t| t.callback_id == "init.lua"),
        "expected init.lua trace, got: {:?}",
        snap.performance_traces
            .iter()
            .map(|t| t.callback_id.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        snap.performance_traces
            .iter()
            .any(|t| t.callback_id.starts_with("autocmd:")),
        "expected event callback trace"
    );
}

#[test]
fn circuit_breaker_state_in_devtools() {
    let mut host = MockHost::new();
    let _clock = host.install_mock_clock();
    host.load_inline(
        "p_cb",
        r#"
id = "p_cb"
name = "p_cb"
version = "0.1.0"
api_version = "1.0"
"#,
        "-- nothing fancy",
    )
    .expect("load");
    // Trip a synthetic breaker through the tracker handle so we can
    // exercise the devtools projection without relying on a real Lua
    // failure.
    let tracker = host.budget_tracker();
    let pid = PluginId::from("p_cb");
    let gen = GenerationId::new(1);
    for _ in 0..5 {
        let _ = tracker.track_call::<(), String>(
            CallbackKind::CommandCallback,
            &pid,
            gen,
            "cmd:explode",
            || Err("crash".to_string()),
        );
    }
    let snap = host.introspect();
    let row = snap
        .circuit_breakers
        .iter()
        .find(|b| b.plugin_id == "p_cb" && b.callback_id == "cmd:explode")
        .expect("breaker row");
    assert_eq!(row.state, "tripped");
    assert_eq!(row.kind, "command");
    assert_eq!(row.consecutive_failures, 5);
    assert_eq!(row.err_count, 5);
    assert_eq!(row.last_failure.as_deref(), Some("crash"));
}

#[test]
fn unload_plugin_drops_breaker_state() {
    let mut host = MockHost::new();
    let _clock = host.install_mock_clock();
    host.load_inline(
        "p_unload",
        r#"
id = "p_unload"
name = "p_unload"
version = "0.1.0"
api_version = "1.0"
"#,
        "-- bare",
    )
    .expect("load");
    let tracker = host.budget_tracker();
    let pid = PluginId::from("p_unload");
    let gen = GenerationId::new(1);
    for _ in 0..5 {
        let _ = tracker.track_call::<(), String>(
            CallbackKind::EventCallback,
            &pid,
            gen,
            "cb_x",
            || Err("oops".to_string()),
        );
    }
    let pre = host.introspect();
    assert!(
        pre.circuit_breakers
            .iter()
            .any(|b| b.plugin_id == "p_unload"),
        "breaker row present before unload"
    );
    host.unload_plugin("p_unload").expect("unload");
    let post = host.introspect();
    assert!(
        post.circuit_breakers
            .iter()
            .all(|b| b.plugin_id != "p_unload"),
        "all breaker rows for the plugin must be dropped"
    );
    assert!(
        post.performance_traces
            .iter()
            .all(|t| t.plugin_id != "p_unload"),
        "all trace rows for the plugin must be dropped"
    );
}

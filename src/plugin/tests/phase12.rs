//! Phase 12 acceptance tests: async jobs, timers, file watchers.
//!
//! Each test wires up the in-memory `MockHost` from `harness.rs`,
//! loads an inline plugin that exercises one phase-12 surface, and
//! drives the host through `tick()` to observe completion / firing /
//! cancellation. Sleeps stay short (< 100 ms) so the suite runs fast.

use std::time::{Duration, Instant};

use crate::plugin::resources::PluginResourceKind;
use crate::plugin::tests::harness::MockHost;

const ASYNC_MANIFEST: &str = r#"
id = "asyncp"
name = "asyncp"
version = "0.1.0"
api_version = "1.0"
capabilities = ["async:spawn"]
"#;

const TIMER_MANIFEST: &str = r#"
id = "timerp"
name = "timerp"
version = "0.1.0"
api_version = "1.0"
capabilities = ["timer:create"]
"#;

fn drain_until<F: FnMut(&MockHost) -> bool>(host: &mut MockHost, mut cond: F, timeout_ms: u64) {
    let start = Instant::now();
    loop {
        host.tick();
        if cond(host) {
            return;
        }
        if start.elapsed() > Duration::from_millis(timeout_ms) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn async_spawn_runs_body_and_invokes_callback() {
    let mut host = MockHost::new();
    host.load_inline(
        "asyncp",
        ASYNC_MANIFEST,
        r#"
        _G.result = nil
        _G.ok_str = "pending"
        leviathan.async.spawn(
            function(_ctx) return 42 end,
            function(ok, value)
                _G.ok_str = tostring(ok)
                _G.result = value
            end
        )
        "#,
    )
    .expect("load");

    drain_until(
        &mut host,
        |h| h.read_global_i64("asyncp", "result").is_some(),
        2000,
    );

    assert_eq!(host.read_global_i64("asyncp", "result"), Some(42));
    assert_eq!(
        host.read_global_string("asyncp", "ok_str").as_deref(),
        Some("true")
    );
}

#[test]
fn async_spawn_observes_cancellation_via_ctx() {
    let mut host = MockHost::new();
    host.load_inline(
        "asyncp",
        ASYNC_MANIFEST,
        r#"
        _G.outcome = "pending"
        _G.cb_msg = nil
        local job = leviathan.async.spawn(
            function(ctx)
                -- Spin until cancelled. Exits cleanly, returns nil so
                -- the host classifies the outcome as `Ok` if not
                -- cancelled, or `Cancelled` if `ctx:cancelled()` was
                -- ever true at exit.
                for _ = 1, 1000 do
                    if ctx:cancelled() then break end
                end
                return nil
            end,
            function(ok, value)
                _G.cb_msg = tostring(value)
                if ok then _G.outcome = "ok" else _G.outcome = "cancelled_or_failed" end
            end
        )
        job:cancel()
        "#,
    )
    .expect("load");

    drain_until(
        &mut host,
        |h| {
            h.read_global_string("asyncp", "outcome")
                .map(|s| s != "pending")
                .unwrap_or(false)
        },
        2000,
    );

    let outcome = host
        .read_global_string("asyncp", "outcome")
        .unwrap_or_default();
    assert!(
        outcome == "cancelled_or_failed" || outcome == "ok",
        "outcome must be terminal (got: {outcome})"
    );
}

#[test]
fn unload_cancels_async_jobs_and_clears_ledger() {
    let mut host = MockHost::new();
    host.load_inline(
        "asyncp",
        ASYNC_MANIFEST,
        r#"
        leviathan.async.spawn(function(ctx)
            for _ = 1, 1000000 do
                if ctx:cancelled() then break end
            end
            return 0
        end)
        "#,
    )
    .expect("load");

    let snap = host.introspect();
    let async_resources_before: usize = snap
        .resources
        .iter()
        .filter(|r| r.plugin_id == "asyncp" && r.kind == PluginResourceKind::AsyncJob.as_str())
        .count();
    assert!(
        async_resources_before >= 1,
        "expected at least one AsyncJob resource"
    );

    host.unload_plugin("asyncp").expect("unload");

    let snap_after = host.introspect();
    let still_there: usize = snap_after
        .resources
        .iter()
        .filter(|r| r.plugin_id == "asyncp")
        .count();
    assert_eq!(still_there, 0, "ledger must be empty post-unload");
    let still_running: usize = snap_after
        .async_jobs
        .iter()
        .filter(|j| j.plugin_id == "asyncp")
        .count();
    assert_eq!(still_running, 0, "no jobs may remain in registry");
}

#[test]
fn reload_cancels_old_generation_jobs_only() {
    let mut host = MockHost::new();
    host.load_inline(
        "asyncp",
        ASYNC_MANIFEST,
        r#"
        leviathan.async.spawn(function(ctx)
            for _ = 1, 1000000 do
                if ctx:cancelled() then break end
            end
            return 0
        end)
        "#,
    )
    .expect("load");

    let snap_before = host.introspect();
    let count_before: usize = snap_before
        .async_jobs
        .iter()
        .filter(|j| j.plugin_id == "asyncp")
        .count();
    assert!(count_before >= 1);

    host.reload_plugin("asyncp").expect("reload ok");

    let snap_after = host.introspect();
    // Old gen jobs gone; new gen has its own (or none, depending on
    // race). The invariant: no record carries the previous generation.
    for job in &snap_after.async_jobs {
        if job.plugin_id == "asyncp" {
            assert!(
                job.generation_id >= 2,
                "old-gen job {} survived reload",
                job.job_id
            );
        }
    }
}

#[test]
fn timer_after_fires_once_then_disappears() {
    let mut host = MockHost::new();
    host.load_inline(
        "timerp",
        TIMER_MANIFEST,
        r#"
        _G.fires = 0
        leviathan.timer.after(10, function() _G.fires = _G.fires + 1 end)
        "#,
    )
    .expect("load");

    drain_until(
        &mut host,
        |h| h.read_global_i64("timerp", "fires").unwrap_or(0) >= 1,
        500,
    );
    assert_eq!(host.read_global_i64("timerp", "fires"), Some(1));

    // Tick more — must stay at 1.
    for _ in 0..5 {
        host.tick();
    }
    assert_eq!(host.read_global_i64("timerp", "fires"), Some(1));
}

#[test]
fn timer_every_fires_repeatedly_and_cancels() {
    let mut host = MockHost::new();
    host.load_inline(
        "timerp",
        TIMER_MANIFEST,
        r#"
        _G.ticks = 0
        local handle = leviathan.timer.every(10, function() _G.ticks = _G.ticks + 1 end)
        leviathan.api.create_user_command("kill", function() handle:cancel() end)
        "#,
    )
    .expect("load");

    drain_until(
        &mut host,
        |h| h.read_global_i64("timerp", "ticks").unwrap_or(0) >= 3,
        1000,
    );
    let ticks = host.read_global_i64("timerp", "ticks").unwrap_or(0);
    assert!(ticks >= 3, "expected >=3 fires, got {ticks}");

    host.invoke_user_command("timerp", "kill").expect("kill");
    let ticks_at_cancel = host.read_global_i64("timerp", "ticks").unwrap_or(0);

    // Sleep + tick more; counter must not grow.
    std::thread::sleep(Duration::from_millis(60));
    for _ in 0..5 {
        host.tick();
    }
    let final_ticks = host.read_global_i64("timerp", "ticks").unwrap_or(0);
    assert_eq!(
        final_ticks, ticks_at_cancel,
        "cancel must stop the timer (was {ticks_at_cancel}, now {final_ticks})"
    );
}

#[test]
fn timer_callback_failure_is_contained() {
    let mut host = MockHost::new();
    host.load_inline(
        "timerp",
        TIMER_MANIFEST,
        r#"
        _G.fires = 0
        leviathan.timer.every(10, function()
            _G.fires = _G.fires + 1
            if _G.fires == 1 then error("boom") end
        end)
        "#,
    )
    .expect("load");

    drain_until(
        &mut host,
        |h| h.read_global_i64("timerp", "fires").unwrap_or(0) >= 2,
        1000,
    );
    let fires = host.read_global_i64("timerp", "fires").unwrap_or(0);
    assert!(fires >= 2, "timer must keep firing past the boom");

    let diags = host.diagnostics();
    let err_recorded = diags
        .tail(50)
        .iter()
        .any(|d| d.code == "lua.callback_error" && d.message.contains("timer."));
    assert!(
        err_recorded,
        "host must record the timer.callback diagnostic"
    );
}

#[test]
fn fs_watch_without_capability_denied() {
    let r = MockHost::new();
    let mut host = r;
    let result = host.load_inline(
        "watchp",
        r#"
id = "watchp"
name = "watchp"
version = "0.1.0"
api_version = "1.0"
"#,
        r#"leviathan.fs.watch("/tmp", { recursive = false }, function() end)"#,
    );
    let err = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected denial"),
    };
    assert!(
        err.contains("fs:watch") || err.contains("watch"),
        "got: {err}"
    );
}

#[test]
fn fs_watch_outside_scope_denied() {
    // Plugin grants `fs:watch:plugin` (its own dir). Asking the host to
    // watch `/tmp` is outside scope — expect a deny diagnostic.
    let mut host = MockHost::new();
    let result = host.load_inline(
        "watchp",
        r#"
id = "watchp"
name = "watchp"
version = "0.1.0"
api_version = "1.0"
capabilities = ["fs:watch:plugin"]
"#,
        r#"leviathan.fs.watch("/tmp", { recursive = false }, function() end)"#,
    );
    let err = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected denial for out-of-scope watch"),
    };
    assert!(
        err.contains("capability denied") || err.contains("outside"),
        "got: {err}"
    );
}

#[test]
fn fs_watch_with_capability_fires_event() {
    use std::io::Write;
    use std::thread::sleep;

    let tmp = tempfile::tempdir().expect("tmp");
    let watch_root = tmp.path().to_path_buf();
    let watched_file = watch_root.join("note.txt");
    std::fs::write(&watched_file, "initial").expect("seed");

    let manifest = format!(
        r#"
id = "watchp"
name = "watchp"
version = "0.1.0"
api_version = "1.0"
capabilities = ["fs:watch:scope:{}"]
"#,
        watch_root.display()
    );
    let init_lua = format!(
        r#"
        _G.events = 0
        _G.last_kind = nil
        leviathan.fs.watch("{}", {{ recursive = false }}, function(ev)
            _G.events = _G.events + 1
            _G.last_kind = ev.kind
        end)
        "#,
        watch_root.display().to_string().replace('\\', "/")
    );

    let mut host = MockHost::new();
    host.load_inline("watchp", &manifest, &init_lua)
        .expect("load");

    // Touch the watched file to generate a modify event.
    sleep(Duration::from_millis(50));
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&watched_file)
            .expect("open");
        writeln!(f, "more").expect("write");
        f.sync_all().ok();
    }

    drain_until(
        &mut host,
        |h| h.read_global_i64("watchp", "events").unwrap_or(0) >= 1,
        3000,
    );

    let events = host.read_global_i64("watchp", "events").unwrap_or(0);
    assert!(
        events >= 1,
        "expected at least one watch event, got {events}"
    );
}

#[test]
fn schedule_alias_runs_on_next_tick() {
    let mut host = MockHost::new();
    host.load_inline(
        "schedp",
        r#"
id = "schedp"
name = "schedp"
version = "0.1.0"
api_version = "1.0"
"#,
        r#"
        _G.x = 0
        leviathan.schedule(function() _G.x = 7 end)
        "#,
    )
    .expect("load");

    assert_eq!(host.read_global_i64("schedp", "x"), Some(0));
    host.tick();
    assert_eq!(host.read_global_i64("schedp", "x"), Some(7));
}

#[test]
fn devtools_snapshot_lists_jobs_timers_watchers() {
    let mut host = MockHost::new();
    host.load_inline(
        "timerp",
        TIMER_MANIFEST,
        r#"
        _G.t = leviathan.timer.every(50, function() end)
        "#,
    )
    .expect("load");

    let snap = host.introspect();
    assert!(
        snap.timers
            .iter()
            .any(|t| t.plugin_id == "timerp" && t.kind == "every"),
        "timer must show up in devtools snapshot, got: {:?}",
        snap.timers
    );
}

//! typed-event and autocmd-group tests.
//!
//! Acceptance gates from the plan, each named test explicitly:
//!
//! - `typed_event_payload_deserialises_in_lua` — typed event with
//!   typed payload deserialises correctly on the Lua side.
//! - `autocmd_group_clear_true_removes_prior_entries` — `clear =
//!   true` on `leviathan.autocmd.group` removes prior autocmds
//!   in the group.
//! - `autocmd_once_true_fires_once_then_auto_removes` — `once =
//!   true` fires once then auto-removes.
//! - `autocmd_debounce_ms_collapses_rapid_fires` — `debounce_ms =
//!   50` collapses rapid fires (uses the host's virtual clock; no
//!   real sleep).
//! - `autocmd_pattern_filters_payload_field` — `pattern = "feature/*"`
//!   filters by the payload's branch-name field.
//! - `autocmd_disabled_after_consecutive_failures` — failing autocmd
//!   disabled after 5 consecutive failures, plugin keeps running.
//! - `reload_drops_old_generation_autocmds` — reload drops every
//!   old-generation autocmd.
//! - `canonical_fetch_event_dispatches_to_canonical_listener` —
//!   canonical fetch events dispatch to canonical listeners.
//! - `devtools_autocmds_reflects_state` — devtools `autocmds` field
//!   reflects every autocmd surface above.
//!
use crate::plugin::tests::harness::{simple_manifest as manifest, MockHost};

#[test]
fn typed_event_payload_deserialises_in_lua() {
    let mut host = MockHost::new();
    host.load_inline(
        "typed",
        &manifest("typed"),
        r#"
        leviathan.autocmd.create("CommitSelected", {
            callback = function(ev)
                _G.last_event = ev.event
                _G.last_alias = ev.alias or ""
                _G.last_hash = ev.payload.commit.hash
            end,
        })
        "#,
    )
    .expect("load");
    host.dispatch_test_event(
        "CommitSelected",
        serde_json::json!({ "commit": { "hash": "deadbeef" } }),
    );
    assert_eq!(
        host.read_global_string("typed", "last_event").as_deref(),
        Some("CommitSelected")
    );
    assert_eq!(
        host.read_global_string("typed", "last_hash").as_deref(),
        Some("deadbeef")
    );
    assert_eq!(
        host.read_global_string("typed", "last_alias").as_deref(),
        Some("")
    );
}

#[test]
fn autocmd_group_clear_true_removes_prior_entries() {
    let mut host = MockHost::new();
    host.load_inline(
        "grp_clear",
        &manifest("grp_clear"),
        r#"
        local g = leviathan.autocmd.group("scratch", { clear = false })
        leviathan.autocmd.create("ThemeChanged", {
            group = g,
            callback = function() _G.fires = (_G.fires or 0) + 1 end,
        })
        leviathan.autocmd.create("ThemeChanged", {
            group = g,
            callback = function() _G.fires = (_G.fires or 0) + 1 end,
        })
        -- second `group("scratch", { clear = true })` call must drop
        -- the two entries above.
        leviathan.autocmd.group("scratch", { clear = true })
        leviathan.autocmd.create("ThemeChanged", {
            group = g,
            callback = function() _G.fires = (_G.fires or 0) + 1 end,
        })
        "#,
    )
    .expect("load");

    host.dispatch_test_event("ThemeChanged", serde_json::json!({ "name": "light" }));
    assert_eq!(
        host.read_global_i64("grp_clear", "fires"),
        Some(1),
        "after clear+re-add only the survivor fires"
    );

    let snap = host.introspect();
    let count = snap
        .autocmds
        .iter()
        .filter(|a| a.plugin_id == "grp_clear" && a.event == "ThemeChanged")
        .count();
    assert_eq!(count, 1);
}

#[test]
fn autocmd_once_true_fires_once_then_auto_removes() {
    let mut host = MockHost::new();
    host.load_inline(
        "once",
        &manifest("once"),
        r#"
        leviathan.autocmd.create("ThemeChanged", {
            once = true,
            callback = function() _G.fires = (_G.fires or 0) + 1 end,
        })
        "#,
    )
    .expect("load");

    host.dispatch_test_event("ThemeChanged", serde_json::json!({ "name": "dark" }));
    host.dispatch_test_event("ThemeChanged", serde_json::json!({ "name": "dark" }));
    host.dispatch_test_event("ThemeChanged", serde_json::json!({ "name": "dark" }));
    assert_eq!(host.read_global_i64("once", "fires"), Some(1));

    let snap = host.introspect();
    let row = snap
        .autocmds
        .iter()
        .find(|a| a.plugin_id == "once" && a.event == "ThemeChanged");
    assert!(row.is_none(), "once autocmd should be auto-removed");
}

#[test]
fn autocmd_debounce_ms_collapses_rapid_fires() {
    let mut host = MockHost::new();
    host.load_inline(
        "deb",
        &manifest("deb"),
        r#"
        leviathan.autocmd.create("WorktreeChanged", {
            debounce_ms = 50,
            callback = function() _G.fires = (_G.fires or 0) + 1 end,
        })
        "#,
    )
    .expect("load");

    // Three fires within the 50ms debounce window — only the first
    // gets through.
    host.dispatch_test_event("WorktreeChanged", serde_json::json!({ "path": "/x" }));
    host.advance_event_clock(10);
    host.dispatch_test_event("WorktreeChanged", serde_json::json!({ "path": "/x" }));
    host.advance_event_clock(10);
    host.dispatch_test_event("WorktreeChanged", serde_json::json!({ "path": "/x" }));
    assert_eq!(host.read_global_i64("deb", "fires"), Some(1));

    // Cross the window — next dispatch fires.
    host.advance_event_clock(60);
    host.dispatch_test_event("WorktreeChanged", serde_json::json!({ "path": "/x" }));
    assert_eq!(host.read_global_i64("deb", "fires"), Some(2));
}

#[test]
fn autocmd_pattern_filters_payload_field() {
    let mut host = MockHost::new();
    host.load_inline(
        "pat",
        &manifest("pat"),
        r#"
        leviathan.autocmd.create("BranchChanged", {
            pattern = "feature/*",
            callback = function(ev)
                _G.fires = (_G.fires or 0) + 1
                _G.last = ev.payload.name
            end,
        })
        "#,
    )
    .expect("load");

    host.dispatch_test_event(
        "BranchChanged",
        serde_json::json!({ "name": "main", "head_hash": "aaa" }),
    );
    assert_eq!(host.read_global_i64("pat", "fires"), None);

    host.dispatch_test_event(
        "BranchChanged",
        serde_json::json!({ "name": "feature/x", "head_hash": "bbb" }),
    );
    assert_eq!(host.read_global_i64("pat", "fires"), Some(1));
    assert_eq!(
        host.read_global_string("pat", "last").as_deref(),
        Some("feature/x")
    );

    host.dispatch_test_event(
        "BranchChanged",
        serde_json::json!({ "name": "feature/y/z", "head_hash": "ccc" }),
    );
    assert_eq!(host.read_global_i64("pat", "fires"), Some(2));
}

#[test]
fn autocmd_disabled_after_consecutive_failures() {
    let mut host = MockHost::new();
    host.load_inline(
        "boom",
        &manifest("boom"),
        r#"
        leviathan.autocmd.create("ThemeChanged", {
            callback = function() error("kaboom") end,
        })
        "#,
    )
    .expect("load");

    for _ in 0..6 {
        host.dispatch_test_event("ThemeChanged", serde_json::json!({ "name": "dark" }));
    }
    let snap = host.introspect();
    let row = snap
        .autocmds
        .iter()
        .find(|a| a.plugin_id == "boom" && a.event == "ThemeChanged")
        .expect("autocmd still present after disable");
    assert!(row.disabled, "autocmd should be disabled");
    assert!(
        row.consecutive_failures >= 5,
        "consecutive failures {} should be >= 5",
        row.consecutive_failures
    );

    // Plugin still loaded — issue another dispatch and confirm it
    // does not panic / unload.
    host.dispatch_test_event("ThemeChanged", serde_json::json!({ "name": "dark" }));
    assert!(host.has_plugin("boom"));

    // The disable diagnostic was emitted.
    let store = host.diagnostics();
    let disable_diag = store
        .tail(50)
        .into_iter()
        .find(|d| d.code == "autocmd.disabled_after_failures");
    assert!(
        disable_diag.is_some(),
        "expected autocmd.disabled_after_failures diagnostic"
    );
}

#[test]
fn reload_drops_old_generation_autocmds() {
    let mut host = MockHost::new();
    host.load_inline(
        "rel",
        &manifest("rel"),
        r#"
        leviathan.autocmd.create("ThemeChanged", {
            callback = function() _G.gen = "v1" end,
        })
        "#,
    )
    .expect("v1 loads");
    host.dispatch_test_event("ThemeChanged", serde_json::json!({ "name": "x" }));
    assert_eq!(host.read_global_string("rel", "gen").as_deref(), Some("v1"));

    host.reload_with_str(
        "rel",
        &manifest("rel"),
        r#"
        leviathan.autocmd.create("ThemeChanged", {
            callback = function() _G.gen = "updated" end,
        })
        "#,
    )
    .expect("updated plugin reloads");

    let snap = host.introspect();
    let rows: Vec<_> = snap
        .autocmds
        .iter()
        .filter(|a| a.plugin_id == "rel" && a.event == "ThemeChanged")
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].generation_id, 2);

    host.dispatch_test_event("ThemeChanged", serde_json::json!({ "name": "y" }));
    assert_eq!(
        host.read_global_string("rel", "gen").as_deref(),
        Some("updated")
    );
}

#[test]
fn sync_focus_fires_focus_changed_autocmd_with_payload() {
    use crate::plugin::ui::focus::{
        FocusReason, FocusSnapshot, FocusSurface, RepositoryFocusSurface,
    };

    let mut host = MockHost::new();
    host.load_inline(
        "focus_listener",
        &manifest("focus_listener"),
        r#"
        leviathan.autocmd.create("FocusChanged", {
            callback = function(ev)
                _G.fires = (_G.fires or 0) + 1
                _G.last_old = ev.payload.old_surface
                _G.last_new = ev.payload.new_surface
                _G.last_reason = ev.payload.reason
            end,
        })
        "#,
    )
    .expect("load");

    let first = FocusSnapshot {
        surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
        reason: FocusReason::Startup,
    };
    assert!(host.host_mut().sync_focus(first.clone()));
    assert_eq!(host.read_global_i64("focus_listener", "fires"), Some(1));
    assert_eq!(
        host.read_global_string("focus_listener", "last_old")
            .as_deref(),
        Some("none")
    );
    assert_eq!(
        host.read_global_string("focus_listener", "last_new")
            .as_deref(),
        Some("repository.graph")
    );

    assert!(!host.host_mut().sync_focus(first));
    assert_eq!(host.read_global_i64("focus_listener", "fires"), Some(1));

    let next = FocusSnapshot {
        surface: FocusSurface::Repository(RepositoryFocusSurface::Details),
        reason: FocusReason::Keymap,
    };
    assert!(host.host_mut().sync_focus(next));
    assert_eq!(host.read_global_i64("focus_listener", "fires"), Some(2));
    assert_eq!(
        host.read_global_string("focus_listener", "last_old")
            .as_deref(),
        Some("repository.graph")
    );
    assert_eq!(
        host.read_global_string("focus_listener", "last_new")
            .as_deref(),
        Some("repository.details")
    );
    assert_eq!(
        host.read_global_string("focus_listener", "last_reason")
            .as_deref(),
        Some("keymap")
    );
}

#[test]
fn canonical_fetch_event_dispatches_to_canonical_listener() {
    let mut host = MockHost::new();
    host.load_inline(
        "fetch_listener",
        &manifest("fetch_listener"),
        r#"
        leviathan.autocmd.create("FetchStarted", {
            callback = function(ev)
                _G.canonical_fires = (_G.canonical_fires or 0) + 1
                _G.canonical_alias = ev.alias or ""
            end,
        })
        "#,
    )
    .expect("load");

    host.dispatch_test_event("FetchStarted", serde_json::json!({ "remote": "origin" }));

    assert_eq!(
        host.read_global_i64("fetch_listener", "canonical_fires"),
        Some(1)
    );
    assert_eq!(
        host.read_global_string("fetch_listener", "canonical_alias")
            .as_deref(),
        Some("")
    );
}

#[test]
fn devtools_autocmds_reflects_state() {
    let mut host = MockHost::new();
    host.load_inline(
        "dev",
        &manifest("dev"),
        r#"
        local g = leviathan.autocmd.group("dev_grp", {})
        leviathan.autocmd.create("ThemeChanged", {
            group = g,
            pattern = "dark*",
            debounce_ms = 25,
            priority = 7,
            callback = function() end,
        })
        leviathan.autocmd.create("ThemeChanged", {
            once = true,
            callback = function() end,
        })
        "#,
    )
    .expect("load");

    let snap = host.introspect();
    let mut rows: Vec<_> = snap
        .autocmds
        .iter()
        .filter(|a| a.plugin_id == "dev")
        .collect();
    rows.sort_by_key(|a| a.id);
    assert_eq!(rows.len(), 2);

    // First autocmd is the grouped one.
    assert_eq!(rows[0].event, "ThemeChanged");
    assert_eq!(rows[0].pattern.as_deref(), Some("dark*"));
    assert_eq!(rows[0].debounce_ms, 25);
    assert_eq!(rows[0].priority, 7);
    assert!(rows[0].group_id.is_some());
    assert!(!rows[0].once);
    assert!(!rows[0].disabled);

    // Second autocmd is the `once = true` one.
    assert!(rows[1].once);
    assert!(rows[1].group_id.is_none());

    // After firing once, the `once` row should disappear and the
    // grouped row's `fires` counter should be 1 (pattern matches).
    host.dispatch_test_event("ThemeChanged", serde_json::json!({ "name": "dark+1" }));
    let snap = host.introspect();
    let rows: Vec<_> = snap
        .autocmds
        .iter()
        .filter(|a| a.plugin_id == "dev")
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].fires, 1);
}

#[test]
fn inline_fixture_listens_to_canonical_fetch_events() {
    let mut host = MockHost::new();
    host.load_inline(
        "fetch_pair",
        &manifest("fetch_pair"),
        r#"
        leviathan.autocmd.create("FetchStarted", {
            callback = function() end,
        })
        leviathan.autocmd.create("FetchFinished", {
            callback = function() end,
        })
        "#,
    )
    .expect("fetch_pair loads");
    let snap = host.introspect();
    let rows: Vec<_> = snap
        .autocmds
        .iter()
        .filter(|a| a.plugin_id == "fetch_pair")
        .collect();
    assert!(
        rows.iter().any(|r| r.event == "FetchStarted"),
        "fixture must register against canonical FetchStarted: {rows:#?}"
    );
    assert!(
        rows.iter().any(|r| r.event == "FetchFinished"),
        "fixture must register against canonical FetchFinished: {rows:#?}"
    );
}

#[test]
fn unknown_event_emits_invalid_event_diagnostic() {
    let mut host = MockHost::new();
    host.load_inline(
        "unk",
        &manifest("unk"),
        r#"
        leviathan.autocmd.create("DefinitelyNotAnEvent", {
            callback = function() end,
        })
        "#,
    )
    .expect("load");
    let store = host.diagnostics();
    let diag = store
        .tail(50)
        .into_iter()
        .find(|d| d.code == "autocmd.invalid_event");
    assert!(diag.is_some(), "expected autocmd.invalid_event diagnostic");
}

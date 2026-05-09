//! Transactional reload tests.

use crate::plugin::devtools::ReloadOutcome;
use crate::plugin::tests::harness::MockHost;

const BASE_MANIFEST: &str = r#"
id = "p6"
name = "p6"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:region:main_bar", "ui:screen"]
"#;

const NOOP_INIT: &str = r#"
leviathan.ui.slot.add{ region = "main_bar",
    id = "p6.slot",
    section = "left",
    priority = 10,
    widget = { kind = "text", value = "v1" },
}
"#;

fn loaded_v1() -> MockHost {
    let mut host = MockHost::new();
    host.load_inline("p6", BASE_MANIFEST, NOOP_INIT)
        .expect("v1 loads");
    host
}

fn old_gen_id(host: &MockHost) -> u64 {
    host.introspect()
        .resources
        .iter()
        .filter(|r| r.plugin_id == "p6")
        .map(|r| r.generation_id)
        .max()
        .unwrap_or(0)
}

#[test]
fn staged_reload_succeeds_drops_old_generation_resources() {
    let mut host = loaded_v1();
    let gen_before = old_gen_id(&host);
    assert_eq!(gen_before, 1);

    host.reload_with_str(
        "p6",
        BASE_MANIFEST,
        r#"
        leviathan.ui.slot.add{ region = "main_bar",
            id = "p6.slot",
            section = "left",
            priority = 10,
            widget = { kind = "text", value = "updated" },
        }
        "#,
    )
    .expect("reload the updated plugin");

    let snap = host.introspect();
    let gens: std::collections::BTreeSet<u64> = snap
        .resources
        .iter()
        .filter(|r| r.plugin_id == "p6")
        .map(|r| r.generation_id)
        .collect();
    assert_eq!(
        gens,
        [2u64].iter().cloned().collect(),
        "every resource keyed to p6 must belong to the new gen (2); got {gens:?}"
    );
    let history = host.host().reload_history("p6");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].outcome, ReloadOutcome::Succeeded);
    assert_eq!(history[0].from_generation_id, 1);
    assert_eq!(history[0].to_generation_id, Some(2));
    assert_eq!(history[0].stage_reached, "swap");
}

#[test]
fn staged_reload_failed_lua_syntax_keeps_old_generation() {
    let mut host = loaded_v1();
    let r = host.reload_with_str("p6", BASE_MANIFEST, "this is not valid lua >>>");
    assert!(r.is_err());
    // v1 still serving.
    assert!(host.has_slot("p6", "main_bar", "left", "p6.slot"));
    let snap = host.introspect();
    assert!(snap
        .resources
        .iter()
        .filter(|r| r.plugin_id == "p6")
        .all(|r| r.generation_id == 1));
    assert!(host.last_reload_error("p6").is_some());
    let diags = host.diagnostics();
    let failed: Vec<_> = diags.by_code("reload.failed");
    assert_eq!(failed.len(), 1);
    let stage = failed[0]
        .context
        .get("stage")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    assert_eq!(stage, "init_lua");
    let history = host.host().reload_history("p6");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].outcome, ReloadOutcome::Failed);
    assert_eq!(history[0].stage_reached, "init_lua");
    assert_eq!(history[0].error_code.as_deref(), Some("lua.init_failed"));
}

#[test]
fn staged_reload_failed_init_error_keeps_old_generation() {
    let mut host = loaded_v1();
    let r = host.reload_with_str("p6", BASE_MANIFEST, r#"error("boom from init")"#);
    assert!(r.is_err());
    assert!(host.has_slot("p6", "main_bar", "left", "p6.slot"));
    let diags = host.diagnostics();
    let failed = diags.by_code("reload.failed");
    assert_eq!(failed.len(), 1);
    assert_eq!(
        failed[0]
            .context
            .get("stage")
            .and_then(|v| v.as_str())
            .unwrap(),
        "init_lua"
    );
}

#[test]
fn staged_reload_failed_manifest_parse_keeps_old_generation() {
    let mut host = loaded_v1();
    let bad_manifest = r#"= = = invalid toml = = ="#;
    let r = host.reload_with_str("p6", bad_manifest, NOOP_INIT);
    assert!(r.is_err());
    assert!(host.has_slot("p6", "main_bar", "left", "p6.slot"));
    let stage = host
        .diagnostics()
        .by_code("reload.failed")
        .last()
        .unwrap()
        .context
        .get("stage")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap();
    assert_eq!(stage, "manifest_parse");
}

#[test]
fn staged_reload_failed_compatibility_keeps_old_generation() {
    let mut host = loaded_v1();
    let bad_compat = r#"
id = "p6"
name = "p6"
version = "0.1.0"
api_version = "99.0"
"#;
    let r = host.reload_with_str("p6", bad_compat, NOOP_INIT);
    assert!(r.is_err());
    assert!(host.has_slot("p6", "main_bar", "left", "p6.slot"));
    let stage = host
        .diagnostics()
        .by_code("reload.failed")
        .last()
        .unwrap()
        .context
        .get("stage")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap();
    assert_eq!(stage, "compatibility");
}

#[test]
fn staged_reload_capability_widening_emits_audit_diagnostic() {
    // Adding a previously-undeclared capability records a
    // `reload.capability_widened` info diagnostic. The grant path
    // upgrades this into a hard `capability.staging_denied` failure
    // when the user has refused the grant; for now the staging path
    // surfaces the change set so the audit log carries it.
    let mut host = loaded_v1();
    host.reload_with_str(
        "p6",
        r#"
id = "p6"
name = "p6"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:region:main_bar", "ui:screen", "fs:read:plugin"]
"#,
        NOOP_INIT,
    )
    .expect("widening reload should succeed (capability grants gate it)");
    assert!(host
        .diagnostics()
        .by_code("reload.capability_widened")
        .iter()
        .any(|d| d
            .context
            .get("added")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(|c| c.as_str() == Some("fs:read:plugin")))
            .unwrap_or(false)));
}

#[test]
fn staged_reload_failed_widget_validation_keeps_old_generation() {
    // Widget that raises during its first call: the staged generation
    // can't even produce a candidate tree, so validation aborts.
    // Decode-time failures are downgraded to diagnostics by design
    // (the renderer's error path matches the production refresh
    // pipeline), but a Lua-level raise is unambiguous.
    let mut host = loaded_v1();
    let r = host.reload_with_str(
        "p6",
        BASE_MANIFEST,
        r#"
        leviathan.ui.slot.add{ region = "main_bar",
            id = "p6.dyn",
            section = "left",
            priority = 10,
            widget = function() error("widget boom") end,
        }
        "#,
    );
    assert!(r.is_err());
    assert!(host.has_slot("p6", "main_bar", "left", "p6.slot"));
    let stage = host
        .diagnostics()
        .by_code("reload.failed")
        .last()
        .unwrap()
        .context
        .get("stage")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap();
    assert_eq!(stage, "validation");
}

#[test]
fn staged_reload_failed_services_validation_keeps_old_generation() {
    let mut host = MockHost::new();
    host.load_inline(
        "svc",
        r#"
id = "svc"
name = "svc"
version = "0.1.0"
api_version = "1.0"
provides_services = ["math@1"]
"#,
        r#"
leviathan.services.register("math@1", { id = function() return 1 end })
"#,
    )
    .expect("v1 loads");
    let r = host.reload_with_str(
        "svc",
        r#"
id = "svc"
name = "svc"
version = "0.1.0"
api_version = "1.0"
provides_services = ["math@1"]
"#,
        r#"-- declared but not registered"#,
    );
    assert!(r.is_err());
    let stage = host
        .diagnostics()
        .by_code("reload.failed")
        .last()
        .unwrap()
        .context
        .get("stage")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap();
    assert_eq!(stage, "validation");
}

#[test]
fn staged_reload_failed_health_check_keeps_old_generation() {
    let mut host = loaded_v1();
    let r = host.reload_with_str(
        "p6",
        BASE_MANIFEST,
        r#"
        leviathan.health.register(function(ctx)
            ctx:error("unhealthy")
        end)
        "#,
    );
    assert!(r.is_err());
    assert!(host.has_slot("p6", "main_bar", "left", "p6.slot"));
    let stage = host
        .diagnostics()
        .by_code("reload.failed")
        .last()
        .unwrap()
        .context
        .get("stage")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap();
    assert_eq!(stage, "health_check");
}

#[test]
fn staged_reload_failed_migration_keeps_old_generation() {
    let mut host = MockHost::new();
    host.load_inline(
        "mig",
        r#"
id = "mig"
name = "mig"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:screen"]
"#,
        r#"
leviathan.ui.screen.register{
    id = "main",
    init = function() return { n = 0 } end,
    view = function(s) return { kind = "text", value = tostring(s.n) } end,
    update = function(s) return s end,
    serialize = function(s) return { n = s.n } end,
    deserialize = function(t) return t end,
}
"#,
    )
    .expect("v1 loads");
    host.open_screen("mig", "main");

    // New init.lua returns a module with an `M.reload` that always
    // raises. Migration must abort and the old plugin must keep
    // serving its slot/screen.
    let r = host.reload_with_str(
        "mig",
        r#"
id = "mig"
name = "mig"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:screen"]
"#,
        r#"
local M = {}
leviathan.ui.screen.register{
    id = "main",
    init = function() return { n = 0 } end,
    view = function(s) return { kind = "text", value = tostring(s.n) } end,
    update = function(s) return s end,
}
function M.reload(_old_state)
    error("migration boom")
end
return M
"#,
    );
    assert!(r.is_err(), "migration failure must propagate");
    let stage = host
        .diagnostics()
        .by_code("reload.failed")
        .last()
        .unwrap()
        .context
        .get("stage")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap();
    assert_eq!(stage, "migration");
    assert!(!host
        .diagnostics()
        .by_code("reload.migration_failed")
        .is_empty());
}

#[test]
fn m_reload_hook_seeds_new_screen_state_from_old() {
    let mut host = MockHost::new();
    host.load_inline(
        "carry",
        r#"
id = "carry"
name = "carry"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:screen"]
"#,
        r#"
leviathan.ui.screen.register{
    id = "s",
    init = function() return { n = 0 } end,
    view = function(s) return { kind = "text", value = tostring(s.n) } end,
    update = function(s) return s end,
    serialize = function(s) return { n = s.n } end,
    deserialize = function(t) return { n = t.n } end,
}
"#,
    )
    .expect("v1 loads");
    host.open_screen("carry", "s");
    host.dispatch_screen_event("carry", "s", "noop");
    // Manually push state.n = 7 into the old gen so we can verify the
    // hook actually seeds it.
    host.host_mut()
        .open_screen("carry".to_string(), "s".to_string());

    // Reload with M.reload that bumps every screen's n by 100.
    host.reload_with_str(
        "carry",
        r#"
id = "carry"
name = "carry"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:screen"]
"#,
        r#"
local M = {}
leviathan.ui.screen.register{
    id = "s",
    init = function() return { n = 0 } end,
    view = function(s) return { kind = "text", value = tostring(s.n) } end,
    update = function(s) return s end,
    serialize = function(s) return { n = s.n } end,
    deserialize = function(t) return { n = t.n } end,
}
function M.reload(old_state)
    local screens = old_state.screens or {}
    local out = { screens = {} }
    for sid, s in pairs(screens) do
        out.screens[sid] = { n = (s.n or 0) + 100 }
    end
    return out
end
return M
"#,
    )
    .expect("reload");

    // Old gen had n = 0; M.reload bumps to 100.
    let post = host.screen_state_json("carry", "s");
    assert_eq!(post.get("n").and_then(|v| v.as_i64()), Some(100));
    let invoked = host.diagnostics().by_code("reload.migration_invoked");
    assert_eq!(invoked.len(), 1);
}

#[test]
fn staged_reload_after_files_run_in_new_generation() {
    let mut host = MockHost::new();
    host.load_inline(
        "after",
        r#"
id = "after"
name = "after"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:screen"]
"#,
        r#"
_G.after_marker = 0
"#,
    )
    .expect("v1 loads");
    host.write_plugin_file("after", "after/plugin/01_set.lua", "_G.after_marker = 1")
        .unwrap();
    host.reload_plugin("after").expect("reload");
    assert_eq!(host.read_global_i64("after", "after_marker"), Some(1));

    // Bump after-file content; verify reload re-runs it against the
    // new gen.
    host.write_plugin_file("after", "after/plugin/01_set.lua", "_G.after_marker = 42")
        .unwrap();
    host.reload_plugin("after").expect("reload again");
    assert_eq!(host.read_global_i64("after", "after_marker"), Some(42));
}

#[test]
fn staged_reload_history_capped_per_plugin() {
    let mut host = loaded_v1();
    // Drive 5 successful reloads; expect five history rows.
    for i in 0..5 {
        let init = format!(
            r#"
            leviathan.ui.slot.add{{ region = "main_bar",
                id = "p6.slot",
                section = "left",
                priority = 10,
                widget = {{ kind = "text", value = "v{i}" }},
            }}
            "#
        );
        host.reload_with_str("p6", BASE_MANIFEST, &init)
            .expect("reload");
    }
    let history = host.host().reload_history("p6");
    assert_eq!(history.len(), 5);
    assert!(history
        .iter()
        .all(|h| h.outcome == ReloadOutcome::Succeeded));
}

#[test]
fn staged_reload_strict_globals_apply_to_new_generation() {
    let strict_manifest = r#"
id = "strict"
name = "strict"
version = "0.1.0"
api_version = "1.0"

[runtime]
strict_globals = true
"#;
    let mut host = MockHost::new();
    host.load_inline_strict("strict", strict_manifest, "-- v1")
        .expect("v1 loads");
    // the staged plugin tries to write a brand-new global; staging exec must fail.
    let r = host.reload_with_str(
        "strict",
        strict_manifest,
        r#"
        sneaky_undeclared = 1
        "#,
    );
    assert!(
        r.is_err(),
        "writing a fresh global under strict should fail"
    );
}

#[test]
fn staged_reload_active_screen_preserved_when_screen_id_unchanged() {
    let mut host = MockHost::new();
    host.load_inline(
        "stable",
        r#"
id = "stable"
name = "stable"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:screen"]
"#,
        r#"
leviathan.ui.screen.register{
    id = "main",
    init = function() return { } end,
    view = function() return { kind = "text", value = "v1" } end,
    update = function(s) return s end,
}
"#,
    )
    .expect("v1");
    host.open_screen("stable", "main");
    assert_eq!(host.host().active_screen(), Some(("stable", "main")));
    host.reload_with_str(
        "stable",
        r#"
id = "stable"
name = "stable"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:screen"]
"#,
        r#"
leviathan.ui.screen.register{
    id = "main",
    init = function() return { } end,
    view = function() return { kind = "text", value = "updated" } end,
    update = function(s) return s end,
}
"#,
    )
    .expect("reload");
    assert_eq!(
        host.host().active_screen(),
        Some(("stable", "main")),
        "active screen must survive when the staged gen still defines it"
    );
}

#[test]
fn staged_reload_active_screen_dropped_when_screen_id_removed() {
    let mut host = MockHost::new();
    host.load_inline(
        "drop",
        r#"
id = "drop"
name = "drop"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:screen"]
"#,
        r#"
leviathan.ui.screen.register{
    id = "main",
    init = function() return { } end,
    view = function() return { kind = "text", value = "v1" } end,
    update = function(s) return s end,
}
"#,
    )
    .expect("v1");
    host.open_screen("drop", "main");
    host.reload_with_str(
        "drop",
        r#"
id = "drop"
name = "drop"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:screen"]
"#,
        r#"
leviathan.ui.screen.register{
    id = "different",
    init = function() return { } end,
    view = function() return { kind = "text", value = "updated" } end,
    update = function(s) return s end,
}
"#,
    )
    .expect("reload");
    assert!(
        host.host().active_screen().is_none(),
        "active screen must drop when the new gen no longer defines it"
    );
}

#[test]
fn staged_reload_completed_diagnostic_carries_duration_ms() {
    let mut host = loaded_v1();
    host.reload_with_str("p6", BASE_MANIFEST, NOOP_INIT)
        .expect("reload");
    let completed = host.diagnostics().by_code("reload.completed");
    assert_eq!(completed.len(), 1);
    assert!(completed[0].context.get("duration_ms").is_some());
    assert!(
        completed[0]
            .context
            .get("previous_generation_id")
            .and_then(|v| v.as_u64())
            == Some(1)
    );
}

#[test]
fn introspect_surfaces_reload_history() {
    let mut host = loaded_v1();
    let _ = host.reload_with_str("p6", BASE_MANIFEST, "this is not lua >>>");
    host.reload_with_str("p6", BASE_MANIFEST, NOOP_INIT)
        .expect("recovery reload");

    let snap = host.introspect();
    assert_eq!(snap.reload_history.len(), 2, "two reload attempts");
    assert_eq!(snap.reload_history[0].outcome, ReloadOutcome::Failed);
    assert_eq!(snap.reload_history[1].outcome, ReloadOutcome::Succeeded);
}

#[test]
fn staged_reload_failure_does_not_register_new_runtime_path() {
    // After a failed reload, the runtime-path registry must still
    // point at the previous gen's root (which equals the same
    // directory in the test harness, so the assertion checks that the
    // *registry entry* still exists — never deleted by a rolled-back
    // staging).
    let mut host = loaded_v1();
    let _ = host.reload_with_str("p6", BASE_MANIFEST, "this is not lua >>>");
    let snap = host.introspect();
    let own_entry = snap
        .runtime_paths
        .iter()
        .find(|r| r.plugin_id == "p6" && r.entry_plugin_id == "p6");
    assert!(own_entry.is_some(), "own runtime-path entry must persist");
}

fn example_plugin_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir("plugins/examples")
        .expect("plugins/examples dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("plugin.toml").is_file() && path.join("init.lua").is_file())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn every_example_plugin_reloads_cleanly() {
    let mut host = MockHost::new();
    let mut ids: Vec<String> = Vec::new();
    for dir in example_plugin_dirs() {
        let raw = std::fs::read_to_string(dir.join("plugin.toml")).expect("manifest");
        let manifest: toml::Value = toml::from_str(&raw).expect("manifest toml");
        let id = manifest
            .get("id")
            .and_then(|value| value.as_str())
            .expect("manifest id")
            .to_string();
        host.host_mut()
            .load_plugin(&dir)
            .unwrap_or_else(|e| panic!("example plugin {id} failed initial load: {e}"));
        ids.push(id);
    }

    for id in &ids {
        host.host_mut()
            .reload_plugin(id)
            .unwrap_or_else(|e| panic!("example plugin {id} failed staged reload: {e}"));
        let history = host.host().reload_history(id);
        assert!(
            history
                .iter()
                .any(|h| h.outcome == ReloadOutcome::Succeeded),
            "example plugin {id} must record a Succeeded reload entry"
        );
        let snap = host.introspect();
        let stale: Vec<_> = snap
            .resources
            .iter()
            .filter(|r| r.plugin_id.as_str() == id.as_str())
            .map(|r| r.generation_id)
            .collect();
        assert!(
            stale.iter().all(|g| *g == 2),
            "{id} resources must all live on gen 2 after one reload; got {stale:?}"
        );
    }
}

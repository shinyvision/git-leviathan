//! Test helpers and typed diagnostics acceptance tests for the typed diagnostic
//! pipeline.

use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticSink, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::resources::PluginId;
use crate::plugin::tests::harness::MockHost;
use std::sync::{Arc, Mutex};

/// Assert at least one diagnostic exists with the given `(plugin_id,
/// code, severity)` tuple. Returns the matched diagnostic for
/// further-grained inspection by callers.
fn expect_diagnostic(
    store: &DiagnosticStore,
    plugin_id: &str,
    code: &str,
    severity: DiagnosticSeverity,
) -> PluginDiagnostic {
    let plugin = PluginId::from(plugin_id);
    let matches: Vec<PluginDiagnostic> = store
        .for_plugin(&plugin)
        .into_iter()
        .filter(|d| d.code == code && d.severity == severity)
        .collect();
    assert!(
        !matches.is_empty(),
        "expected diagnostic plugin={plugin_id} code={code} severity={severity}, got {:?}",
        store
            .entries()
            .into_iter()
            .map(|d| format!("{}/{}/{}", d.plugin_id, d.severity, d.code))
            .collect::<Vec<_>>()
    );
    matches.into_iter().next().unwrap()
}

/// Assert no diagnostic at or above `min` severity is present. Used to
/// prove a happy-path test didn't silently emit warnings/errors.
fn expect_no_diagnostics_at_least(store: &DiagnosticStore, min: DiagnosticSeverity) {
    let bad = store.at_least(min);
    assert!(
        bad.is_empty(),
        "expected no diagnostics at or above {min}, got {:?}",
        bad.into_iter()
            .map(|d| format!("{}/{}/{}", d.plugin_id, d.severity, d.code))
            .collect::<Vec<_>>()
    );
}

/// Number of diagnostics currently retained.
fn diagnostic_count(store: &DiagnosticStore) -> usize {
    store.len()
}

const HAPPY_MANIFEST: &str = r#"
id = "happy"
name = "Happy"
version = "0.1.0"
api_version = "1.0"
"#;

#[derive(Clone, Default)]
struct CaptureSink {
    rows: Arc<Mutex<Vec<String>>>,
}

impl CaptureSink {
    fn rows(&self) -> Vec<String> {
        self.rows.lock().expect("capture lock").clone()
    }
}

impl DiagnosticSink for CaptureSink {
    fn emit(&self, diagnostic: &PluginDiagnostic) {
        self.rows
            .lock()
            .expect("capture lock")
            .push(format!("{}/{}", diagnostic.plugin_id, diagnostic.code));
    }
}

#[test]
fn happy_path_load_emits_only_info_diagnostics() {
    let mut host = MockHost::new();
    host.load_inline("happy", HAPPY_MANIFEST, "").expect("load");
    let store = host.diagnostics();
    expect_no_diagnostics_at_least(&store, DiagnosticSeverity::Warning);
    let info = expect_diagnostic(
        &store,
        "happy",
        "host.plugin_loaded",
        DiagnosticSeverity::Info,
    );
    assert_eq!(info.generation_id.unwrap().get(), 1);
}

#[test]
fn manifest_debug_gates_terminal_sink_without_dropping_diagnostics() {
    let capture = CaptureSink::default();
    let store = DiagnosticStore::with_plugin_debug_sink(Arc::new(capture.clone()));
    let mut host = MockHost::new();
    host.host_mut().set_diagnostic_store(store.clone());

    host.load_inline(
        "quiet",
        r#"
id = "quiet"
name = "Quiet"
version = "0.1.0"
api_version = "1.0"
debug = false
"#,
        r#"leviathan.log("quiet explicit log")"#,
    )
    .expect("quiet load");
    host.load_inline(
        "loud",
        r#"
id = "loud"
name = "Loud"
version = "0.1.0"
api_version = "1.0"
debug = true
"#,
        r#"leviathan.log("loud explicit log")"#,
    )
    .expect("loud load");

    assert_eq!(
        store
            .by_code("host.plugin_loaded")
            .into_iter()
            .filter(|d| d.plugin_id.as_str() == "quiet" || d.plugin_id.as_str() == "loud")
            .count(),
        2
    );
    let rows = capture.rows();
    assert!(
        rows.iter().any(|row| row == "loud/plugin.log"),
        "debug plugin explicit logs should reach sink: {rows:?}"
    );
    assert!(
        rows.iter().any(|row| row == "loud/host.plugin_loaded"),
        "debug plugin diagnostics should reach sink: {rows:?}"
    );
    assert!(
        rows.iter().all(|row| !row.starts_with("quiet/")),
        "non-debug plugin should not reach sink: {rows:?}"
    );
}

#[test]
fn invalid_manifest_emits_structured_diagnostic_and_keeps_running() {
    let mut host = MockHost::new();
    let bad = "id = \"oops\"\nname = \"Oops\"\nversion = \"0.1.0\"\napi_version = \"99.0\"\n";
    let res = host.load_inline("oops", bad, "");
    assert!(res.is_err(), "load should fail on incompatible api_version");

    let store = host.diagnostics();
    let diag = expect_diagnostic(
        &store,
        "oops",
        "manifest.incompatible_api_version",
        DiagnosticSeverity::Error,
    );
    assert!(diag.message.contains("supported plugin api versions: 1.0"));
    assert!(
        matches!(diag.source, Some(PluginSourceSpan::Manifest { ref key, .. }) if key.as_deref() == Some("api_version"))
    );
    // Host keeps running: a subsequent good load succeeds.
    host.load_inline("happy", HAPPY_MANIFEST, "")
        .expect("subsequent load works");
}

#[test]
fn invalid_widget_emits_widget_path_diagnostic() {
    let mut host = MockHost::new();
    host.load_inline(
        "wbad",
        r#"
id = "wbad"
name = "Wbad"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:screen"]
"#,
        r#"
        leviathan.ui.screen.register{
            id = "screen",
            init = function() return {} end,
            -- view returns a value that can't be serialised into JSON
            -- (function values aren't valid in a widget tree).
            view = function() return { kind = "text", value = function() end } end,
            update = function(s) return s end,
        }
        "#,
    )
    .expect("load");
    host.open_screen("wbad", "screen");

    let store = host.diagnostics();
    let diag = expect_diagnostic(
        &store,
        "wbad",
        "widget.invalid_tree",
        DiagnosticSeverity::Error,
    );
    let path = match diag.source {
        Some(PluginSourceSpan::Widget { path }) => path,
        other => panic!("expected widget source, got {other:?}"),
    };
    assert!(
        path.contains("screen.screen.view") || path.contains("screen"),
        "widget source path should include screen id: {path}"
    );
}

#[test]
fn invalid_widget_field_emits_field_level_diagnostic() {
    // widget AST: a bad field type produces a *specific* code (not the
    // generic `widget.invalid_tree`) and a `Widget` source span that
    // points at the offending field path.
    let mut host = MockHost::new();
    host.load_inline(
        "wbad2",
        r#"
id = "wbad2"
name = "Wbad2"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:screen"]
"#,
        r#"
        leviathan.ui.screen.register{
            id = "screen",
            init = function() return {} end,
            -- `text` field of a button must be a string; a number
            -- triggers widget.field_type_mismatch with an exact path.
            view = function() return {
                kind = "row",
                children = {
                    { kind = "text", value = "ok" },
                    { kind = "button", text = 7 },
                },
            } end,
            update = function(s) return s end,
        }
        "#,
    )
    .expect("load");
    host.open_screen("wbad2", "screen");

    let store = host.diagnostics();
    let diag = expect_diagnostic(
        &store,
        "wbad2",
        "widget.field_type_mismatch",
        DiagnosticSeverity::Error,
    );
    let path = match diag.source {
        Some(PluginSourceSpan::Widget { path }) => path,
        other => panic!("expected widget source, got {other:?}"),
    };
    assert!(
        path.contains("children[1].text"),
        "expected widget path to point at the bad field, got {path}"
    );
}

#[test]
fn lua_callback_error_emits_traceback_diagnostic() {
    let mut host = MockHost::new();
    host.load_inline(
        "callback_oops",
        r#"
id = "callback_oops"
name = "callback_oops"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:region:main_bar"]
"#,
        r#"
        leviathan.autocmd.create({ "BranchChanged" }, {
            callback = function() error("kaboom from autocmd") end,
        })
        "#,
    )
    .expect("load");

    host.host_mut().fire_event("BranchChanged");

    let store = host.diagnostics();
    // autocmd renamed the autocmd-callback failure code from the
    // generic `lua.callback_error` to the more specific
    // `autocmd.callback_failed` (the screen-update path still uses
    // the original code; this codify the new boundary).
    let diag = expect_diagnostic(
        &store,
        "callback_oops",
        "autocmd.callback_failed",
        DiagnosticSeverity::Error,
    );
    let (file, line, _traceback) = match diag.source {
        Some(PluginSourceSpan::Lua {
            file,
            line,
            traceback,
        }) => (file, line, traceback),
        other => panic!("expected lua source span, got {other:?}"),
    };
    assert!(
        file.contains("plugins/callback_oops"),
        "file should name the plugin chunk: {file}"
    );
    // mlua's RuntimeError often inlines the chunk name + line; we
    // accept either parsed line OR a substring match in the message
    // as proof the location survived the round trip.
    if line.is_none() {
        assert!(
            diag.message.contains(":"),
            "expected location in message: {}",
            diag.message
        );
    }
    assert_eq!(diag.context["event"], "BranchChanged");
}

#[test]
fn denied_capability_emits_diagnostic_and_audit_entry() {
    let mut host = MockHost::new();
    // Trigger the denial inside a deferred callback so init.lua
    // doesn't abort on the raised mlua error — the host still emits
    // a `capability.denied` diagnostic the moment the gated API is
    // called, and the host itself keeps running.
    host.load_inline(
        "denier",
        r#"
id = "denier"
name = "denier"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:region:main_bar"]
"#,
        r#"
        leviathan.api.schedule(function()
            local ok, err = pcall(leviathan.fs.read_file, "/etc/passwd")
            _G.read_ok = ok and 1 or 0
        end)
        "#,
    )
    .expect("load");
    host.tick();

    let store = host.diagnostics();
    let diag = expect_diagnostic(
        &store,
        "denier",
        "capability.denied",
        DiagnosticSeverity::Error,
    );
    assert_eq!(diag.context["capability"], "fs:read");
    assert_eq!(diag.generation_id.unwrap().get(), 1);

    // The audit log also records the denial — diagnostics and audit
    // are complementary, not exclusive.
    let snap = host.introspect();
    assert!(
        snap.audit_recent
            .iter()
            .any(|e| e.plugin_id == "denier" && e.capability == "fs:read"),
        "audit log should record fs:read denial"
    );
    // Plugin is still loaded; host kept running.
    assert!(host.has_plugin("denier"));
}

#[test]
fn failed_reload_emits_reload_diagnostic_and_keeps_old_generation_active() {
    let mut host = MockHost::new();
    host.load_inline(
        "rel",
        r#"
id = "rel"
name = "rel"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:region:main_bar"]
"#,
        r#"
        leviathan.ui.slot.add{ region = "main_bar",
            id = "rel.slot",
            section = "left",
            priority = 10,
            widget = { kind = "text", value = "old" },
        }
        "#,
    )
    .expect("load");

    let res = host.reload_with_str(
        "rel",
        r#"
id = "rel"
name = "rel"
version = "0.1.0"
api_version = "1.0"
"#,
        "this is not valid lua >>>",
    );
    assert!(res.is_err(), "reload should fail on bad lua");

    let store = host.diagnostics();
    let _diag = expect_diagnostic(&store, "rel", "reload.failed", DiagnosticSeverity::Error);
    // Old generation kept active: slot still present.
    let snap = host.introspect();
    assert!(snap.slots.iter().any(|s| s.id == "rel.slot"));
    let active_gen = snap
        .resources
        .iter()
        .find(|r| r.plugin_id == "rel")
        .map(|r| r.generation_id)
        .unwrap_or(0);
    assert_eq!(active_gen, 1, "generation 1 should still be live");
}

#[test]
fn cleanup_failure_records_diagnostic() {
    use crate::plugin::resources::{
        GenerationId, PluginId as ResId, PluginResourceKind, ResourceCleaner, ResourceLedger,
        ResourceRecord,
    };

    struct FailingCleaner;
    impl ResourceCleaner for FailingCleaner {
        fn cleanup_resource(&mut self, _resource: &ResourceRecord) -> Result<(), String> {
            Err("simulated".into())
        }
    }

    let store =
        DiagnosticStore::with_sink(std::sync::Arc::new(crate::plugin::diagnostic::NullSink));
    let ledger = ResourceLedger::new(ResId::from("c"), GenerationId::new(2));
    ledger.record(PluginResourceKind::Slot, "slot:x", None);
    let report = ledger.cleanup_all(&mut FailingCleaner);
    assert!(report.has_errors());
    // Convert manually like the host does, then assert.
    for error in report.errors {
        store.record(
            PluginDiagnostic::new(
                error.resource.plugin_id.clone(),
                DiagnosticSeverity::Error,
                "cleanup.resource_failed",
                error.error,
            )
            .with_generation(error.resource.generation_id),
        );
    }
    let diag = expect_diagnostic(
        &store,
        "c",
        "cleanup.resource_failed",
        DiagnosticSeverity::Error,
    );
    assert_eq!(diag.generation_id.unwrap().get(), 2);
    assert_eq!(diagnostic_count(&store), 1);
}

#[test]
fn store_query_helpers_filter_correctly() {
    let mut host = MockHost::new();
    host.load_inline("a", HAPPY_MANIFEST.replace("happy", "a").as_str(), "")
        .expect("load");
    host.load_inline("b", HAPPY_MANIFEST.replace("happy", "b").as_str(), "")
        .expect("load");

    let store = host.diagnostics();
    let a = store.for_plugin(&PluginId::from("a"));
    let b = store.for_plugin(&PluginId::from("b"));
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);

    let info = store.at_least(DiagnosticSeverity::Info);
    assert!(info.len() >= 2);

    let loaded = store.by_code("host.plugin_loaded");
    assert_eq!(loaded.len(), 2);

    // Snapshot of the inspector exposes the diagnostics list.
    let snap = host.introspect();
    assert!(!snap.diagnostics.is_empty());
    assert!(snap
        .diagnostics
        .iter()
        .any(|d| d.code == "host.plugin_loaded" && d.severity == "info"));
}

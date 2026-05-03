//! Phase 19 tests — devtools completion. Verifies the ten host-bundled
//! devtools commands are reachable through the unified dispatcher,
//! the diagnostic-bundle exporter excludes secret values, and the
//! introspector snapshot exposes every plugin-owned resource kind.

use serde_json::json;

use crate::plugin::commands::InvokeOutcome;
use crate::plugin::devtools_commands::devtools_command_names;
use crate::plugin::tests::harness::MockHost;

const SMOKE_MANIFEST: &str = r#"
id = "p"
name = "p"
version = "0.1.0"
api_version = "1.0"
"#;

fn manifest_with_id(id: &str) -> String {
    format!(
        r#"
id = "{id}"
name = "{id}"
version = "0.1.0"
api_version = "1.0"
"#
    )
}

#[test]
fn each_plugin_command_dispatches_through_palette() {
    let mut host = MockHost::new();
    host.load_inline("p", SMOKE_MANIFEST, "").expect("load");
    let registry = host.command_registry();
    let names: Vec<String> = registry
        .borrow()
        .summaries()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    for cmd in devtools_command_names() {
        assert!(
            names.contains(&cmd.to_string()),
            "devtools command `{cmd}` missing from registry"
        );
    }
    // Drive each through `invoke_command`, asserting we reach the body
    // (i.e. the outcome is `Ok`, the registry surfaced the command,
    // and validate_args was happy with the supplied args).
    let cases: &[(&str, serde_json::Value)] = &[
        ("plugin.reload", json!({ "plugin_id": "p" })),
        ("plugin.disable", json!({ "plugin_id": "p" })),
        ("plugin.enable", json!({ "plugin_id": "p" })),
        ("plugin.open_log", json!({ "plugin_id": "p" })),
        ("plugin.inspect_ui_tree", json!({ "plugin_id": "p" })),
        ("plugin.run_health_check", json!({ "plugin_id": "p" })),
        ("plugin.clear_state", json!({ "plugin_id": "p" })),
        (
            "plugin.export_diagnostic_bundle",
            json!({ "plugin_id": "p" }),
        ),
        ("plugin.show_capability_audit", json!({})),
        ("plugin.show_runtime_path", json!({ "plugin_id": "p" })),
    ];
    for (name, args) in cases {
        let (outcome, _value) = host.invoke_devtools_command(name, args.clone());
        assert!(
            matches!(outcome, InvokeOutcome::Ok),
            "command `{name}` did not dispatch ok: {outcome:?}"
        );
    }
}

#[test]
fn disable_skips_plugin_on_subsequent_load() {
    let mut host = MockHost::new();
    host.load_inline("p", SMOKE_MANIFEST, "").expect("load");
    let (outcome, value) =
        host.invoke_devtools_command("plugin.disable", json!({ "plugin_id": "p" }));
    assert!(matches!(outcome, InvokeOutcome::Ok));
    assert!(host.host().is_plugin_disabled("p"));
    assert_eq!(value.get("unloaded").and_then(|v| v.as_bool()), Some(true));
    assert!(!host.host().has_loaded_plugins());
}

#[test]
fn enable_undoes_disable() {
    let mut host = MockHost::new();
    host.load_inline("p", SMOKE_MANIFEST, "").expect("load");
    host.invoke_devtools_command("plugin.disable", json!({ "plugin_id": "p" }));
    assert!(host.host().is_plugin_disabled("p"));
    let (outcome, value) =
        host.invoke_devtools_command("plugin.enable", json!({ "plugin_id": "p" }));
    assert!(matches!(outcome, InvokeOutcome::Ok));
    assert!(!host.host().is_plugin_disabled("p"));
    // Plugin should reload from its known root.
    assert_eq!(value.get("reloaded").and_then(|v| v.as_bool()), Some(true));
    assert!(host.host().has_loaded_plugins());
}

#[test]
fn clear_state_wipes_state_and_cache_but_not_secrets_by_default() {
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        SMOKE_MANIFEST,
        r#"
            local store = leviathan.persist.open("data", { version = 1 })
            store:set("foo", "bar")
            "#,
    )
    .expect("load");
    // Seed a secret directly so we can prove default surfaces don't
    // wipe it.
    let plugin_id = "p";
    let plugin_root = host.plugin_dir(plugin_id).unwrap().to_path_buf();
    let storage = host.host().storage_paths_for_test(plugin_id, &plugin_root);
    std::fs::create_dir_all(&storage.secrets_dir).unwrap();
    let secret_path = storage.secrets_dir.join("secrets.json");
    std::fs::write(
        &secret_path,
        r#"{"values":{"my_token":"REDACTED-CANARY-VALUE"}}"#,
    )
    .unwrap();
    let pre_state_files = storage.state_dir.join("data.json");
    assert!(
        pre_state_files.exists(),
        "data.json should exist after persist write"
    );

    let (outcome, _value) =
        host.invoke_devtools_command("plugin.clear_state", json!({ "plugin_id": "p" }));
    assert!(matches!(outcome, InvokeOutcome::Ok));
    // Default surfaces are state + cache. State dir should now be
    // gone; secrets dir untouched.
    assert!(
        !pre_state_files.exists(),
        "default clear_state must wipe state dir"
    );
    assert!(
        secret_path.exists(),
        "secrets dir must NOT be wiped by default"
    );
    let secret_raw = std::fs::read_to_string(&secret_path).unwrap();
    assert!(secret_raw.contains("REDACTED-CANARY-VALUE"));
}

#[test]
fn clear_state_with_secrets_wipes_secrets_only_when_requested() {
    let mut host = MockHost::new();
    host.load_inline("p", SMOKE_MANIFEST, "").expect("load");
    let plugin_id = "p";
    let plugin_root = host.plugin_dir(plugin_id).unwrap().to_path_buf();
    let storage = host.host().storage_paths_for_test(plugin_id, &plugin_root);
    std::fs::create_dir_all(&storage.secrets_dir).unwrap();
    let secret_path = storage.secrets_dir.join("secrets.json");
    std::fs::write(&secret_path, r#"{"values":{"k":"v"}}"#).unwrap();

    let (outcome, _value) = host.invoke_devtools_command(
        "plugin.clear_state",
        json!({ "plugin_id": "p", "surfaces": "secrets" }),
    );
    assert!(matches!(outcome, InvokeOutcome::Ok));
    assert!(
        !secret_path.exists(),
        "explicit secrets surface must wipe secret store"
    );
}

#[test]
fn inspect_ui_tree_returns_widget_ast_inventory() {
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        SMOKE_MANIFEST,
        r#"
            leviathan.ui.main_bar.add{
                id = "p.slot",
                section = "left",
                priority = 5,
                widget = { kind = "text", value = "hi" },
            }
            "#,
    )
    .expect("load");
    let (outcome, value) =
        host.invoke_devtools_command("plugin.inspect_ui_tree", json!({ "plugin_id": "p" }));
    assert!(matches!(outcome, InvokeOutcome::Ok));
    assert_eq!(value.get("ok").and_then(|v| v.as_bool()), Some(true));
    let inv = value.get("inventory").expect("inventory");
    let slots = inv.get("slots").and_then(|v| v.as_array()).expect("slots");
    assert!(
        slots
            .iter()
            .any(|s| s.get("id").and_then(|v| v.as_str()) == Some("p.slot")),
        "expected p.slot in inventory: {inv:?}"
    );
}

#[test]
fn run_health_check_returns_structured_result() {
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        SMOKE_MANIFEST,
        r#"
            leviathan.health.register(function(ctx)
                ctx:ok("alive")
                ctx:warn("slow")
            end)
            "#,
    )
    .expect("load");
    let (outcome, value) =
        host.invoke_devtools_command("plugin.run_health_check", json!({ "plugin_id": "p" }));
    assert!(matches!(outcome, InvokeOutcome::Ok));
    let items = value
        .get("items")
        .and_then(|v| v.as_array())
        .expect("items");
    assert_eq!(items.len(), 2);
    let messages: Vec<&str> = items
        .iter()
        .filter_map(|i| i.get("message").and_then(|v| v.as_str()))
        .collect();
    assert!(messages.contains(&"alive"));
    assert!(messages.contains(&"slow"));
}

#[test]
fn export_diagnostic_bundle_excludes_secret_values() {
    let mut host = MockHost::new();
    host.load_inline("p", SMOKE_MANIFEST, "").expect("load");
    let plugin_id = "p";
    let plugin_root = host.plugin_dir(plugin_id).unwrap().to_path_buf();
    let storage = host.host().storage_paths_for_test(plugin_id, &plugin_root);
    std::fs::create_dir_all(&storage.secrets_dir).unwrap();
    let secret_path = storage.secrets_dir.join("secrets.json");
    std::fs::write(
        &secret_path,
        r#"{"values":{"my_token":"SUPER-SECRET-VALUE-12345"}}"#,
    )
    .unwrap();

    // First export: no flags. Must not contain the secret value.
    let (outcome, value) = host.invoke_devtools_command(
        "plugin.export_diagnostic_bundle",
        json!({ "plugin_id": "p" }),
    );
    assert!(matches!(outcome, InvokeOutcome::Ok));
    let serialized = value.to_string();
    assert!(
        !serialized.contains("SUPER-SECRET-VALUE-12345"),
        "default bundle leaked secret: {serialized}"
    );

    // Second export: include_secrets = true and include_state = true.
    // Even when secret metadata is requested, the bundle must NOT
    // contain raw secret values; only the key list.
    let (outcome, value) = host.invoke_devtools_command(
        "plugin.export_diagnostic_bundle",
        json!({
            "plugin_id": "p",
            "include_state": true,
            "include_secrets": true,
        }),
    );
    assert!(matches!(outcome, InvokeOutcome::Ok));
    let serialized = value.to_string();
    assert!(
        !serialized.contains("SUPER-SECRET-VALUE-12345"),
        "include_secrets=true bundle leaked secret value: {serialized}"
    );
    // But the secret KEY name must appear in `secret_metadata`.
    let meta = value
        .get("secret_metadata")
        .and_then(|v| v.as_array())
        .expect("secret_metadata array");
    assert!(meta.iter().any(|m| m
        .get("keys")
        .and_then(|k| k.as_array())
        .map(|a| a.iter().any(|s| s.as_str() == Some("my_token")))
        .unwrap_or(false)));
}

#[test]
fn export_diagnostic_bundle_includes_state_when_requested() {
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        SMOKE_MANIFEST,
        r#"
            local s = leviathan.persist.open("data", { version = 1 })
            s:set("hello", "world")
            "#,
    )
    .expect("load");

    let (outcome, value_off) = host.invoke_devtools_command(
        "plugin.export_diagnostic_bundle",
        json!({ "plugin_id": "p" }),
    );
    assert!(matches!(outcome, InvokeOutcome::Ok));
    assert!(
        value_off.get("state").and_then(|v| v.as_object()).is_none()
            || value_off.get("state").map(|v| v.is_null()).unwrap_or(true)
    );

    let (outcome, value_on) = host.invoke_devtools_command(
        "plugin.export_diagnostic_bundle",
        json!({ "plugin_id": "p", "include_state": true }),
    );
    assert!(matches!(outcome, InvokeOutcome::Ok));
    let serialized = value_on.to_string();
    assert!(
        serialized.contains("hello") && serialized.contains("world"),
        "expected state in bundle: {serialized}"
    );
}

#[test]
fn capability_audit_filters_by_plugin_id_and_limit() {
    let mut host = MockHost::new();
    // Loading without `fs:read` capability should record a Denied
    // audit entry when the plugin tries to read.
    let _ = host.load_inline(
        "denied",
        r#"
id = "denied"
name = "denied"
version = "0.1.0"
api_version = "1.0"
"#,
        r#"leviathan.fs.read_file("/etc/passwd")"#,
    );
    // Load a second plugin so we have at least two plugin ids in
    // play.
    host.load_inline("other", &manifest_with_id("other"), "")
        .expect("load other");

    let (outcome, value_filtered) = host.invoke_devtools_command(
        "plugin.show_capability_audit",
        json!({ "plugin_id": "denied", "limit": 50 }),
    );
    assert!(matches!(outcome, InvokeOutcome::Ok));
    let entries = value_filtered
        .get("entries")
        .and_then(|v| v.as_array())
        .expect("entries");
    assert!(!entries.is_empty(), "expected at least one audit entry");
    for entry in entries {
        assert_eq!(
            entry.get("plugin_id").and_then(|v| v.as_str()),
            Some("denied"),
            "filter must exclude other plugins"
        );
    }
    // limit clamps the row count.
    let (_, value_limited) =
        host.invoke_devtools_command("plugin.show_capability_audit", json!({ "limit": 1 }));
    let count = value_limited
        .get("entries")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    assert!(count <= 1, "limit=1 must clamp rows; got {count}");
}

#[test]
fn runtime_path_command_returns_phase5_entries() {
    let mut host = MockHost::new();
    host.load_inline("p", SMOKE_MANIFEST, "").expect("load");
    let (outcome, value) =
        host.invoke_devtools_command("plugin.show_runtime_path", json!({ "plugin_id": "p" }));
    assert!(matches!(outcome, InvokeOutcome::Ok));
    let entries = value
        .get("entries")
        .and_then(|v| v.as_array())
        .expect("entries");
    assert!(
        !entries.is_empty(),
        "runtime path must include the plugin's own root"
    );
    assert!(entries
        .iter()
        .any(|e| e.get("entry_plugin_id").and_then(|v| v.as_str()) == Some("p")));
}

#[test]
fn reload_command_runs_through_staged_reload() {
    let mut host = MockHost::new();
    host.load_inline("p", SMOKE_MANIFEST, "").expect("load");
    let (outcome, value) =
        host.invoke_devtools_command("plugin.reload", json!({ "plugin_id": "p" }));
    assert!(matches!(outcome, InvokeOutcome::Ok));
    assert_eq!(value.get("ok").and_then(|v| v.as_bool()), Some(true));
    // Reload history must record at least one row for the plugin.
    let history = host.host().reload_history("p");
    assert!(
        !history.is_empty(),
        "reload command must produce a reload history row"
    );
}

#[test]
fn every_resource_kind_visible_in_inspector_snapshot() {
    let mut host = MockHost::new();
    let manifest = r#"
id = "kitchen_sink"
name = "kitchen_sink"
version = "0.1.0"
api_version = "1.0"
provides_services = ["thing@1"]
capabilities = ["async:spawn", "timer:create", "fs:watch:plugin", "ui:overlay", "ui:context_menu", "ui:graph_decoration", "ui:diff_decoration"]
"#;
    let init = r#"
        -- Slot
        leviathan.ui.main_bar.add{
            id = "ks.slot",
            section = "left",
            priority = 5,
            widget = { kind = "text", value = "x" },
        }
        -- Command
        leviathan.command.create("ks.cmd", { run = function() end })
        -- Keymap
        leviathan.keymap.set("global", "<leader>k", "ks.cmd")
        -- Autocmd
        leviathan.autocmd.create("FetchStarted", { callback = function() end })
        -- Service
        leviathan.services.register("thing@1", { foo = function() return 1 end })
        -- Overlay
        leviathan.ui.overlay{
            id = "ks.overlay",
            priority = 10,
            widget = { kind = "text", value = "modal" },
        }
        -- Context menu item
        leviathan.ui.context_menu("repository.diff.context_menu", {
            id = "ks.menu",
            label = "do thing",
            command = "ks.cmd",
            priority = 1,
        })
        -- Persist (storage row)
        local s = leviathan.persist.open("data", { version = 1 })
        s:set("k", "v")
        -- Settings (just registers the schema)
        leviathan.settings.define_schema({ foo = { type = "string", default = "x" } })
        -- Health check
        leviathan.health.register(function(ctx) ctx:ok("alive") end)
    "#;
    host.load_inline("kitchen_sink", manifest, init)
        .expect("load");
    let snap = host.introspect();
    let id = "kitchen_sink";
    assert!(snap.plugins.iter().any(|p| p.id == id));
    assert!(
        snap.slots.iter().any(|s| s.owner_plugin_id == id),
        "slot row missing"
    );
    assert!(
        snap.commands.iter().any(|c| c.plugin_id == id),
        "command row missing"
    );
    assert!(
        snap.keymaps.iter().any(|k| k.plugin_id == id),
        "keymap row missing"
    );
    assert!(
        snap.autocmds.iter().any(|a| a.plugin_id == id),
        "autocmd row missing"
    );
    assert!(
        snap.services.iter().any(|s| s.publisher_plugin_id == id),
        "service row missing"
    );
    assert!(
        snap.overlays.iter().any(|o| o.plugin_id == id),
        "overlay row missing"
    );
    assert!(
        snap.context_menu_items.iter().any(|c| c.plugin_id == id),
        "context_menu row missing"
    );
    assert!(
        snap.storage.iter().any(|s| s.plugin_id == id),
        "storage row missing"
    );
    assert!(
        snap.settings.iter().any(|s| s.plugin_id == id),
        "settings row missing"
    );
    assert!(
        snap.secrets.iter().any(|s| s.plugin_id == id),
        "secrets row missing"
    );
    assert!(
        snap.runtime_paths.iter().any(|r| r.plugin_id == id),
        "runtime path row missing"
    );
    assert!(
        snap.resources.iter().any(|r| r.plugin_id == id),
        "resource row missing"
    );
}

/// Devtools snapshot test — exercises that the introspector surfaces
/// stable shapes (sortedness + presence) regardless of registration
/// order. Asserts a normalised projection rather than concrete unix-
/// ms values that change run-to-run.
#[test]
fn devtools_snapshot_shape_is_stable() {
    let mut host = MockHost::new();
    host.load_inline("alpha", &manifest_with_id("alpha"), "")
        .expect("load alpha");
    host.load_inline("beta", &manifest_with_id("beta"), "")
        .expect("load beta");
    let snap = host.introspect();
    let plugin_ids: Vec<&str> = snap.plugins.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        plugin_ids,
        vec!["alpha", "beta"],
        "plugins must be sorted by id"
    );
    // Every devtools command shows up under the host plugin id.
    let host_commands: Vec<&str> = snap
        .commands
        .iter()
        .filter(|c| c.plugin_id == "<host>")
        .map(|c| c.name.as_str())
        .collect();
    for cmd in devtools_command_names() {
        assert!(
            host_commands.contains(&cmd),
            "devtools command `{cmd}` missing from snapshot"
        );
    }
}

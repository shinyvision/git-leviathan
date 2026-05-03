//! Phase 16 acceptance tests: lazy loading and activation.
//!
//! Acceptance gates exercised here:
//! - lazy plugin does not load its Lua state until first trigger;
//! - lazy command invocation activates the plugin and dispatches
//!   the command through the unified registry (single entry path);
//! - lazy keymap invocation activates the plugin and re-dispatches
//!   the chord;
//! - lazy event dispatch activates the plugin and the freshly
//!   installed autocmds observe the same event;
//! - file-presence trigger activates the plugin when the named
//!   relative path appears in the active workdir;
//! - failed activation is contained and visible (diagnostics +
//!   `LazyEntrySummary::last_error`);
//! - repeated failure poisons the plugin so we don't spin up
//!   repeatedly;
//! - eager (no `[activation]`) plugins still load immediately
//!   through `resolve_and_load`;
//! - devtools `lazy_plugins` row reflects status / activations /
//!   triggers.

use crate::plugin::commands::InvokeOutcome;
use crate::plugin::tests::harness::MockHost;

const EAGER_MANIFEST: &str = r#"
id = "eager"
name = "eager"
version = "0.1.0"
api_version = "1.0"
"#;

fn lazy_manifest(id: &str, activation: &str) -> String {
    format!(
        r#"
id = "{id}"
name = "{id}"
version = "0.1.0"
api_version = "1.0"

[activation]
{activation}
"#
    )
}

fn diag_codes(host: &MockHost) -> Vec<String> {
    host.diagnostics()
        .tail(500)
        .into_iter()
        .map(|d| d.code)
        .collect()
}

#[test]
fn lazy_plugin_does_not_load_lua_state_until_trigger() {
    let mut host = MockHost::new();
    host.load_inline_set(&[(
        "lazy",
        &lazy_manifest("lazy", "commands = [\"lazy.go\"]"),
        // init.lua deliberately writes a global the test asserts is
        // *not* set until activation.
        r#"
        _G.activated = "yes"
        leviathan.command.create("lazy.go", {
            title = "Go",
            description = "go.",
            run = function(_) _G.ran = "yes" end,
        })
        "#,
    )])
    .expect("resolve and load");

    // Lua state never built — no value for the global, no plugin in
    // the live `plugins` set, but the lazy registry has it.
    assert!(host.read_global_string("lazy", "activated").is_none());
    let snap = host.introspect();
    assert!(snap.plugins.iter().all(|p| p.id != "lazy"));
    let lazy = snap
        .lazy_plugins
        .iter()
        .find(|l| l.plugin_id == "lazy")
        .expect("lazy summary present");
    assert_eq!(lazy.status, "lazy");
    assert!(lazy.triggers.iter().any(|t| t == "command:lazy.go"));
    assert_eq!(lazy.activations, 0);
}

#[test]
fn lazy_command_invocation_activates_plugin() {
    let mut host = MockHost::new();
    host.load_inline_set(&[(
        "lc",
        &lazy_manifest("lc", "commands = [\"lc.run\"]"),
        r#"
        _G.activated = "yes"
        leviathan.command.create("lc.run", {
            title = "Run",
            description = "run.",
            run = function(_) _G.ran = (_G.ran or 0) + 1 end,
        })
        "#,
    )])
    .expect("resolve and load");

    // No Lua state yet.
    assert!(host.read_global_string("lc", "activated").is_none());

    let out = host.invoke_command("lc.run", serde_json::Value::Null);
    assert!(matches!(out, InvokeOutcome::Ok), "got: {out:?}");
    assert_eq!(
        host.read_global_string("lc", "activated").as_deref(),
        Some("yes")
    );
    assert_eq!(host.read_global_i64("lc", "ran"), Some(1));

    // Snapshot reflects activation.
    let snap = host.introspect();
    let lazy = snap
        .lazy_plugins
        .iter()
        .find(|l| l.plugin_id == "lc")
        .expect("lazy summary still present after activation");
    assert_eq!(lazy.status, "active");
    assert_eq!(lazy.activations, 1);
    assert!(lazy.last_activation_unix_ms.is_some());

    // Repeated invocation does not reload the plugin.
    let _ = host.invoke_command("lc.run", serde_json::Value::Null);
    let snap2 = host.introspect();
    let lazy2 = snap2
        .lazy_plugins
        .iter()
        .find(|l| l.plugin_id == "lc")
        .expect("lazy summary present");
    assert_eq!(
        lazy2.activations, 1,
        "second invocation must not re-activate"
    );
    assert_eq!(host.read_global_i64("lc", "ran"), Some(2));
}

#[test]
fn lazy_keymap_invocation_activates_plugin() {
    use crate::plugin::keymap::parse_key_sequence;

    let mut host = MockHost::new();
    host.load_inline_set(&[(
        "lk",
        &lazy_manifest(
            "lk",
            "[[activation.keymaps]]\ncontext = \"global\"\nkey = \"<leader>l\"",
        ),
        r#"
        _G.activated = "yes"
        leviathan.command.create("lk.run", {
            title = "Run",
            description = "run.",
            run = function(_) _G.ran = (_G.ran or 0) + 1 end,
        })
        leviathan.keymap.set("global", "<leader>l", "lk.run", { description = "run" })
        "#,
    )])
    .expect("resolve and load");
    host.set_keymap_leader(",");

    assert!(host.read_global_string("lk", "activated").is_none());

    let chord = parse_key_sequence("<leader>l", ",").expect("parse");
    let outcome = host.dispatch_key("global", &chord);
    assert!(
        matches!(outcome, crate::plugin::keymap::KeymapDispatchOutcome::Dispatched { .. }),
        "expected Dispatched, got {outcome:?}"
    );
    assert_eq!(host.read_global_i64("lk", "ran"), Some(1));
    let snap = host.introspect();
    let lazy = snap.lazy_plugins.iter().find(|l| l.plugin_id == "lk").unwrap();
    assert_eq!(lazy.status, "active");
    assert_eq!(lazy.activations, 1);
}

#[test]
fn lazy_event_dispatch_activates_plugin() {
    let mut host = MockHost::new();
    host.load_inline_set(&[(
        "le",
        &lazy_manifest("le", "events = [\"FetchStarted\"]"),
        r#"
        _G.activated = "yes"
        leviathan.autocmd.create("FetchStarted", {
            callback = function(_) _G.fetch_seen = (_G.fetch_seen or 0) + 1 end,
        })
        "#,
    )])
    .expect("resolve and load");

    assert!(host.read_global_string("le", "activated").is_none());

    host.dispatch_test_event("FetchStarted", serde_json::json!({ "remote_name": "origin" }));
    assert_eq!(
        host.read_global_string("le", "activated").as_deref(),
        Some("yes")
    );
    // The freshly installed autocmd observed the same event after
    // activation — exactly one fire.
    assert_eq!(host.read_global_i64("le", "fetch_seen"), Some(1));
    let snap = host.introspect();
    let lazy = snap.lazy_plugins.iter().find(|l| l.plugin_id == "le").unwrap();
    assert_eq!(lazy.status, "active");
}

#[test]
fn lazy_file_presence_activates_plugin() {
    let mut host = MockHost::new();
    let workdir = tempfile::tempdir().expect("workdir");
    let touch_file = workdir.path().join(".eslintrc");
    std::fs::write(&touch_file, "{}\n").expect("write touch file");
    host.load_inline_set(&[(
        "lf",
        &lazy_manifest("lf", "files = [\".eslintrc\"]"),
        r#"_G.activated = "yes""#,
    )])
    .expect("resolve and load");

    assert!(host.read_global_string("lf", "activated").is_none());
    // sync_repository drives the file-presence probe.
    host.host_mut().sync_repository(
        "demo-repo",
        workdir.path().to_str().unwrap(),
        "main",
        "deadbeefdeadbeef",
        "origin",
        &[],
    );
    assert_eq!(
        host.read_global_string("lf", "activated").as_deref(),
        Some("yes")
    );
}

#[test]
fn failed_lazy_activation_is_contained_and_visible() {
    let mut host = MockHost::new();
    host.load_inline_set(&[(
        "boom",
        &lazy_manifest("boom", "commands = [\"boom.run\"]"),
        // init.lua raises during exec.
        r#"error("kaboom")"#,
    )])
    .expect("resolve and load");

    // First invocation triggers activation; load fails. The outcome
    // is `NotFound` (no command was registered) but the host is
    // still healthy.
    let _ = host.invoke_command("boom.run", serde_json::Value::Null);
    let codes = diag_codes(&host);
    assert!(
        codes.iter().any(|c| c == "activation.failed"),
        "expected activation.failed in: {codes:?}"
    );
    let snap = host.introspect();
    let lazy = snap.lazy_plugins.iter().find(|l| l.plugin_id == "boom").unwrap();
    assert_eq!(
        lazy.status, "lazy",
        "transient failure must NOT poison after a single failure"
    );
    assert!(lazy.last_error.is_some());
}

#[test]
fn repeated_failure_poisons_lazy_plugin() {
    let mut host = MockHost::new();
    host.load_inline_set(&[(
        "poison",
        &lazy_manifest("poison", "commands = [\"poison.run\"]"),
        r#"error("repeatedly")"#,
    )])
    .expect("resolve and load");

    let _ = host.invoke_command("poison.run", serde_json::Value::Null);
    let _ = host.invoke_command("poison.run", serde_json::Value::Null);
    let codes = diag_codes(&host);
    assert!(
        codes.iter().any(|c| c == "activation.poisoned"),
        "expected activation.poisoned in: {codes:?}"
    );
    let snap = host.introspect();
    let lazy = snap
        .lazy_plugins
        .iter()
        .find(|l| l.plugin_id == "poison")
        .unwrap();
    assert_eq!(lazy.status, "poisoned");
    // A third invocation must not retry the load.
    let pre_codes = diag_codes(&host);
    let _ = host.invoke_command("poison.run", serde_json::Value::Null);
    let post_codes = diag_codes(&host);
    let extra_failed = post_codes.iter().filter(|c| *c == "activation.failed").count()
        - pre_codes.iter().filter(|c| *c == "activation.failed").count();
    assert_eq!(extra_failed, 0, "poisoned plugin must not retry");
}

#[test]
fn eager_plugin_without_activation_section_loads_immediately() {
    let mut host = MockHost::new();
    host.load_inline_set(&[("eager", EAGER_MANIFEST, r#"_G.eager_loaded = "yes""#)])
        .expect("resolve and load");
    // Eager — Lua ran immediately during resolve_and_load.
    assert_eq!(
        host.read_global_string("eager", "eager_loaded").as_deref(),
        Some("yes")
    );
    let snap = host.introspect();
    assert!(
        snap.plugins.iter().any(|p| p.id == "eager"),
        "eager plugin must appear in the loaded set"
    );
    assert!(
        snap.lazy_plugins.iter().all(|l| l.plugin_id != "eager"),
        "eager plugin must NOT appear in the lazy registry"
    );
}

#[test]
fn lazy_devtools_summary_shape() {
    let mut host = MockHost::new();
    host.load_inline_set(&[(
        "shape",
        &lazy_manifest(
            "shape",
            r#"commands = ["shape.a", "shape.b"]
events = ["FetchStarted"]
regions = ["main_bar"]
manual = true

[[activation.keymaps]]
context = "global"
key = "<leader>x"

[activation.repository_shape]
has_remote = true
"#,
        ),
        r#"
        leviathan.command.create("shape.a", { title="a", description="a", run = function(_) end })
        leviathan.command.create("shape.b", { title="b", description="b", run = function(_) end })
        "#,
    )])
    .expect("resolve and load");
    let snap = host.introspect();
    let lazy = snap
        .lazy_plugins
        .iter()
        .find(|l| l.plugin_id == "shape")
        .expect("shape lazy entry");
    assert_eq!(lazy.status, "lazy");
    assert_eq!(lazy.activations, 0);
    assert!(lazy.last_activation_unix_ms.is_none());
    // Sorted, contains every declared trigger.
    assert!(lazy.triggers.windows(2).all(|w| w[0] <= w[1]));
    assert!(lazy.triggers.iter().any(|t| t == "command:shape.a"));
    assert!(lazy.triggers.iter().any(|t| t == "command:shape.b"));
    assert!(lazy.triggers.iter().any(|t| t == "event:FetchStarted"));
    assert!(lazy.triggers.iter().any(|t| t == "keymap:global:<leader>x"));
    assert!(lazy.triggers.iter().any(|t| t == "region:main_bar"));
    assert!(lazy.triggers.iter().any(|t| t == "repository_shape"));
    assert!(lazy.triggers.iter().any(|t| t == "manual"));
}

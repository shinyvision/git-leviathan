use std::path::PathBuf;

use serde_json::Value;

use crate::plugin::tests::harness::MockHost;

const MANIFEST: &str = r#"
id = "phase13"
name = "phase13"
version = "0.1.0"
api_version = "1.0"
"#;

const SECRET_MANIFEST: &str = r#"
id = "phase13"
name = "phase13"
version = "0.1.0"
api_version = "1.0"
capabilities = ["credentials"]
"#;

fn surface_path(host: &MockHost, surface: &str) -> PathBuf {
    let snap = host.introspect();
    PathBuf::from(
        snap.storage
            .iter()
            .find(|row| row.plugin_id == "phase13" && row.surface == surface)
            .unwrap_or_else(|| panic!("missing surface {surface}"))
            .path
            .clone(),
    )
}

#[test]
fn migration_file_failure_keeps_original_state_file() {
    let mut host = MockHost::new();
    host.load_inline(
        "phase13",
        MANIFEST,
        r#"
        local s = leviathan.persist.open("data", { version = 1 })
        s:set("n", 1)
        "#,
    )
    .expect("initial load");

    let state_file = surface_path(&host, "state").join("data.json");
    host.write_plugin_file(
        "phase13",
        "migrations/data/1_to_2.lua",
        r#"return function(old) error("migration boom") end"#,
    )
    .expect("migration file");

    let result = host.reload_with_str(
        "phase13",
        MANIFEST,
        r#"
        local s = leviathan.persist.open("data", { version = 2 })
        _G.n = s:get("n")
        "#,
    );
    assert!(result.is_err(), "migration failure must abort reload");

    let file: Value = serde_json::from_str(&std::fs::read_to_string(&state_file).unwrap()).unwrap();
    assert_eq!(file.get("version").and_then(Value::as_u64), Some(1));
    assert_eq!(
        file.pointer("/data/n").and_then(Value::as_i64),
        Some(1),
        "failed migration must not commit partial state"
    );
}

#[test]
fn corrupt_state_is_backed_up_and_recreated() {
    let mut host = MockHost::new();
    host.load_inline("phase13", MANIFEST, "").expect("load");
    let state_file = surface_path(&host, "state").join("data.json");
    std::fs::create_dir_all(state_file.parent().unwrap()).unwrap();
    std::fs::write(&state_file, "{not json").unwrap();

    host.reload_with_str(
        "phase13",
        MANIFEST,
        r#"
        local s = leviathan.persist.open("data", { version = 1 })
        _G.missing = s:get("n") == nil and 1 or 0
        s:set("n", 8)
        "#,
    )
    .expect("corrupt state should not break reload");

    assert_eq!(host.read_global_i64("phase13", "missing"), Some(1));
    assert!(state_file.with_extension("json.corrupt").exists());
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&state_file).unwrap()).unwrap();
    assert_eq!(file.pointer("/data/n").and_then(Value::as_i64), Some(8));
}

#[test]
fn settings_validate_before_save_and_fire_on_change() {
    let mut host = MockHost::new();
    host.load_inline(
        "phase13",
        SECRET_MANIFEST,
        r#"
        local ok, err = leviathan.settings.define_schema({
          mode = { type = "string", default = "auto", enum = { "auto", "manual" } },
          limit = { type = "integer", default = 2, min = 1, max = 5 },
        })
        _G.schema_ok = ok and 1 or 0
        _G.default_mode = leviathan.settings.get().mode
        local bad_ok, bad_err = leviathan.settings.set({ limit = 99 })
        _G.bad_ok = bad_ok and 1 or 0
        _G.bad_err = bad_err
        leviathan.settings.on_change(function(new_settings)
          _G.changed_limit = new_settings.limit
        end)
        local good_ok, good_err = leviathan.settings.set({ limit = 4 })
        _G.good_ok = good_ok and 1 or 0
        _G.good_err = good_err or ""
        "#,
    )
    .expect("load");

    assert_eq!(host.read_global_i64("phase13", "schema_ok"), Some(1));
    assert_eq!(
        host.read_global_string("phase13", "default_mode")
            .as_deref(),
        Some("auto")
    );
    assert_eq!(host.read_global_i64("phase13", "bad_ok"), Some(0));
    assert!(host
        .read_global_string("phase13", "bad_err")
        .unwrap()
        .contains("limit"));
    assert_eq!(host.read_global_i64("phase13", "good_ok"), Some(1));
    assert_eq!(host.read_global_i64("phase13", "changed_limit"), Some(4));

    let settings = host.introspect().settings;
    let row = settings
        .iter()
        .find(|row| row.plugin_id == "phase13")
        .unwrap();
    assert!(
        row.valid,
        "settings metadata should validate: {:?}",
        row.errors
    );
    assert!(PathBuf::from(&row.path).ends_with("settings.json"));
    assert!(row.schema_keys.contains(&"limit".to_string()));
    assert!(row.value_keys.contains(&"limit".to_string()));
}

#[test]
fn reset_surfaces_and_secret_metadata_do_not_expose_secret_values() {
    let mut host = MockHost::new();
    host.load_inline(
        "phase13",
        SECRET_MANIFEST,
        r#"
        leviathan.persist.open("state_data", { version = 1 }):set("token", "state-value")
        leviathan.persist.open("config_data", { version = 1, surface = "config" }):set("n", 2)
        leviathan.persist.open("cache_data", { version = 1, surface = "cache" }):set("n", 3)
        leviathan.settings.define_schema({ enabled = { type = "boolean", default = true } })
        leviathan.settings.set({ enabled = false })
        leviathan.secrets.set("api_token", "super-secret-value")
        "#,
    )
    .expect("load");

    let snap = host.introspect();
    let secret = snap
        .secrets
        .iter()
        .find(|row| row.plugin_id == "phase13")
        .unwrap();
    assert_eq!(secret.key_count, 1);
    assert_eq!(secret.keys, vec!["api_token".to_string()]);
    assert!(PathBuf::from(&secret.path).ends_with("secrets.json"));

    let state_dir = surface_path(&host, "state");
    let config_dir = surface_path(&host, "config");
    let state_text = read_tree_text(&state_dir);
    let config_text = read_tree_text(&config_dir);
    assert!(!state_text.contains("super-secret-value"));
    assert!(!config_text.contains("super-secret-value"));

    host.reset_plugin_storage("phase13", "cache")
        .expect("reset cache");
    let snap = host.introspect();
    let cache = snap
        .storage
        .iter()
        .find(|row| row.plugin_id == "phase13" && row.surface == "cache")
        .unwrap();
    assert!(!cache.exists);
    assert_eq!(cache.file_count, 0);
    assert_eq!(cache.byte_count, 0);
    assert!(cache.corrupt_files.is_empty());

    host.reset_plugin_storage("phase13", "secrets")
        .expect("reset secrets");
    let snap = host.introspect();
    let secret = snap
        .secrets
        .iter()
        .find(|row| row.plugin_id == "phase13")
        .unwrap();
    assert_eq!(secret.key_count, 0);
}

#[test]
fn secrets_require_credentials_capability() {
    let mut host = MockHost::new();
    let result = host.load_inline(
        "phase13",
        MANIFEST,
        r#"leviathan.secrets.set("api_token", "super-secret-value")"#,
    );
    let err = result
        .expect_err("credentials capability should be required")
        .to_string();
    assert!(
        err.contains("credentials") || err.contains("capability"),
        "got: {err}"
    );
    assert!(
        host.diagnostics()
            .by_code("capability.denied")
            .iter()
            .any(|d| d.context["capability"] == "credentials"),
        "missing credentials denial diagnostic"
    );
}

#[test]
fn persist_transaction_rolls_back_on_callback_failure() {
    let mut host = MockHost::new();
    host.load_inline(
        "phase13",
        MANIFEST,
        r#"
        local ok = pcall(function()
          leviathan.persist.transaction(function(tx)
            tx:set("a", 1)
            error("stop")
          end)
        end)
        _G.failed = ok and 0 or 1
        leviathan.persist.transaction(function(tx)
          tx:set("b", 2)
        end)
        local s = leviathan.persist.open("default", { version = 1 })
        _G.a_missing = s:get("a") == nil and 1 or 0
        _G.b = s:get("b")
        "#,
    )
    .expect("load");

    assert_eq!(host.read_global_i64("phase13", "failed"), Some(1));
    assert_eq!(host.read_global_i64("phase13", "a_missing"), Some(1));
    assert_eq!(host.read_global_i64("phase13", "b"), Some(2));
}

fn read_tree_text(path: &std::path::Path) -> String {
    let mut out = String::new();
    let Ok(entries) = std::fs::read_dir(path) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.push_str(&read_tree_text(&path));
        } else if let Ok(raw) = std::fs::read_to_string(path) {
            out.push_str(&raw);
        }
    }
    out
}

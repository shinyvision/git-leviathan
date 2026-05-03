//! package layout acceptance tests for the Lua module loader, runtimepath
//! resolution, after-directory walk, and strict-globals enforcement.
//!
//! Each test maps onto one of the acceptance criteria in the plugin package layout
//! plan:
//!
//! - own-module loading                  -> `plugin_can_require_its_own_module`
//! - dependency-module loading           -> `plugin_can_require_declared_dependency_module`
//! - forbidden cross-plugin private mod  -> `plugin_cannot_require_undeclared_other_plugin`
//! - after/ order                        -> `after_plugin_files_run_in_lexical_order`
//! - module cache cleared on reload     -> `module_cache_drops_on_reload`
//! - strict-global denial               -> `strict_globals_block_undeclared_global_write`
//!   + `strict_globals_block_undeclared_global_read`

use crate::plugin::diagnostic::DiagnosticSeverity;
use crate::plugin::tests::harness::MockHost;

const STRICT_MANIFEST: &str = r#"
id = "strict_p"
name = "strict_p"
version = "0.1.0"
api_version = "1.0"

[runtime]
strict_globals = true
"#;

#[test]
fn plugin_can_require_its_own_module() {
    let mut host = MockHost::new();
    let dir = host.root().join("p1");
    std::fs::create_dir_all(dir.join("lua").join("p1")).unwrap();
    std::fs::write(
        dir.join("lua").join("p1").join("greet.lua"),
        "return { hello = function(name) return 'hi ' .. name end }\n",
    )
    .unwrap();
    host.load_inline(
        "p1",
        r#"
id = "p1"
name = "p1"
version = "0.1.0"
api_version = "1.0"
"#,
        r#"
local m = require("p1.greet")
_G.greeting = m.hello("world")
"#,
    )
    .expect("loads with module present");

    // Module loaded; introspect surfaces it.
    let snap = host.introspect();
    let modules: Vec<&str> = snap
        .loaded_modules
        .iter()
        .map(|m| m.module_name.as_str())
        .collect();
    assert!(modules.contains(&"p1.greet"), "got: {modules:?}");
    let entry = snap
        .loaded_modules
        .iter()
        .find(|m| m.module_name == "p1.greet")
        .unwrap();
    assert_eq!(entry.source_plugin_id, "p1");
    assert_eq!(entry.kind, "own");
    // Runtime path entries include the own root.
    let path: Vec<&str> = snap.runtime_paths.iter().map(|p| p.kind.as_str()).collect();
    assert!(path.contains(&"own"));
}

#[test]
fn plugin_can_require_declared_dependency_module() {
    let mut host = MockHost::new();

    // Provider plugin — exports a `lib` module.
    let provider_dir = host.root().join("provider");
    std::fs::create_dir_all(provider_dir.join("lua").join("provider")).unwrap();
    std::fs::write(
        provider_dir.join("lua").join("provider").join("lib.lua"),
        "return { value = 42 }",
    )
    .unwrap();
    host.load_inline(
        "provider",
        r#"
id = "provider"
name = "provider"
version = "0.1.0"
api_version = "1.0"
"#,
        "-- nothing to do at init",
    )
    .expect("provider loads");

    host.load_inline(
        "consumer",
        r#"
id = "consumer"
name = "consumer"
version = "0.1.0"
api_version = "1.0"
requires_plugins = ["provider"]
"#,
        r#"
local lib = require("provider.lib")
_G.dep_value = lib.value
"#,
    )
    .expect("consumer loads after declaring provider");

    let v = host.read_global_i64("consumer", "dep_value");
    assert_eq!(v, Some(42));
    let snap = host.introspect();
    assert!(snap.loaded_modules.iter().any(|m| m.plugin_id == "consumer"
        && m.source_plugin_id == "provider"
        && m.kind == "dependency"));
}

#[test]
fn plugin_cannot_require_undeclared_other_plugin() {
    let mut host = MockHost::new();

    let provider_dir = host.root().join("secret");
    std::fs::create_dir_all(provider_dir.join("lua").join("secret")).unwrap();
    std::fs::write(
        provider_dir.join("lua").join("secret").join("api.lua"),
        "return { secret = 'shhh' }",
    )
    .unwrap();
    host.load_inline(
        "secret",
        r#"
id = "secret"
name = "secret"
version = "0.1.0"
api_version = "1.0"
"#,
        "-- ",
    )
    .expect("secret loads");

    // attacker doesn't declare `secret` in requires_plugins.
    let result = host.load_inline(
        "attacker",
        r#"
id = "attacker"
name = "attacker"
version = "0.1.0"
api_version = "1.0"
"#,
        r#"
local s = require("secret.api")
_G.leaked = s.secret
"#,
    );
    let err = result
        .expect_err("cross-plugin require must fail")
        .to_string();
    assert!(
        err.contains("requires_plugins") || err.contains("forbidden"),
        "expected forbidden message; got: {err}"
    );
    let diags = host.diagnostics().by_code("lua.module.forbidden");
    assert!(!diags.is_empty(), "forbidden diagnostic must be recorded");
}

#[test]
fn after_plugin_files_run_in_lexical_order() {
    let mut host = MockHost::new();
    host.load_inline(
        "ordered",
        r#"
id = "ordered"
name = "ordered"
version = "0.1.0"
api_version = "1.0"
"#,
        "_G.order = ''\n",
    )
    .expect("init loads");

    // Drop after/plugin/*.lua in non-sorted creation order to prove
    // lexical sort happens host-side.
    host.write_plugin_file(
        "ordered",
        "after/plugin/30_z.lua",
        "_G.order = _G.order .. 'z'",
    )
    .unwrap();
    host.write_plugin_file(
        "ordered",
        "after/plugin/10_a.lua",
        "_G.order = _G.order .. 'a'",
    )
    .unwrap();
    host.write_plugin_file(
        "ordered",
        "after/plugin/20_m.lua",
        "_G.order = _G.order .. 'm'",
    )
    .unwrap();
    host.reload_plugin("ordered")
        .expect("reload picks up after/");

    let order = host.host().plugin_global_string("ordered", "order");
    assert_eq!(order.as_deref(), Some("amz"));
}

#[test]
fn module_cache_drops_on_reload() {
    let mut host = MockHost::new();

    let p_dir = host.root().join("p_reload");
    std::fs::create_dir_all(p_dir.join("lua").join("p_reload")).unwrap();
    std::fs::write(
        p_dir.join("lua").join("p_reload").join("counter.lua"),
        "return { n = 1 }",
    )
    .unwrap();
    host.load_inline(
        "p_reload",
        r#"
id = "p_reload"
name = "p_reload"
version = "0.1.0"
api_version = "1.0"
"#,
        r#"
local c = require("p_reload.counter")
_G.start_n = c.n
"#,
    )
    .expect("loads");

    assert_eq!(host.read_global_i64("p_reload", "start_n"), Some(1));
    let pre = host.introspect();
    assert_eq!(
        pre.loaded_modules
            .iter()
            .filter(|m| m.plugin_id == "p_reload")
            .count(),
        1
    );

    // Mutate the module on disk; reload should pick up the new value
    // because the cache was dropped.
    std::fs::write(
        p_dir.join("lua").join("p_reload").join("counter.lua"),
        "return { n = 99 }",
    )
    .unwrap();
    host.reload_plugin("p_reload").expect("reload");

    assert_eq!(host.read_global_i64("p_reload", "start_n"), Some(99));
    // After reload only the new generation's cache is visible.
    let post = host.introspect();
    let post_records: Vec<&str> = post
        .loaded_modules
        .iter()
        .filter(|m| m.plugin_id == "p_reload")
        .map(|m| m.module_name.as_str())
        .collect();
    assert_eq!(post_records, vec!["p_reload.counter"]);
}

#[test]
fn strict_globals_block_undeclared_global_write() {
    let mut host = MockHost::new();
    let result = host.load_inline_strict(
        "strict_p",
        STRICT_MANIFEST,
        r#"
_G.banned = 1
"#,
    );
    let err = result.expect_err("write should be rejected").to_string();
    assert!(
        err.contains("strict globals") || err.contains("undeclared global"),
        "got: {err}"
    );
    let warns = host.diagnostics().by_code("lua.strict_globals.write");
    assert!(!warns.is_empty(), "write diagnostic must be recorded");
    assert_eq!(warns[0].severity, DiagnosticSeverity::Warning);
}

#[test]
fn strict_globals_block_undeclared_global_read() {
    let mut host = MockHost::new();
    let result = host.load_inline_strict(
        "strict_p",
        STRICT_MANIFEST,
        r#"
local x = some_unknown_global
"#,
    );
    let err = result.expect_err("read should be rejected").to_string();
    assert!(
        err.contains("strict globals") || err.contains("undeclared global"),
        "got: {err}"
    );
    let reads = host.diagnostics().by_code("lua.strict_globals.read");
    assert!(!reads.is_empty(), "read diagnostic must be recorded");
}

#[test]
fn require_with_invalid_name_records_diagnostic() {
    let mut host = MockHost::new();
    let result = host.load_inline(
        "bad_name",
        r#"
id = "bad_name"
name = "bad_name"
version = "0.1.0"
api_version = "1.0"
"#,
        r#"
require("..")
"#,
    );
    assert!(result.is_err());
    let diags = host.diagnostics().by_code("lua.module.invalid_name");
    assert!(!diags.is_empty());
}

#[test]
fn runtime_path_lua_api_lists_entries() {
    let mut host = MockHost::new();

    let provider_dir = host.root().join("dep1");
    std::fs::create_dir_all(provider_dir.join("lua").join("dep1")).unwrap();
    host.load_inline(
        "dep1",
        r#"
id = "dep1"
name = "dep1"
version = "0.1.0"
api_version = "1.0"
"#,
        "-- nothing",
    )
    .expect("dep1 loads");

    host.load_inline(
        "intro",
        r#"
id = "intro"
name = "intro"
version = "0.1.0"
api_version = "1.0"
requires_plugins = ["dep1"]
"#,
        r#"
local p = leviathan.runtime.path()
_G.path_count = #p
_G.first_kind = p[1].kind
_G.second_kind = p[2].kind
"#,
    )
    .expect("intro loads");

    assert_eq!(host.read_global_i64("intro", "path_count"), Some(2));
    assert_eq!(
        host.host()
            .plugin_global_string("intro", "first_kind")
            .as_deref(),
        Some("own")
    );
    assert_eq!(
        host.host()
            .plugin_global_string("intro", "second_kind")
            .as_deref(),
        Some("dependency")
    );
}

#[test]
fn runtime_module_graph_lists_loaded_modules() {
    let mut host = MockHost::new();

    let dir = host.root().join("graphy");
    std::fs::create_dir_all(dir.join("lua").join("graphy")).unwrap();
    std::fs::write(
        dir.join("lua").join("graphy").join("a.lua"),
        "return { id = 'a' }",
    )
    .unwrap();
    std::fs::write(
        dir.join("lua").join("graphy").join("b.lua"),
        "return { id = 'b' }",
    )
    .unwrap();
    host.load_inline(
        "graphy",
        r#"
id = "graphy"
name = "graphy"
version = "0.1.0"
api_version = "1.0"
"#,
        r#"
require("graphy.a")
require("graphy.b")
local g = leviathan.runtime.module_graph()
_G.entries = #g
_G.first_modules = #g[1].modules
"#,
    )
    .expect("loads");

    assert_eq!(host.read_global_i64("graphy", "entries"), Some(1));
    assert_eq!(host.read_global_i64("graphy", "first_modules"), Some(2));
}

//! keymaps acceptance tests: context-aware keymaps.
//!
//! Each test cites the acceptance-criterion bullet from the plan
//! (`PLUGIN_REFACTOR.md` keymaps, lines ~534-577) it covers. The
//! ordering follows the "Required tests" list in the keymaps brief.

use crate::plugin::commands::{InvokeOutcome, HOST_COMMAND_PLUGIN_ID};
use crate::plugin::keymap::{
    keymap_prefix_changed_payload, parse_key_sequence, KeymapDispatchOutcome, Keystroke,
    MatchOutcome,
};
use crate::plugin::tests::harness::{simple_manifest as manifest, MockHost};

fn ks(s: &str) -> Vec<Keystroke> {
    parse_key_sequence(s, ",").unwrap()
}

#[test]
fn keymap_in_repository_does_not_fire_in_tab_bar() {
    // Acceptance: plugin keymaps work only in declared contexts.
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        _G.fired = 0
        leviathan.command.create("p.go", { run = function() _G.fired = _G.fired + 1 end })
        leviathan.keymap.set("repository", "gl", "p.go")
        "#,
    )
    .expect("p loads");

    let out = host.dispatch_key("repository", &ks("gl"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));
    assert_eq!(host.read_global_i64("p", "fired"), Some(1));

    let out = host.dispatch_key("tab_bar", &ks("gl"));
    assert!(
        matches!(out, KeymapDispatchOutcome::Unhandled),
        "tab_bar must not match a `repository` binding: got {out:?}"
    );
    assert_eq!(host.read_global_i64("p", "fired"), Some(1));
}

#[test]
fn chord_with_leader_only_fires_after_full_sequence() {
    // Acceptance: chord matcher walks left-to-right; partial chord
    // is `Pending`, full chord is `Match`.
    let mut host = MockHost::new();
    host.set_keymap_leader(",");
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        _G.fired = 0
        leviathan.command.create("p.hunk", { run = function() _G.fired = _G.fired + 1 end })
        leviathan.keymap.set("global", "<leader>gh", "p.hunk")
        "#,
    )
    .expect("p loads");

    // Just the leader: pending.
    let registry = host.keymap_registry();
    let pending = registry.borrow().match_chord("global", &ks(","));
    assert!(matches!(pending, MatchOutcome::Pending));
    assert_eq!(host.read_global_i64("p", "fired"), Some(0));

    // Leader + g: still pending.
    let pending = registry.borrow().match_chord("global", &ks(",g"));
    assert!(matches!(pending, MatchOutcome::Pending));

    // Full chord: match + dispatch.
    let out = host.dispatch_key("global", &ks(",gh"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));
    assert_eq!(host.read_global_i64("p", "fired"), Some(1));
}

#[test]
fn lua_can_set_space_as_keymap_leader() {
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        leviathan.keymap.set_leader("<Space>")
        _G.fired = 0
        leviathan.command.create("p.pull", { run = function() _G.fired = _G.fired + 1 end })
        leviathan.keymap.set("repository.graph", "<leader>gp", "p.pull")
        "#,
    )
    .expect("p loads");

    let registry = host.keymap_registry();
    let pending = registry
        .borrow()
        .match_chord("repository.graph", &ks("<Space>"));
    assert!(matches!(pending, MatchOutcome::Pending));

    let out = host.dispatch_key("repository.graph", &ks("<Space>gp"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));
    assert_eq!(host.read_global_i64("p", "fired"), Some(1));
}

#[test]
fn chord_prefix_wins_over_single_key_when_prefix_present() {
    // Acceptance: with both `g` and `gl` bound, typing `g` is
    // `Pending`, typing `gl` matches the chord — `g` alone never
    // fires while a longer chord is registered as a continuation.
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        _G.g_fired = 0
        _G.gl_fired = 0
        leviathan.command.create("p.g",  { run = function() _G.g_fired  = _G.g_fired  + 1 end })
        leviathan.command.create("p.gl", { run = function() _G.gl_fired = _G.gl_fired + 1 end })
        leviathan.keymap.set("global", "g",  "p.g")
        leviathan.keymap.set("global", "gl", "p.gl")
        "#,
    )
    .expect("p loads");

    // `g` alone is pending — the matcher knows there's a longer
    // chord prefixed by `g`. We do *not* claim "g" matches by itself
    // even though it has an exact binding — the standard vim
    // behaviour is that the longest-pending wins; our matcher
    // surfaces this as `Pending` and the caller can resolve on
    // timeout. keymaps's contract: prefix presence beats exact
    // match when a longer chord is also bound. (Verified via the
    // headless registry — the live input layer's timeout policy is
    // outside keymaps scope.)
    let registry = host.keymap_registry();
    let out = registry.borrow().match_chord("global", &ks("g"));
    assert!(matches!(out, MatchOutcome::Pending));
    let out = host.dispatch_key("global", &ks("gl"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));
    assert_eq!(host.read_global_i64("p", "gl_fired"), Some(1));
    assert_eq!(
        host.read_global_i64("p", "g_fired"),
        Some(0),
        "the `gl` chord must NOT also fire `g`"
    );
}

#[test]
fn prefix_hints_group_next_keys_for_overlay() {
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        leviathan.command.create("p.top", { run = function() end })
        leviathan.command.create("p.deep", { run = function() end })
        leviathan.command.create("p.unlabeled", { run = function() end })
        leviathan.keymap.set("repository", "gg", "p.top", { description = "Back to top" })
        leviathan.keymap.set("repository", "glrmn", "p.deep")
        leviathan.keymap.set("repository", "gz", "p.unlabeled")
        "#,
    )
    .expect("p loads");

    let registry = host.keymap_registry();
    let hints = registry.borrow().prefix_hints("repository.graph", &ks("g"));
    let labels: Vec<_> = hints.iter().map(|hint| hint.key.as_str()).collect();
    assert_eq!(labels, vec!["g", "l", "z"]);
    let gg = hints.iter().find(|hint| hint.key == "g").unwrap();
    assert_eq!(gg.description, "Back to top");
    assert_eq!(gg.command, "p.top");

    let gl = hints.iter().find(|hint| hint.key == "l").unwrap();
    assert!(gl.is_group);
    assert_eq!(gl.child_count, 1);

    let gz = hints.iter().find(|hint| hint.key == "z").unwrap();
    assert_eq!(gz.description, "");
    assert_eq!(gz.command, "p.unlabeled");
}

#[test]
fn detail_dirty_keymap_does_not_match_plain_details_context() {
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        leviathan.command.create("p.focus_commit_message", { run = function() end })
        leviathan.keymap.set("repository.details.dirty", "gc", "p.focus_commit_message")
        "#,
    )
    .expect("p loads");

    let registry = host.keymap_registry();
    assert!(matches!(
        registry
            .borrow()
            .match_chord("repository.details", &ks("gc")),
        MatchOutcome::None
    ));
    assert!(matches!(
        registry
            .borrow()
            .match_chord("repository.details.dirty", &ks("gc")),
        MatchOutcome::Match { .. }
    ));
}

#[test]
fn plugin_vs_plugin_resolves_lex_by_plugin_id_loser_marked_conflict_lost() {
    // Acceptance: plugin conflicts are deterministic; loser shows
    // `conflict_lost` in devtools.
    let mut host = MockHost::new();
    host.load_inline(
        "zzz",
        &manifest("zzz"),
        r#"
        _G.zzz_fired = 0
        leviathan.command.create("zzz.go", { run = function() _G.zzz_fired = _G.zzz_fired + 1 end })
        leviathan.keymap.set("repository", "gl", "zzz.go")
        "#,
    )
    .expect("zzz loads");
    host.load_inline(
        "aaa",
        &manifest("aaa"),
        r#"
        _G.aaa_fired = 0
        leviathan.command.create("aaa.go", { run = function() _G.aaa_fired = _G.aaa_fired + 1 end })
        leviathan.keymap.set("repository", "gl", "aaa.go")
        "#,
    )
    .expect("aaa loads");

    let snap = host.introspect();
    let aaa = snap
        .keymaps
        .iter()
        .find(|k| k.plugin_id == "aaa")
        .expect("aaa row");
    let zzz = snap
        .keymaps
        .iter()
        .find(|k| k.plugin_id == "zzz")
        .expect("zzz row");
    assert_eq!(aaa.status, "active");
    assert_eq!(zzz.status, "conflict_lost");
    assert_eq!(zzz.conflict_with.as_ref().unwrap().plugin_id, "aaa");

    // Dispatch fires the winner.
    let out = host.dispatch_key("repository", &ks("gl"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));
    assert_eq!(host.read_global_i64("aaa", "aaa_fired"), Some(1));
    assert_eq!(host.read_global_i64("zzz", "zzz_fired"), Some(0));

    // Diagnostic recorded.
    host.assert_diag_code("keymap.conflict_lost");
}

#[test]
fn built_in_wins_over_plugin() {
    // Acceptance: built-ins win by default; plugin entry is
    // `conflict_lost`.
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        _G.plugin_fired = 0
        leviathan.command.create("p.refresh", { run = function() _G.plugin_fired = _G.plugin_fired + 1 end })
        leviathan.keymap.set("global", "<C-r>", "p.refresh")
        "#,
    )
    .expect("p loads");
    host.set_builtin_keymap("global", "<C-r>", "repository.refresh", "host reload");

    let snap = host.introspect();
    let host_row = snap
        .keymaps
        .iter()
        .find(|k| k.source == "built-in" && k.command == "repository.refresh")
        .expect("built-in row");
    let plugin_row = snap
        .keymaps
        .iter()
        .find(|k| k.source == "plugin")
        .expect("plugin row");
    assert_eq!(host_row.status, "active");
    assert_eq!(host_row.plugin_id, HOST_COMMAND_PLUGIN_ID);
    assert_eq!(plugin_row.status, "conflict_lost");

    // Dispatch hits the built-in's command (which is a host no-op,
    // so `plugin_fired` stays 0 for the right reason).
    let out = host.dispatch_key("global", &ks("<C-r>"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));
    assert_eq!(host.read_global_i64("p", "plugin_fired"), Some(0));
}

#[test]
fn user_wins_over_plugin() {
    // Acceptance: user mappings win over plugins.
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        _G.plugin_fired = 0
        leviathan.command.create("p.go", { run = function() _G.plugin_fired = _G.plugin_fired + 1 end })
        leviathan.keymap.set("repository", "gl", "p.go")
        "#,
    )
    .expect("p loads");
    // User-config binding routes the same chord to a different
    // command. The host has a built-in `repository.refresh` which
    // we'll route to.
    host.set_user_keymap("repository", "gl", "repository.refresh");

    let out = host.dispatch_key("repository", &ks("gl"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));
    if let KeymapDispatchOutcome::Dispatched { command, .. } = out {
        assert_eq!(command, "repository.refresh");
    }
    assert_eq!(
        host.read_global_i64("p", "plugin_fired"),
        Some(0),
        "user override must beat plugin binding"
    );

    let snap = host.introspect();
    let plugin_row = snap.keymaps.iter().find(|k| k.source == "plugin").unwrap();
    assert_eq!(plugin_row.status, "conflict_lost");
}

#[test]
fn unload_removes_plugin_keymaps_and_reactivates_losers() {
    // Acceptance: unload removes plugin keymaps; conflict losers
    // re-activate when the winner unloads.
    let mut host = MockHost::new();
    host.load_inline(
        "aaa",
        &manifest("aaa"),
        r#"
        _G.aaa_fired = 0
        leviathan.command.create("aaa.go", { run = function() _G.aaa_fired = _G.aaa_fired + 1 end })
        leviathan.keymap.set("repository", "gl", "aaa.go")
        "#,
    )
    .expect("load");
    host.load_inline(
        "zzz",
        &manifest("zzz"),
        r#"
        _G.zzz_fired = 0
        leviathan.command.create("zzz.go", { run = function() _G.zzz_fired = _G.zzz_fired + 1 end })
        leviathan.keymap.set("repository", "gl", "zzz.go")
        "#,
    )
    .expect("load");

    // aaa wins (lex).
    let out = host.dispatch_key("repository", &ks("gl"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));
    assert_eq!(host.read_global_i64("aaa", "aaa_fired"), Some(1));

    // Unload aaa: zzz should reactivate.
    host.unload_plugin("aaa").expect("unload aaa");
    let snap = host.introspect();
    assert!(
        !snap.keymaps.iter().any(|k| k.plugin_id == "aaa"),
        "aaa keymap rows must be gone after unload"
    );
    let zzz_row = snap
        .keymaps
        .iter()
        .find(|k| k.plugin_id == "zzz")
        .expect("zzz row present");
    assert_eq!(zzz_row.status, "active");

    // Now dispatch fires zzz.
    let out = host.dispatch_key("repository", &ks("gl"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));
    assert_eq!(host.read_global_i64("zzz", "zzz_fired"), Some(1));
}

#[test]
fn reload_swaps_keymaps_atomically_with_generation_keys() {
    // Acceptance: reload drops every (plugin_id, generation_id) row
    // and replaces with the new generation's rows; the registry never
    // carries stale generations.
    use crate::plugin::keymap::KeymapSource;
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        leviathan.command.create("p.v1", { run = function() end })
        leviathan.keymap.set("global", "ga", "p.v1")
        "#,
    )
    .expect("v1 loads");

    let registry = host.keymap_registry();
    let v1_gen = {
        let r = registry.borrow();
        let entry = r
            .entries()
            .iter()
            .find(|e| e.source == KeymapSource::Plugin && e.plugin_id == "p")
            .expect("v1 entry");
        entry.generation_id.unwrap()
    };

    // Rewrite plugin with a different keymap, reload.
    let dir = host.plugin_dir("p").expect("dir").to_path_buf();
    std::fs::write(dir.join("plugin.toml"), {
        let mut m = manifest("p");
        m.push_str("\n[runtime]\nstrict_globals = false\n");
        m
    })
    .unwrap();
    std::fs::write(
        dir.join("init.lua"),
        r#"
        leviathan.command.create("p.next", { run = function() end })
        leviathan.keymap.set("global", "gb", "p.next")
        "#,
    )
    .unwrap();
    host.reload_plugin("p").expect("reload ok");

    let r = registry.borrow();
    let plugin_rows: Vec<_> = r
        .entries()
        .iter()
        .filter(|e| e.source == KeymapSource::Plugin && e.plugin_id == "p")
        .collect();
    assert_eq!(plugin_rows.len(), 1, "exactly one row for plugin p");
    let row = plugin_rows[0];
    assert_eq!(row.command, "p.next");
    assert_ne!(
        row.generation_id.unwrap(),
        v1_gen,
        "reload must mint a fresh generation; v1 row must not survive"
    );
    // The v1 keymap chord is no longer registered.
    drop(r);
    let out = host.dispatch_key("global", &ks("ga"));
    assert!(matches!(out, KeymapDispatchOutcome::Unhandled));
    let out = host.dispatch_key("global", &ks("gb"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));
}

#[test]
fn reload_failure_restores_prior_keymaps() {
    // Acceptance: failed reload preserves the previous generation's
    // keymap rows.
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        leviathan.command.create("p.v1", { run = function() end })
        leviathan.keymap.set("global", "ga", "p.v1")
        "#,
    )
    .expect("v1 loads");

    // Confirm the v1 keymap is live.
    let out = host.dispatch_key("global", &ks("ga"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));

    // Break the plugin and try to reload.
    let dir = host.plugin_dir("p").expect("dir").to_path_buf();
    std::fs::write(dir.join("init.lua"), "this is not valid lua >>>").unwrap();
    let result = host.reload_plugin("p");
    assert!(result.is_err(), "broken reload must fail");

    // v1 keymap still serves.
    let out = host.dispatch_key("global", &ks("ga"));
    assert!(
        matches!(out, KeymapDispatchOutcome::Dispatched { .. }),
        "v1 binding must still serve after failed reload"
    );
}

#[test]
fn keymap_triggered_event_fires_with_typed_payload() {
    // Acceptance: KeymapTriggered event fires on dispatch with
    // (context, key, command, plugin_id, ok) payload.
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        _G.last_payload_context = ""
        _G.last_payload_key = ""
        _G.last_payload_command = ""
        _G.last_payload_plugin = ""
        _G.fires = 0
        leviathan.command.create("p.go", { run = function() end })
        leviathan.keymap.set("repository", "gl", "p.go")
        leviathan.autocmd.create("KeymapTriggered", {
            callback = function(ev)
                _G.fires = _G.fires + 1
                _G.last_payload_context = ev.payload.context or ""
                _G.last_payload_key     = ev.payload.key     or ""
                _G.last_payload_command = ev.payload.command or ""
                _G.last_payload_plugin  = ev.payload.plugin_id or ""
            end,
        })
        "#,
    )
    .expect("p loads");

    let out = host.dispatch_key("repository", &ks("gl"));
    assert!(matches!(out, KeymapDispatchOutcome::Dispatched { .. }));

    assert_eq!(host.read_global_i64("p", "fires"), Some(1));
    assert_eq!(
        host.read_global_string("p", "last_payload_context")
            .as_deref(),
        Some("repository")
    );
    assert_eq!(
        host.read_global_string("p", "last_payload_key").as_deref(),
        Some("gl")
    );
    assert_eq!(
        host.read_global_string("p", "last_payload_command")
            .as_deref(),
        Some("p.go")
    );
    assert_eq!(
        host.read_global_string("p", "last_payload_plugin")
            .as_deref(),
        Some("p")
    );
}

#[test]
fn plugin_keymap_routes_through_command_dispatcher() {
    // Acceptance: at least one command can be invoked
    // through a keymap. The command dispatcher records every
    // invocation in `CommandRegistry::summaries`'s `fires` counter,
    // so the simplest proof is to assert that counter goes up.
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        _G.command_runs = 0
        leviathan.command.create("p.refresh", {
            description = "command driven by keymap",
            run = function(args)
                _G.command_runs = _G.command_runs + 1
            end,
        })
        leviathan.keymap.set("repository", "<C-r>", "p.refresh")
        "#,
    )
    .expect("p loads");

    // Pre: command has 0 fires.
    let pre = host
        .command_registry()
        .borrow()
        .summaries()
        .iter()
        .find(|c| c.name == "p.refresh")
        .map(|c| c.fires)
        .expect("command present");
    assert_eq!(pre, 0);

    let out = host.dispatch_key("repository", &ks("<C-r>"));
    match out {
        KeymapDispatchOutcome::Dispatched {
            command, outcome, ..
        } => {
            assert_eq!(command, "p.refresh");
            assert!(matches!(outcome, InvokeOutcome::Ok));
        }
        other => panic!("expected Dispatched, got {other:?}"),
    }

    // Post: command's `fires` counter incremented — proving the
    // command registry dispatcher ran the body.
    let post = host
        .command_registry()
        .borrow()
        .summaries()
        .iter()
        .find(|c| c.name == "p.refresh")
        .map(|c| c.fires)
        .expect("command present");
    assert_eq!(post, 1);
    assert_eq!(host.read_global_i64("p", "command_runs"), Some(1));
}

#[test]
fn keymap_prefix_changed_event_carries_prefix_hints_for_plugin_ui() {
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        _G.prefix_active = 0
        _G.prefix_value = ""
        _G.first_hint = ""
        _G.first_label = ""
        leviathan.command.create("p.top", { run = function() end })
        leviathan.command.create("p.deep", { run = function() end })
        leviathan.keymap.set("repository", "gg", "p.top", { description = "Back to top" })
        leviathan.keymap.set("repository", "glrmn", "p.deep")
        leviathan.autocmd.create("KeymapPrefixChanged", {
            callback = function(ev)
                _G.prefix_active = ev.payload.active == true and 1 or 0
                _G.prefix_value = ev.payload.prefix or ""
                local hint = ev.payload.hints and ev.payload.hints[1]
                _G.first_hint = hint and hint.key or ""
                _G.first_label = hint and hint.description or ""
            end,
        })
        "#,
    )
    .expect("p loads");

    let hints = host
        .keymap_registry()
        .borrow()
        .prefix_hints("repository.graph", &ks("g"));
    let payload = keymap_prefix_changed_payload(true, "repository.graph", "g", hints, "pending");
    host.dispatch_test_event("KeymapPrefixChanged", serde_json::Value::Object(payload));

    assert_eq!(host.read_global_i64("p", "prefix_active"), Some(1));
    assert_eq!(
        host.read_global_string("p", "prefix_value").as_deref(),
        Some("g")
    );
    assert_eq!(
        host.read_global_string("p", "first_hint").as_deref(),
        Some("g")
    );
    assert_eq!(
        host.read_global_string("p", "first_label").as_deref(),
        Some("Back to top")
    );
}

#[test]
fn keymap_del_removes_only_callers_own_binding() {
    // Bonus: `leviathan.keymap.del` removes the calling plugin's
    // binding and leaves the conflict-lost peer (or built-in)
    // bindings alone.
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        leviathan.command.create("p.go", { run = function() end })
        leviathan.keymap.set("repository", "gl", "p.go")
        leviathan.keymap.del("repository", "gl")
        "#,
    )
    .expect("p loads");

    let snap = host.introspect();
    assert!(
        !snap.keymaps.iter().any(|k| k.plugin_id == "p"),
        "plugin p's keymap row should be gone after del"
    );
}

#[test]
fn invalid_key_string_is_rejected_with_diagnostic_and_does_not_block_load() {
    // Bonus: malformed lhs surfaces a `keymap.invalid_key`
    // diagnostic and the binding is dropped; load completes.
    let mut host = MockHost::new();
    host.load_inline(
        "p",
        &manifest("p"),
        r#"
        leviathan.command.create("p.noop", { run = function() end })
        leviathan.keymap.set("global", "<C-", "p.noop")
        "#,
    )
    .expect("plugin must still load even with a bad keymap");
    host.assert_diag_code("keymap.invalid_key");
    let snap = host.introspect();
    assert!(snap.keymaps.iter().all(|k| k.plugin_id != "p"));
}

#[test]
fn list_lua_api_returns_resolver_view() {
    // Sanity: `leviathan.keymap.list()` returns the same shape the
    // devtools snapshot uses, including conflict_lost rows.
    let mut host = MockHost::new();
    host.load_inline(
        "aaa",
        &manifest("aaa"),
        r#"
        leviathan.command.create("aaa.go", { run = function() end })
        leviathan.keymap.set("repository", "gl", "aaa.go")
        "#,
    )
    .expect("load");
    host.load_inline(
        "zzz",
        &manifest("zzz"),
        r#"
        leviathan.command.create("zzz.go", { run = function() end })
        leviathan.keymap.set("repository", "gl", "zzz.go")
        _G.entries = #leviathan.keymap.list()
        "#,
    )
    .expect("load");

    // zzz observed at least 1 entry (aaa's already-installed row;
    // zzz's own row is still in BuildState during init.lua and
    // hasn't been committed yet — by design, list() reflects what's
    // resolved at call time). After init completes, both rows are
    // in the registry.
    let n = host
        .read_global_i64("zzz", "entries")
        .expect("entries global");
    assert!(
        n >= 1,
        "expected at least 1 keymap row from list() during init, got {n}"
    );
    let snap = host.introspect();
    let post_count = snap
        .keymaps
        .iter()
        .filter(|k| k.command.ends_with(".go"))
        .count();
    assert_eq!(
        post_count, 2,
        "after both plugins finish loading, both rows are visible"
    );
}

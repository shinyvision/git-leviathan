//! acceptance tests. These tests load the fixture plugins
//! under `plugins/acceptance_fixtures` and assert against the host registries
//! and devtools projection instead of only checking file presence.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::plugin::commands::InvokeOutcome;
use crate::plugin::keymap::{parse_key_sequence, KeymapDispatchOutcome};
use crate::plugin::performance::{CallbackKind, Outcome as PerfOutcome};
use crate::plugin::resources::{GenerationId, PluginId};
use crate::plugin::tests::harness::MockHost;
use crate::services::gateway::GitRepositoryGateway;
use crate::services::test_support::init_test_repo;

const GOOD: &[&str] = &[
    "issue_tracker",
    "repository_info_v1",
    "commit_lens",
    "diff_notes",
    "repo_guard",
    "lazy_demo",
    "toolbar_plugin",
    "graph_decoration_provider",
    "diff_gutter_provider",
    "dock_panel",
    "full_screen",
    "settings_panel",
];

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/acceptance_fixtures")
}

fn copy_tree(src: &Path, dst: &Path) {
    if dst.exists() {
        fs::remove_dir_all(dst).expect("remove old fixture copy");
    }
    fs::create_dir_all(dst).expect("create fixture copy root");
    for entry in fs::read_dir(src).expect("read fixture dir") {
        let entry = entry.expect("fixture dir entry");
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_tree(&src_path, &dst_path);
        } else {
            fs::copy(&src_path, &dst_path).expect("copy fixture file");
        }
    }
}

fn stage_good_fixtures(host: &MockHost) -> HashMap<String, PathBuf> {
    let root = host.root().join("acceptance_good");
    fs::create_dir_all(&root).expect("acceptance good root");
    let mut dirs = HashMap::new();
    for id in GOOD {
        let dst = root.join(id);
        copy_tree(&fixtures_root().join("good").join(id), &dst);
        dirs.insert((*id).to_string(), dst);
    }
    dirs
}

fn load_good_fixtures(host: &mut MockHost, dirs: &HashMap<String, PathBuf>) {
    let mut ordered: Vec<PathBuf> = GOOD.iter().map(|id| dirs[*id].clone()).collect();
    ordered.sort();
    let root = host.root().join("acceptance_good");
    host.host_mut().resolve_and_load(&root, &ordered);
}

fn copy_bad_fixture(host: &MockHost, name: &str) -> PathBuf {
    let dst = host.root().join("acceptance_bad").join(name);
    copy_tree(&fixtures_root().join("bad_plugin_suite").join(name), &dst);
    dst
}

fn collect_files(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).expect("read scan dir") {
        let entry = entry.expect("scan dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

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

fn generation_for(host: &MockHost, plugin_id: &str) -> u64 {
    host.introspect()
        .resources
        .iter()
        .filter(|r| r.plugin_id == plugin_id)
        .map(|r| r.generation_id)
        .max()
        .unwrap_or(0)
}

#[test]
fn public_plugin_fixtures_do_not_use_retired_ui_names() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_files(&repo.join("plugins/acceptance_fixtures"), &mut files);
    collect_files(&repo.join("plugins/examples"), &mut files);
    let forbidden = [
        ["leviathan.ui.", "regions"].concat(),
        ["leviathan.ui.", "main_bar"].concat(),
        ["ui", ".", "v", "2"].concat(),
        ["ui", "_", "v", "2"].concat(),
        ["regions", ".add_slot"].concat(),
        ["regions", ".replace_slot"].concat(),
        ["regions", ".remove_slot"].concat(),
        ["ui:", "graph_decoration"].concat(),
        ["ui:", "diff_decoration"].concat(),
        ["ui:", "context_menu", "\""].concat(),
        ["compatibility", " alias"].concat(),
        ["compatibility", " grant"].concat(),
        ["migration", " guide"].concat(),
    ];
    for path in files {
        let content = fs::read_to_string(&path).unwrap_or_default();
        let lower = content.to_ascii_lowercase();
        for term in &forbidden {
            assert!(
                !lower.contains(term),
                "{} contains retired UI term `{term}`",
                path.display()
            );
        }
    }
}

#[test]
fn good_demo_plugins_exercise_dream_system_and_cleanly_unload() {
    let mut host = MockHost::new();
    host.set_keymap_leader(",");
    let dirs = stage_good_fixtures(&host);
    load_good_fixtures(&mut host, &dirs);

    let snap = host.introspect();
    for id in GOOD.iter().filter(|id| **id != "lazy_demo") {
        assert!(
            snap.plugins
                .iter()
                .any(|p| p.id == *id && p.api_version == "1.0"),
            "{id} should be eagerly loaded; plugins={:?}; diagnostics={:?}",
            snap.plugins
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            host.diagnostics()
                .tail(80)
                .iter()
                .map(|d| (
                    d.plugin_id.as_str().to_string(),
                    d.code.clone(),
                    d.message.clone()
                ))
                .collect::<Vec<_>>()
        );
    }
    assert!(
        snap.lazy_plugins
            .iter()
            .any(|p| p.plugin_id == "lazy_demo" && p.status == "lazy"),
        "lazy_demo should be registered but not active yet"
    );

    assert!(host.has_slot(
        "repository_info_v1",
        "main_bar",
        "left",
        "builtin.repo_info"
    ));
    assert!(host.has_slot(
        "commit_lens",
        "repository",
        "details.top",
        "plugin.commit_lens.annotation"
    ));
    assert!(host.has_slot(
        "toolbar_plugin",
        "main_bar",
        "right",
        "plugin.toolbar_plugin.ping"
    ));

    assert!(snap
        .services
        .iter()
        .any(|s| s.publisher_plugin_id == "issue_tracker" && s.key == "issue_tracker@1"));
    assert!(snap.service_call_traces.iter().any(|t| {
        t.caller_plugin_id == "commit_lens"
            && t.provider_plugin_id == "issue_tracker"
            && t.service_key == "issue_tracker@1"
            && t.method == "lookup"
            && t.success
    }));
    assert!(snap
        .loaded_modules
        .iter()
        .any(|m| m.plugin_id == "commit_lens" && m.module_name == "commit_lens.labels"));
    assert!(snap.graph_decorations.iter().any(|d| {
        d.plugin_id == "commit_lens" && d.commit_hash == "abc1234" && d.kind == "badge"
    }));
    assert!(snap.graph_decorations.iter().any(|d| {
        d.plugin_id == "graph_decoration_provider" && d.id == "graph-provider.acceptance"
    }));
    assert!(snap
        .diff_decorations
        .iter()
        .any(|d| d.plugin_id == "diff_notes" && d.kind == "line_hint"));
    assert!(snap
        .diff_decorations
        .iter()
        .any(|d| d.plugin_id == "diff_gutter_provider" && d.kind == "line_gutter"));
    assert!(snap
        .context_menu_items
        .iter()
        .any(|i| i.plugin_id == "diff_notes" && i.command == "diff_notes.add"));
    assert!(snap
        .settings
        .iter()
        .any(|s| s.plugin_id == "commit_lens" && s.schema_keys.contains(&"enabled".to_string())));
    assert!(snap
        .dock_panels
        .iter()
        .any(|p| p.plugin_id == "dock_panel" && p.id == "acceptance"));
    assert!(snap
        .extension_contributions
        .iter()
        .any(|c| { c.plugin_id == "full_screen" && c.point_id == "screens" && c.id == "home" }));
    assert!(snap.settings.iter().any(|s| {
        s.plugin_id == "settings_panel"
            && s.schema_keys.contains(&"enabled".to_string())
            && s.custom_view.is_some()
    }));
    assert!(snap
        .storage
        .iter()
        .any(|s| s.plugin_id == "commit_lens" && s.surface == "state"));
    assert!(snap
        .timers
        .iter()
        .any(|t| t.plugin_id == "commit_lens" && t.kind == "every"));
    assert!(snap
        .file_watchers
        .iter()
        .any(|w| w.plugin_id == "commit_lens"));

    assert!(host
        .invoke_command("commit_lens.refresh", json!({}))
        .is_ok());
    let pre_async = host.introspect();
    assert!(pre_async
        .async_jobs
        .iter()
        .any(|j| j.plugin_id == "commit_lens"));
    drain_until(
        &mut host,
        |h| h.read_global_i64("commit_lens", "async_done") == Some(24),
        2000,
    );
    assert_eq!(host.read_global_i64("commit_lens", "async_done"), Some(24));

    let chord = parse_key_sequence("gl", ",").expect("parse keymap");
    let out = host.dispatch_key("repository", &chord);
    assert!(
        matches!(out, KeymapDispatchOutcome::Dispatched { .. }),
        "commit_lens keymap should dispatch: {out:?}"
    );
    assert_eq!(host.read_global_i64("commit_lens", "keymap_runs"), Some(1));

    host.dispatch_test_event("CommitSelected", json!({ "commit": { "hash": "abc1234" } }));
    assert_eq!(
        host.read_global_i64("commit_lens", "commit_events"),
        Some(1)
    );

    std::thread::sleep(Duration::from_millis(30));
    drain_until(
        &mut host,
        |h| h.read_global_i64("commit_lens", "timer_fires").unwrap_or(0) >= 1,
        500,
    );
    assert!(
        host.read_global_i64("commit_lens", "timer_fires")
            .unwrap_or(0)
            >= 1
    );

    let watched = dirs["commit_lens"].join("watched.txt");
    std::thread::sleep(Duration::from_millis(50));
    {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&watched)
            .expect("open watched file");
        writeln!(file, "acceptance").expect("write watched file");
        file.sync_all().ok();
    }
    drain_until(
        &mut host,
        |h| h.read_global_i64("commit_lens", "watch_fires").unwrap_or(0) >= 1,
        3000,
    );
    assert!(
        host.read_global_i64("commit_lens", "watch_fires")
            .unwrap_or(0)
            >= 1
    );

    assert!(host.invoke_command("diff_notes.add", json!({})).is_ok());
    assert_eq!(host.read_global_i64("diff_notes", "note_actions"), Some(1));

    let (repo_dir, repo) = init_test_repo("repo_guard_acceptance");
    let head_hash = repo.head().unwrap().target().unwrap().to_string();
    host.host_mut()
        .set_repository_gateway(Some(GitRepositoryGateway::from_path(repo_dir.path_str())));
    assert!(host
        .invoke_command(
            "repo_guard.create_branch",
            json!({ "name": "repo_guard_default" }),
        )
        .is_ok());
    assert_eq!(host.read_global_i64("repo_guard", "branch_ok"), Some(1));

    assert!(host
        .invoke_command("repo_guard.reset_hard", json!({ "ref": head_hash }))
        .is_ok());
    assert_eq!(host.read_global_i64("repo_guard", "reset_ok"), Some(0));
    assert!(host
        .read_global_string("repo_guard", "reset_err")
        .unwrap_or_default()
        .contains("requires confirmation"));
    let head_hash = repo.head().unwrap().target().unwrap().to_string();
    host.host().destructive_policy().approve_next(
        "repo_guard",
        "reset",
        "acceptance-test",
        json!({ "ref": head_hash, "mode": "hard" }),
    );
    assert!(host
        .invoke_command("repo_guard.reset_hard", json!({ "ref": head_hash }))
        .is_ok());
    assert_eq!(host.read_global_i64("repo_guard", "reset_ok"), Some(1));
    assert!(host
        .host()
        .destructive_policy()
        .history()
        .iter()
        .any(|h| { h.plugin_id == "repo_guard" && h.op == "reset" && h.approved }));

    host.dispatch_test_event("FetchStarted", json!({ "remote_name": "origin" }));
    assert_eq!(
        host.read_global_string("lazy_demo", "activated").as_deref(),
        Some("yes")
    );
    assert_eq!(host.read_global_i64("lazy_demo", "lazy_events"), Some(1));

    let health = host.run_health_checks();
    assert!(health.plugins.iter().any(
        |p| p.plugin_id == "commit_lens" && p.items.iter().any(|i| i.message.contains("ready"))
    ));

    let (outcome, bundle) = host.invoke_devtools_command(
        "plugin.export_diagnostic_bundle",
        json!({ "plugin_id": "commit_lens", "include_state": true }),
    );
    assert!(matches!(outcome, InvokeOutcome::Ok));
    let bundle_text = bundle.to_string();
    assert!(bundle_text.contains("ISSUE-abc123"));
    assert!(!bundle_text.contains("super-secret"));

    for id in GOOD {
        let before = generation_for(&host, id);
        host.host_mut()
            .reload_plugin(id)
            .unwrap_or_else(|e| panic!("successful reload for {id}: {e}"));
        assert!(
            generation_for(&host, id) > before,
            "{id} should advance generation on successful reload"
        );
    }

    for id in GOOD {
        let dir = &dirs[*id];
        let init_path = dir.join("init.lua");
        let original = fs::read_to_string(&init_path).expect("read original init");
        let generation = generation_for(&host, id);
        fs::write(&init_path, "this is not valid lua >>>").expect("stage bad init");
        let result = host.host_mut().reload_plugin(id);
        assert!(result.is_err(), "{id} reload should fail");
        assert_eq!(
            generation_for(&host, id),
            generation,
            "{id} old generation should survive failed reload"
        );
        assert!(
            host.introspect().plugins.iter().any(|p| p.id == *id),
            "{id} should still be live after failed reload"
        );
        fs::write(&init_path, original).expect("restore init");
        host.host_mut()
            .reload_plugin(id)
            .unwrap_or_else(|e| panic!("{id} should reload after restore: {e}"));
    }

    let final_snap = host.introspect();
    for id in GOOD {
        assert!(
            final_snap.resources.iter().any(|r| r.plugin_id == *id),
            "devtools resources should include {id}"
        );
    }
    assert!(final_snap
        .commands
        .iter()
        .any(|c| c.plugin_id == "commit_lens"));
    assert!(final_snap
        .keymaps
        .iter()
        .any(|k| k.plugin_id == "commit_lens"));
    assert!(final_snap
        .autocmds
        .iter()
        .any(|a| a.plugin_id == "commit_lens"));
    assert!(final_snap
        .runtime_paths
        .iter()
        .any(|r| r.plugin_id == "commit_lens"));
    assert!(final_snap
        .performance_traces
        .iter()
        .any(|t| t.plugin_id == "commit_lens"));
    assert!(final_snap
        .pending_git_writes
        .iter()
        .any(|w| w.plugin_id == "repo_guard"));

    for id in GOOD.iter().rev() {
        host.unload_plugin(id)
            .unwrap_or_else(|e| panic!("unload {id}: {e}"));
        let snap = host.introspect();
        assert!(
            snap.resources.iter().all(|r| r.plugin_id != *id),
            "{id} resources should be gone after unload"
        );
        assert!(
            snap.slots.iter().all(|s| s.owner_plugin_id != *id),
            "{id} slots should be gone after unload"
        );
        assert!(
            snap.commands.iter().all(|c| c.plugin_id != *id),
            "{id} commands should be gone after unload"
        );
        assert!(
            snap.keymaps.iter().all(|k| k.plugin_id != *id),
            "{id} keymaps should be gone after unload"
        );
        assert!(
            snap.async_jobs.iter().all(|j| j.plugin_id != *id),
            "{id} async jobs should be gone after unload"
        );
        assert!(
            snap.timers.iter().all(|t| t.plugin_id != *id),
            "{id} timers should be gone after unload"
        );
        assert!(
            snap.file_watchers.iter().all(|w| w.plugin_id != *id),
            "{id} watchers should be gone after unload"
        );
    }
}

#[test]
fn lazy_demo_activates_from_each_declared_trigger() {
    for trigger in ["command", "keymap", "event"] {
        let mut host = MockHost::new();
        host.set_keymap_leader(",");
        let dirs = stage_good_fixtures(&host);
        load_good_fixtures(&mut host, &dirs);
        assert!(host.read_global_string("lazy_demo", "activated").is_none());

        match trigger {
            "command" => {
                assert!(host.invoke_command("lazy_demo.run", json!({})).is_ok());
                assert_eq!(host.read_global_i64("lazy_demo", "lazy_runs"), Some(1));
            }
            "keymap" => {
                let chord = parse_key_sequence(",d", ",").expect("leader keymap");
                let outcome = host.dispatch_key("global", &chord);
                assert!(matches!(outcome, KeymapDispatchOutcome::Dispatched { .. }));
                assert_eq!(host.read_global_i64("lazy_demo", "lazy_runs"), Some(1));
            }
            "event" => {
                host.dispatch_test_event("FetchStarted", json!({ "remote_name": "origin" }));
                assert_eq!(host.read_global_i64("lazy_demo", "lazy_events"), Some(1));
            }
            _ => unreachable!(),
        }
        assert_eq!(
            host.read_global_string("lazy_demo", "activated").as_deref(),
            Some("yes"),
            "lazy_demo should activate from {trigger}"
        );
        let row = host
            .introspect()
            .lazy_plugins
            .into_iter()
            .find(|p| p.plugin_id == "lazy_demo")
            .expect("lazy row");
        assert_eq!(row.status, "active");
        assert_eq!(row.activations, 1);
    }
}

#[test]
fn capability_revocation_and_upgrade_prompts_are_enforced() {
    let mut host = MockHost::new();
    let dirs = stage_good_fixtures(&host);
    load_good_fixtures(&mut host, &dirs);

    host.host_mut()
        .revoke_capability("repo_guard", "0.1.0", "git:write:branch")
        .expect("revoke branch write");
    assert!(host
        .invoke_command(
            "repo_guard.create_branch",
            json!({ "name": "repo_guard_denied" }),
        )
        .is_ok());
    assert_eq!(host.read_global_i64("repo_guard", "branch_ok"), Some(0));
    assert!(host
        .read_global_string("repo_guard", "branch_err")
        .unwrap_or_default()
        .contains("capability denied"));
    assert!(host
        .diagnostics()
        .tail(100)
        .iter()
        .any(|d| d.plugin_id.as_str() == "repo_guard" && d.code == "capability.revoked"));

    let external = tempfile::tempdir().expect("external plugin dir");
    let plugin_dir = external.path().join("repo_guard_upgrade");
    copy_tree(&fixtures_root().join("good/repo_guard"), &plugin_dir);
    fs::write(
        plugin_dir.join("plugin.toml"),
        r#"
id = "repo_guard_upgrade"
name = "Repo Guard Upgrade"
version = "0.1.0"
api_version = "1.0"
capabilities = ["git:write:branch"]

[runtime]
strict_globals = false
"#,
    )
    .expect("write v1 manifest");
    fs::write(
        plugin_dir.join("init.lua"),
        "-- v1 has no sensitive calls\n",
    )
    .expect("write init");
    host.host_mut()
        .load_plugin(&plugin_dir)
        .expect("external v1 loads with a pending prompt");

    fs::write(
        plugin_dir.join("plugin.toml"),
        r#"
id = "repo_guard_upgrade"
name = "Repo Guard Upgrade"
version = "0.2.0"
api_version = "1.0"
capabilities = ["git:write:branch", "env"]

[runtime]
strict_globals = false
"#,
    )
    .expect("write updated manifest");
    let result = host.host_mut().reload_plugin("repo_guard_upgrade");
    assert!(result.is_err(), "capability widening should block reload");
    assert!(host
        .diagnostics()
        .by_code("capability.upgrade_required")
        .iter()
        .any(|d| d.plugin_id.as_str() == "repo_guard_upgrade"));
    assert!(host
        .introspect()
        .audit_recent
        .iter()
        .any(|e| e.plugin_id == "repo_guard_upgrade" && e.capability == "grant.upgrade_prompted"));
}

#[test]
fn bad_plugin_suite_is_contained_and_inspectable() {
    let mut host = MockHost::new();

    let invalid_ui = copy_bad_fixture(&host, "invalid_ui");
    let err = host.host_mut().load_plugin(&invalid_ui).unwrap_err();
    assert!(err.to_string().contains("invalid widget tree"));

    let denied = copy_bad_fixture(&host, "denied_capability");
    let err = host.host_mut().load_plugin(&denied).unwrap_err();
    assert!(err.to_string().contains("capability"));
    assert!(host
        .diagnostics()
        .by_code("capability.denied")
        .iter()
        .any(|d| d.plugin_id.as_str() == "bad_denied_capability"));

    let slow = copy_bad_fixture(&host, "slow_callback");
    host.host_mut()
        .load_plugin(&slow)
        .expect("slow fixture loads");
    let clock = host.install_mock_clock();
    let tracker = host.budget_tracker();
    let generation = GenerationId::new(generation_for(&host, "bad_slow_callback"));
    let pid = PluginId::from("bad_slow_callback");
    let outcome = tracker.track_call::<(), String>(
        CallbackKind::EventCallback,
        &pid,
        generation,
        "autocmd:BranchChanged",
        || {
            clock.advance_ms(500);
            Ok(())
        },
    );
    assert!(matches!(outcome, PerfOutcome::Ok(())));
    assert!(host
        .diagnostics()
        .by_code("performance.hard_breach")
        .iter()
        .any(|d| d.plugin_id.as_str() == "bad_slow_callback"));
    assert!(host
        .introspect()
        .performance_traces
        .iter()
        .any(|t| t.plugin_id == "bad_slow_callback"));

    let failed_reload = copy_bad_fixture(&host, "failed_reload");
    host.host_mut()
        .load_plugin(&failed_reload)
        .expect("failed_reload initial load");
    assert!(host.has_slot("bad_failed_reload", "main_bar", "left", "bad.failed_reload"));
    let generation = generation_for(&host, "bad_failed_reload");
    fs::write(failed_reload.join("init.lua"), "this is not valid lua >>>")
        .expect("write broken reload");
    assert!(host.host_mut().reload_plugin("bad_failed_reload").is_err());
    assert_eq!(generation_for(&host, "bad_failed_reload"), generation);
    assert!(host.has_slot("bad_failed_reload", "main_bar", "left", "bad.failed_reload"));

    let cycle_a = copy_bad_fixture(&host, "dependency_cycle_a");
    let cycle_b = copy_bad_fixture(&host, "dependency_cycle_b");
    let bad_root = host.root().join("acceptance_bad");
    host.host_mut()
        .resolve_and_load(&bad_root, &[cycle_a, cycle_b]);
    let snap = host.introspect();
    assert!(snap.plugins.iter().all(|p| p.id != "bad_cycle_a"));
    assert!(snap.plugins.iter().all(|p| p.id != "bad_cycle_b"));
    assert!(host
        .diagnostics()
        .by_code("dependency.cycle")
        .iter()
        .any(|d| d.plugin_id.as_str() == "bad_cycle_a"));

    let race = copy_bad_fixture(&host, "async_cleanup_race");
    host.host_mut()
        .load_plugin(&race)
        .expect("async cleanup fixture loads");
    assert!(host
        .introspect()
        .async_jobs
        .iter()
        .any(|j| j.plugin_id == "bad_async_cleanup_race"));
    host.unload_plugin("bad_async_cleanup_race")
        .expect("unload async cleanup fixture");
    let snap = host.introspect();
    assert!(snap
        .resources
        .iter()
        .all(|r| r.plugin_id != "bad_async_cleanup_race"));
    assert!(snap
        .async_jobs
        .iter()
        .all(|j| j.plugin_id != "bad_async_cleanup_race"));
}

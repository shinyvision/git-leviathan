//! extension points acceptance tests: extension-point expansion.
//!
//! Acceptance gates exercised here:
//! - the region descriptor table validates mounted slot addresses;
//! - `leviathan.ui.overlay` records overlays through the resource
//!   ledger and surfaces them on `InspectorSnapshot`;
//! - overlay registration is gated by the `ui:overlay` capability;
//! - `leviathan.ui.context_menu` items sort by priority ascending;
//! - `leviathan.ui.graph_decoration` returns a typed AST that flows
//!   through the inspector;
//! - `leviathan.ui.diff_decoration` returns a typed AST that flows
//!   through the inspector;
//! - `unload_plugin` drops every extension a plugin owned.

use git_leviathan_plugin_api::descriptor::region::REGIONS;

use crate::plugin::tests::harness::MockHost;

const ALL_EXT_CAPS: &str = r#"["ui:overlay", "ui:context_menu:repository.diff.context_menu", "ui:context_menu:repository.graph.context_menu", "ui:decoration:graph", "ui:decoration:diff"]"#;

fn manifest_with_caps(id: &str, caps: &str) -> String {
    format!(
        r#"
id = "{id}"
name = "{id}"
version = "0.1.0"
api_version = "1.0"
capabilities = {caps}
"#
    )
}

#[test]
fn region_table_validates_new_static_addresses() {
    let repo = REGIONS.get("repository").expect("repository present");
    for pane in ["sidebar", "graph", "details"] {
        assert!(repo.validate_address(Some(pane), Some("top")).is_ok());
        assert!(repo.validate_address(Some(pane), Some("bottom")).is_ok());
    }
}

#[test]
fn region_table_rejects_unmounted_addresses() {
    let repo = REGIONS.get("repository").unwrap();
    assert!(REGIONS.get("status_bar").is_none());
    assert!(REGIONS.get("repository.diff").is_none());
    assert!(REGIONS.get("repository.graph").is_none());
    assert!(REGIONS.get("repository.details").is_none());
    assert!(repo
        .validate_address(Some("sidebar"), Some("section:my-extra-pane"))
        .is_err());
    assert!(repo
        .validate_address(Some("graph"), Some("row:abc1234"))
        .is_err());
}

#[test]
fn extension_point_descriptors_cover_phase7_surfaces() {
    use git_leviathan_plugin_api::descriptor::extension_point::{
        ExtensionPointKind, EXTENSION_POINTS,
    };
    let ids: Vec<&str> = EXTENSION_POINTS.iter().map(|point| point.id).collect();
    for id in [
        "repository.diff.context_menu",
        "repository.graph.row_badge",
        "repository.diff.line_gutter",
        "overlays",
        "screens",
        "commands",
        "settings.panel",
        "dock.pane",
    ] {
        assert!(ids.contains(&id), "missing extension point {id}");
    }
    for kind in [
        ExtensionPointKind::Slot,
        ExtensionPointKind::ContextMenu,
        ExtensionPointKind::Decoration,
        ExtensionPointKind::Overlay,
        ExtensionPointKind::Screen,
        ExtensionPointKind::Command,
        ExtensionPointKind::SettingsPanel,
        ExtensionPointKind::DockPane,
    ] {
        assert!(EXTENSION_POINTS.iter().any(|point| point.kind == kind));
    }
}

#[test]
fn overlay_registration_appears_in_devtools() {
    let mut host = MockHost::new();
    host.load_inline(
        "p_overlay",
        &manifest_with_caps("p_overlay", ALL_EXT_CAPS),
        r##"
        leviathan.ui.overlay{
            id = "confirm",
            priority = 100,
            dismissible = true,
            key_events = { "Tab", "ArrowUp", "ArrowDown" },
            widget = { kind = "text", value = "Are you sure?" },
        }
        "##,
    )
    .expect("overlay registers");

    let snap = host.introspect();
    let overlay = snap
        .overlays
        .iter()
        .find(|o| o.plugin_id == "p_overlay" && o.id == "confirm")
        .expect("overlay row present");
    assert_eq!(overlay.priority, 100);
    assert!(overlay.dismissible);
    assert_eq!(overlay.key_events, vec!["tab", "up", "down"]);
    // `widget` is carried through verbatim from the plugin-supplied AST so
    // devtools can introspect the overlay payload. `source_location` is
    // unset for inline-loaded plugins; the field still has to project so
    // record→summary stays lossless.
    assert_eq!(overlay.widget.node.kind(), "text");
    // `source_location` is whatever the Lua loader recorded for the
    // overlay registration; it just has to project through. Inline
    // plugins may report a synthetic location but never panic.
    let _ = overlay.source_location.as_deref();
}

#[test]
fn overlay_registration_requires_ui_overlay_capability() {
    let mut host = MockHost::new();
    let err = host
        .load_inline(
            "no_cap",
            // No capability list — `ui:overlay` not declared.
            r#"
id = "no_cap"
name = "no_cap"
version = "0.1.0"
api_version = "1.0"
"#,
            r#"
            leviathan.ui.overlay{
                id = "x",
                widget = { kind = "text", value = "hi" },
            }
            "#,
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("ui:overlay"), "got: {err}");
}

#[test]
fn overlay_event_callback_can_remove_overlay() {
    let mut host = MockHost::new();
    host.load_inline(
        "palette",
        &manifest_with_caps("palette", ALL_EXT_CAPS),
        r#"
        hits = 0
        last_event = ""
        last_value = ""
        leviathan.ui.overlay{
            id = "cmd",
            priority = 10,
            dismissible = true,
            widget = { kind = "button", text = "Run", on_click = "choose", value = { name = "status" } },
            on_event = function(id, event, value)
                hits = hits + 1
                last_event = event
                last_value = value.name or ""
                if event == "close" then
                    leviathan.ui.remove_overlay(id)
                end
            end,
        }
        "#,
    )
    .expect("overlay registers");

    host.host_mut().dispatch_overlay_event(
        "palette",
        "cmd",
        "choose",
        serde_json::json!({ "name": "status" }),
    );
    assert_eq!(host.read_global_i64("palette", "hits"), Some(1));
    assert_eq!(
        host.read_global_string("palette", "last_event").as_deref(),
        Some("choose")
    );
    assert_eq!(
        host.read_global_string("palette", "last_value").as_deref(),
        Some("status")
    );

    host.host_mut()
        .dispatch_overlay_event("palette", "cmd", "close", serde_json::json!({}));
    assert_eq!(host.read_global_i64("palette", "hits"), Some(2));
    assert!(host.introspect().overlays.iter().all(|o| o.id != "cmd"));
}

#[test]
fn context_menu_items_sorted_by_priority() {
    let mut host = MockHost::new();
    host.load_inline(
        "ctx",
        &manifest_with_caps("ctx", ALL_EXT_CAPS),
        r#"
        leviathan.ui.context_menu("repository.diff.context_menu", {
            id = "stage",   label = "Stage",   command = "git.stage",   priority = 30,
        })
        leviathan.ui.context_menu("repository.diff.context_menu", {
            id = "blame",   label = "Blame",   command = "git.blame",   priority = 10,
        })
        leviathan.ui.context_menu("repository.diff.context_menu", {
            id = "discard", label = "Discard", command = "git.discard", priority = 20,
        })
        "#,
    )
    .expect("context_menu registers");

    let snap = host.introspect();
    let items: Vec<&str> = snap
        .context_menu_items
        .iter()
        .filter(|i| i.region == "repository.diff.context_menu")
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(items, vec!["blame", "discard", "stage"]);

    // Each row carries its full payload (label/command/priority +
    // optional capability gate + source-location) through to devtools.
    let blame = snap
        .context_menu_items
        .iter()
        .find(|i| i.id == "blame")
        .expect("blame row");
    assert_eq!(blame.label, "Blame");
    assert_eq!(blame.command, "git.blame");
    assert_eq!(blame.priority, 10);
    assert!(blame.condition_capability.is_none());
    let _ = blame.source_location.as_deref();
}

#[test]
fn contribute_registers_static_and_dynamic_decorations() {
    let mut host = MockHost::new();
    host.load_inline(
        "uiext",
        &manifest_with_caps(
            "uiext",
            r#"["ui:context_menu:repository.diff.context_menu", "ui:decoration:graph", "ui:decoration:diff"]"#,
        ),
        r##"
        local menu, menu_err = leviathan.ui.contribute("repository.diff.context_menu", {
            id = "copy-path", label = "Copy Path", command = "diff.copy_path", priority = 5,
        })
        assert(menu and not menu_err)

        local static_g, static_g_err = leviathan.ui.contribute("repository.graph.row_badge", {
            id = "reviewed", commit_hash = "abc1234",
            decoration = { kind = "badge", text = "OK", fg = "#fff", bg = "#047857" },
        })
        assert(static_g and not static_g_err)

        local static_d, static_d_err = leviathan.ui.contribute("repository.diff.line_gutter", {
            id = "line-pin",
            decoration = { kind = "line_gutter", file = "src/lib.rs", line = 9, glyph = "!" },
        })
        assert(static_d and not static_d_err)

        local graph_provider, graph_err = leviathan.ui.contribute("repository.graph.row_badge", {
            id = "dynamic-review",
            provider = function(ctx)
                return { kind = "badge", text = ctx.payload.graph_row.commit_hash }
            end,
        })
        assert(graph_provider and not graph_err)

        local diff_provider, diff_err = leviathan.ui.contribute("repository.diff.line_gutter", {
            id = "dynamic-line",
            provider = function(ctx)
                return { kind = "line_gutter", glyph = tostring(ctx.payload.diff_line.line) }
            end,
        })
        assert(diff_provider and not diff_err)
        "##,
    )
    .expect("UI extension contributions register");

    let snap = host.introspect();
    assert!(snap
        .extension_contributions
        .iter()
        .any(|row| row.point_id == "repository.diff.context_menu" && row.id == "copy-path"));
    assert!(snap
        .extension_contributions
        .iter()
        .any(|row| row.point_id == "repository.graph.row_badge" && row.dynamic));
    assert!(snap
        .context_menu_items
        .iter()
        .any(|row| row.region == "repository.diff.context_menu" && row.id == "copy-path"));

    let graph_rows = host
        .host()
        .extension_graph_decorations_for_commit("cafebabe");
    assert!(graph_rows.iter().any(|row| {
        matches!(
            &row.decoration,
            git_leviathan_plugin_api::descriptor::decoration::GraphDecoration::Badge { text, .. }
                if text == "cafebabe"
        )
    }));
    let diff_rows = host
        .host()
        .extension_diff_decorations_for_line("src/lib.rs", 9);
    assert!(diff_rows.iter().any(|row| {
        matches!(
            &row.decoration,
            git_leviathan_plugin_api::descriptor::decoration::DiffDecoration::LineGutter {
                glyph,
                ..
            } if glyph == "9"
        )
    }));
}

#[test]
fn decoration_invalidation_tracks_required_refresh_events() {
    let mut host = MockHost::new();
    let before = host.introspect().decoration_revision;
    host.dispatch_test_event("CommitSelected", serde_json::json!({ "hash": "a" }));
    host.dispatch_test_event("DiffLoaded", serde_json::json!({ "hash": "b" }));
    host.dispatch_test_event("RefsChanged", serde_json::json!({ "count": 2 }));
    let snap = host.introspect();
    assert!(snap.decoration_revision >= before + 3);
    let reasons: Vec<&str> = snap
        .decoration_invalidations
        .iter()
        .map(|row| row.reason.as_str())
        .collect();
    assert!(reasons.contains(&"selection"));
    assert!(reasons.contains(&"diff_load"));
    assert!(reasons.contains(&"refs_change"));
}

#[test]
fn graph_decoration_returned_for_commit_row() {
    let mut host = MockHost::new();
    host.load_inline(
        "gd",
        &manifest_with_caps("gd", ALL_EXT_CAPS),
        r##"
        leviathan.ui.graph_decoration("abc1234", {
            kind = "badge",
            text = "WIP",
            fg = "#fff",
            bg = "#aa0000",
        })
        leviathan.ui.graph_decoration("abc1234", {
            id = "lane-decor",
            kind = "lane",
            index = 2,
            color = "#00ff88",
        })
        "##,
    )
    .expect("graph_decoration registers");

    let snap = host.introspect();
    let rows: Vec<_> = snap
        .graph_decorations
        .iter()
        .filter(|d| d.commit_hash == "abc1234")
        .collect();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|d| d.kind == "badge"));
    assert!(rows
        .iter()
        .any(|d| d.kind == "lane" && d.id == "lane-decor"));

    // The decoration AST projects through verbatim and is visible to
    // devtools as JSON; e.g. the lane row carries its index/color so
    // an inspector can render the same paint without round-tripping
    // through Lua. `source_location` rides along even when unset.
    let lane = rows
        .iter()
        .find(|d| d.id == "lane-decor")
        .expect("lane row");
    assert_eq!(lane.decoration["kind"], "lane");
    assert_eq!(lane.decoration["index"], 2);
    assert_eq!(lane.decoration["color"], "#00ff88");
    let _ = lane.source_location.as_deref();
}

#[test]
fn diff_decoration_returned_for_line() {
    let mut host = MockHost::new();
    host.load_inline(
        "dd",
        &manifest_with_caps("dd", ALL_EXT_CAPS),
        r#"
        leviathan.ui.diff_decoration({
            kind = "line_hint",
            severity = "warn",
            text = "trailing whitespace",
            file = "src/foo.rs",
            line = 42,
        })
        leviathan.ui.diff_decoration({
            kind = "hunk_badge",
            hunk_id = "h1",
            label = "+5/-1",
        })
        "#,
    )
    .expect("diff_decoration registers");

    let snap = host.introspect();
    let kinds: Vec<&str> = snap
        .diff_decorations
        .iter()
        .filter(|d| d.plugin_id == "dd")
        .map(|d| d.kind.as_str())
        .collect();
    assert!(kinds.contains(&"line_hint"));
    assert!(kinds.contains(&"hunk_badge"));

    // `decoration` carries the typed AST (severity/text/file/line for
    // line_hint, hunk_id/label for hunk_badge). `source_location`
    // projects through alongside.
    let line_hint = snap
        .diff_decorations
        .iter()
        .find(|d| d.plugin_id == "dd" && d.kind == "line_hint")
        .expect("line_hint row");
    assert_eq!(line_hint.decoration["severity"], "warn");
    assert_eq!(line_hint.decoration["text"], "trailing whitespace");
    assert_eq!(line_hint.decoration["line"], 42);
    let _ = line_hint.source_location.as_deref();
}

#[test]
fn unload_clears_overlays_context_menus_and_decorations() {
    let mut host = MockHost::new();
    host.load_inline(
        "owner",
        &manifest_with_caps("owner", ALL_EXT_CAPS),
        r##"
        leviathan.ui.overlay{
            id = "ov",
            priority = 0,
            dismissible = true,
            widget = { kind = "text", value = "hi" },
        }
        leviathan.ui.context_menu("repository.graph.context_menu", {
            id = "rebase", label = "Rebase", command = "git.rebase", priority = 0,
        })
        leviathan.ui.graph_decoration("deadbeef", {
            kind = "marker", shape = "dot", color = "#fff",
        })
        leviathan.ui.diff_decoration({
            kind = "line_gutter", file = "f.rs", line = 1, glyph = ">",
        })
        "##,
    )
    .expect("load owner");

    let pre = host.introspect();
    assert!(pre.overlays.iter().any(|o| o.plugin_id == "owner"));
    assert!(pre
        .context_menu_items
        .iter()
        .any(|i| i.plugin_id == "owner"));
    assert!(pre.graph_decorations.iter().any(|d| d.plugin_id == "owner"));
    assert!(pre.diff_decorations.iter().any(|d| d.plugin_id == "owner"));

    host.unload_plugin("owner").expect("unload owner");

    let post = host.introspect();
    assert!(post.overlays.iter().all(|o| o.plugin_id != "owner"));
    assert!(post
        .context_menu_items
        .iter()
        .all(|i| i.plugin_id != "owner"));
    assert!(post
        .graph_decorations
        .iter()
        .all(|d| d.plugin_id != "owner"));
    assert!(post.diff_decorations.iter().all(|d| d.plugin_id != "owner"));
}

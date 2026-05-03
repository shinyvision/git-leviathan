//! extension points acceptance tests: extension-point expansion.
//!
//! Acceptance gates exercised here:
//! - the region descriptor table validates the new static
//!   addresses (`status_bar.left`, `repository.diff.toolbar`, …)
//!   plus dynamic prefixes (`section:`, `row:`, `line:`, `hunk:`);
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

const ALL_EXT_CAPS: &str =
    r#"["ui:overlay", "ui:context_menu", "ui:graph_decoration", "ui:diff_decoration"]"#;

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
    // status_bar fixed sections.
    let status_bar = REGIONS.get("status_bar").expect("status_bar present");
    assert!(status_bar.validate_address(None, Some("left")).is_ok());
    assert!(status_bar.validate_address(None, Some("center")).is_ok());
    assert!(status_bar.validate_address(None, Some("right")).is_ok());

    // diff toolbar / context_menu fixed sections.
    let diff = REGIONS
        .get("repository.diff")
        .expect("repository.diff present");
    assert!(diff.validate_address(Some("diff"), Some("toolbar")).is_ok());
    assert!(diff
        .validate_address(Some("diff"), Some("context_menu"))
        .is_ok());

    // graph fixed sections.
    let graph = REGIONS
        .get("repository.graph")
        .expect("repository.graph present");
    assert!(graph
        .validate_address(Some("graph"), Some("decorations"))
        .is_ok());
    assert!(graph
        .validate_address(Some("graph"), Some("context_menu"))
        .is_ok());

    // details panel.
    let details = REGIONS
        .get("repository.details")
        .expect("repository.details present");
    assert!(details
        .validate_address(Some("details"), Some("commit_header"))
        .is_ok());
    assert!(details
        .validate_address(Some("details"), Some("files"))
        .is_ok());
}

#[test]
fn region_table_validates_dynamic_line_addresses() {
    let diff = REGIONS.get("repository.diff").unwrap();
    // Dynamic line / hunk ids accepted with non-empty tail.
    assert!(diff
        .validate_address(Some("diff"), Some("line:src/foo.rs:42"))
        .is_ok());
    assert!(diff.validate_address(Some("diff"), Some("hunk:7")).is_ok());

    let graph = REGIONS.get("repository.graph").unwrap();
    assert!(graph
        .validate_address(Some("graph"), Some("row:abc1234"))
        .is_ok());

    // Sidebar dynamic section.
    let repo = REGIONS.get("repository").unwrap();
    assert!(repo
        .validate_address(Some("sidebar"), Some("section:my-extra-pane"))
        .is_ok());
}

#[test]
fn region_table_rejects_unknown_dynamic_prefix() {
    let diff = REGIONS.get("repository.diff").unwrap();
    let err = diff
        .validate_address(Some("diff"), Some("blockchain:1"))
        .unwrap_err();
    assert!(err.contains("no section"), "got: {err}");

    // Empty tail after a known prefix is rejected with a helpful
    // message rather than silently accepted.
    let err = diff
        .validate_address(Some("diff"), Some("line:"))
        .unwrap_err();
    assert!(err.contains("empty tail"), "got: {err}");
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

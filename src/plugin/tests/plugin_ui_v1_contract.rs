use serde_json::json;

use crate::core::TabId;
use crate::plugin::audit::AuditOutcome;
use crate::plugin::slots::Container;
use crate::plugin::tab_snapshot::{TabSnapshotEntry, TabsSnapshot};
use crate::plugin::tests::harness::MockHost;
use crate::widgets::chrome::main_bar::{builtins as main_bar_builtins, MainBarRegistry, SlotCtx};
use crate::widgets::chrome::repo_region as rr;
use crate::widgets::chrome::tab_bar_slots::{
    builtins as tab_bar_builtins, TabBarCtx, TabBarRegistry,
};

const SLOT_CAPS: &str =
    r#"["ui:region:main_bar", "ui:region:tab_bar", "ui:region:repository", "ui:screen"]"#;
const EXT_CAPS: &str = r#"["ui:region:main_bar", "ui:overlay", "ui:context_menu:repository.diff.context_menu", "ui:decoration:graph", "ui:decoration:diff"]"#;

fn manifest(id: &str) -> String {
    format!(
        r#"
id = "{id}"
name = "{id}"
version = "0.1.0"
api_version = "1.0"
capabilities = {SLOT_CAPS}
"#
    )
}

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

fn main_bar_registry(host: &MockHost) -> MainBarRegistry {
    let mut registry = MainBarRegistry::new();
    main_bar_builtins::register_all(&mut registry);
    host.host().apply_main_bar_slots(&mut registry);
    registry
}

fn tab_bar_registry(host: &MockHost) -> TabBarRegistry {
    let mut registry = TabBarRegistry::new();
    tab_bar_builtins::register_all(&mut registry);
    host.host().apply_tab_bar_slots(&mut registry);
    registry
}

fn repo_registry(host: &MockHost) -> rr::RepoRegionRegistry {
    let mut registry = rr::RepoRegionRegistry::new();
    host.host().apply_repo_region_slots(&mut registry);
    registry
}

fn tab_snapshot() -> TabsSnapshot {
    TabsSnapshot {
        tabs: vec![TabSnapshotEntry {
            id: TabId(1),
            path: "/tmp/repo".into(),
            name: "repo".into(),
        }],
        active_id: Some(TabId(1)),
        active_path: Some("/tmp/repo".into()),
    }
}

#[test]
fn v1_slots_replay_and_render_for_mounted_regions() {
    let mut host = MockHost::new();
    host.load_inline(
        "slots",
        &manifest("slots"),
        r#"
        leviathan.ui.slot.add{
            region = "main_bar", section = "right", id = "plugin.slots.main",
            priority = 90, widget = { kind = "text", value = "main" },
        }
        leviathan.ui.slot.add{
            region = "tab_bar", section = "right", id = "plugin.slots.tab",
            priority = 90, widget = { kind = "text", value = "tab" },
        }
        leviathan.ui.slot.add{
            region = "repository", pane = "sidebar", section = "top",
            id = "plugin.slots.repo", priority = 90,
            widget = { kind = "text", value = "repo" },
        }
        "#,
    )
    .expect("load slots");

    let main = main_bar_registry(&host);
    let main_slot = main
        .iter_container(Container::Section("right".into()))
        .find(|slot| slot.id == "plugin.slots.main")
        .expect("main slot");
    let main_ctx = SlotCtx::new("repo", "main", None, None, None, None);
    let _ = (main_slot.builder)(&main_ctx);

    let tab = tab_bar_registry(&host);
    let tabs = tab_snapshot();
    let tab_ctx = TabBarCtx::with_tabs(&tabs);
    let tab_slot = tab
        .iter_container(Container::Section("right".into()))
        .find(|slot| slot.id == "plugin.slots.tab")
        .expect("tab slot");
    let _ = (tab_slot.builder)(&tab_ctx);

    let repo = repo_registry(&host);
    assert!(rr::render_top(&repo, rr::Pane::Sidebar).is_some());
    assert!(rr::render_bottom(&repo, rr::Pane::Sidebar).is_none());
}

#[test]
fn v1_dynamic_widgets_refresh_on_load_event_and_tab_sync() {
    let mut host = MockHost::new();
    host.load_inline(
        "dyn",
        &manifest("dyn"),
        r#"
        _G.widget_refreshes = 0
        _G.theme_hits = 0

        leviathan.autocmd.create({ "ThemeChanged" }, {
            callback = function()
                _G.theme_hits = _G.theme_hits + 1
            end,
        })

        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.dyn.counter",
            priority = 1,
            widget = function()
                _G.widget_refreshes = _G.widget_refreshes + 1
                return { kind = "text", value = tostring(_G.widget_refreshes) }
            end,
        }
        "#,
    )
    .expect("load dyn");

    assert_eq!(host.read_global_i64("dyn", "widget_refreshes"), Some(1));
    host.dispatch_test_event("ThemeChanged", json!({ "name": "dark" }));
    assert_eq!(host.read_global_i64("dyn", "theme_hits"), Some(1));
    assert_eq!(host.read_global_i64("dyn", "widget_refreshes"), Some(2));

    host.host_mut().sync_tab_registry(&tab_snapshot());
    assert_eq!(host.read_global_i64("dyn", "widget_refreshes"), Some(3));
}

#[test]
fn v1_dynamic_widgets_refresh_on_focus_change() {
    use crate::plugin::ui::focus::{
        FocusReason, FocusSnapshot, FocusSurface, RepositoryFocusSurface,
    };

    let mut host = MockHost::new();
    host.load_inline(
        "focus_widget",
        &manifest("focus_widget"),
        r#"
        _G.widget_refreshes = 0
        _G.last_focus_surface = ""

        leviathan.ui.slot.add{
            region = "repository", pane = "graph", section = "top",
            id = "plugin.focus_widget.bar", priority = 1,
            widget = function(ctx)
                _G.widget_refreshes = _G.widget_refreshes + 1
                _G.last_focus_surface = ctx.focus.surface
                return { kind = "text", value = ctx.focus.surface }
            end,
        }
        "#,
    )
    .expect("load");

    let initial_refreshes = host
        .read_global_i64("focus_widget", "widget_refreshes")
        .unwrap_or(0);

    let first = FocusSnapshot {
        surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
        reason: FocusReason::Keymap,
    };
    assert!(host.host_mut().sync_focus(first.clone()));
    let after_first = host
        .read_global_i64("focus_widget", "widget_refreshes")
        .unwrap_or(0);
    assert!(after_first > initial_refreshes);
    assert_eq!(
        host.read_global_string("focus_widget", "last_focus_surface")
            .as_deref(),
        Some("repository.graph")
    );

    assert!(!host.host_mut().sync_focus(first));
    let after_resync = host
        .read_global_i64("focus_widget", "widget_refreshes")
        .unwrap_or(0);
    assert_eq!(after_resync, after_first);

    let next = FocusSnapshot {
        surface: FocusSurface::Repository(RepositoryFocusSurface::Details),
        reason: FocusReason::Keymap,
    };
    assert!(host.host_mut().sync_focus(next));
    let after_change = host
        .read_global_i64("focus_widget", "widget_refreshes")
        .unwrap_or(0);
    assert!(after_change > after_first);
    assert_eq!(
        host.read_global_string("focus_widget", "last_focus_surface")
            .as_deref(),
        Some("repository.details")
    );
}

#[test]
fn v1_focus_indicator_pattern_through_existing_slots() {
    use crate::plugin::ui::focus::{
        FocusReason, FocusSnapshot, FocusSurface, RepositoryFocusSurface,
    };

    let mut host = MockHost::new();
    host.load_inline(
        "focus_bars",
        &manifest("focus_bars"),
        r#"
        _G.graph_active = 0
        _G.details_active = 0

        local function bar(ctx)
            if ctx.focus.matches_pane then
                return { kind = "text", value = "BAR" }
            end
            return { kind = "text", value = "" }
        end

        leviathan.ui.slot.add{
            region = "repository", pane = "graph", section = "top",
            id = "plugin.focus_bars.graph", priority = 1,
            widget = function(ctx)
                _G.graph_active = ctx.focus.matches_pane and 1 or 0
                return bar(ctx)
            end,
        }

        leviathan.ui.slot.add{
            region = "repository", pane = "details", section = "top",
            id = "plugin.focus_bars.details", priority = 1,
            widget = function(ctx)
                _G.details_active = ctx.focus.matches_pane and 1 or 0
                return bar(ctx)
            end,
        }
        "#,
    )
    .expect("load focus_bars");

    let graph_focus = FocusSnapshot {
        surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
        reason: FocusReason::Keymap,
    };
    assert!(host.host_mut().sync_focus(graph_focus));
    assert_eq!(host.read_global_i64("focus_bars", "graph_active"), Some(1));
    assert_eq!(
        host.read_global_i64("focus_bars", "details_active"),
        Some(0)
    );

    let details_focus = FocusSnapshot {
        surface: FocusSurface::Repository(RepositoryFocusSurface::Details),
        reason: FocusReason::Keymap,
    };
    assert!(host.host_mut().sync_focus(details_focus));
    assert_eq!(host.read_global_i64("focus_bars", "graph_active"), Some(0));
    assert_eq!(
        host.read_global_i64("focus_bars", "details_active"),
        Some(1)
    );
}

#[test]
fn v1_dynamic_widgets_refresh_only_declared_dependencies() {
    let mut host = MockHost::new();
    host.load_inline(
        "deps",
        &manifest("deps"),
        r#"
        _G.widget_refreshes = 0

        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.deps.counter",
            priority = 1,
            depends_on = { "selection" },
            widget = function()
                _G.widget_refreshes = _G.widget_refreshes + 1
                return { kind = "text", value = tostring(_G.widget_refreshes) }
            end,
        }
        "#,
    )
    .expect("load deps");

    assert_eq!(host.read_global_i64("deps", "widget_refreshes"), Some(1));
    host.dispatch_test_event("ThemeChanged", json!({ "name": "dark" }));
    assert_eq!(host.read_global_i64("deps", "widget_refreshes"), Some(1));
    host.host_mut().sync_tab_registry(&tab_snapshot());
    assert_eq!(host.read_global_i64("deps", "widget_refreshes"), Some(1));
    host.dispatch_test_event("CommitSelected", json!({ "hash": "abc123" }));
    assert_eq!(host.read_global_i64("deps", "widget_refreshes"), Some(2));

    let snap = host.introspect();
    let row = snap
        .ui_render_diagnostics
        .iter()
        .find(|row| row.plugin_id == "deps" && row.slot_id == "plugin.deps.counter")
        .expect("ui render diagnostics row");
    assert_eq!(row.dependencies, vec!["selection"]);
    assert_eq!(row.last_refresh_causes, vec!["selection_changed"]);
    assert_eq!(row.refresh_count, 2);
}

#[test]
fn v1_dynamic_widget_errors_keep_last_good_ast_and_surface_badge() {
    let mut host = MockHost::new();
    host.load_inline(
        "stale",
        &manifest("stale"),
        r#"
        _G.fail = false

        leviathan.autocmd.create("ThemeChanged", {
            callback = function()
                _G.fail = true
            end,
        })

        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.stale.widget",
            priority = 1,
            depends_on = { "theme" },
            widget = function()
                if _G.fail then error("render failed") end
                return { kind = "text", value = "last good" }
            end,
        }
        "#,
    )
    .expect("load stale");

    host.dispatch_test_event("ThemeChanged", json!({ "name": "dark" }));
    let snap = host.introspect();
    let row = snap
        .ui_render_diagnostics
        .iter()
        .find(|row| row.plugin_id == "stale" && row.slot_id == "plugin.stale.widget")
        .expect("ui render diagnostics row");
    assert_eq!(row.last_status, "stale");
    assert!(row.diagnostic_badge);
    assert_eq!(row.stale_count, 1);
    assert!(row
        .last_error
        .as_deref()
        .unwrap_or_default()
        .contains("render failed"));
}

#[test]
fn v1_remove_replace_replay_in_source_order() {
    let mut host = MockHost::new();
    host.load_inline(
        "ops",
        &manifest("ops"),
        r#"
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.ops.target",
            priority = 10, widget = { kind = "text", value = "add" },
        }
        leviathan.ui.slot.replace(
            { region = "main_bar", section = "left", id = "plugin.ops.target" },
            {
                region = "main_bar", section = "left", id = "plugin.ops.target",
                priority = 20, widget = { kind = "text", value = "replace" },
            }
        )
        "#,
    )
    .expect("load ops");

    let registry = main_bar_registry(&host);
    let slot = registry
        .iter_container(Container::Section("left".into()))
        .find(|slot| slot.id == "plugin.ops.target")
        .expect("replaced slot");
    assert_eq!(slot.priority, 20);

    host.reload_with_str(
        "ops",
        &manifest("ops"),
        r#"
        leviathan.ui.slot.remove{
            region = "main_bar", section = "left", id = "plugin.ops.target",
        }
        "#,
    )
    .expect("reload remove");

    let registry = main_bar_registry(&host);
    assert!(!registry.contains_display_id("plugin.ops.target"));
}

#[test]
fn v1_unload_cleans_slots_and_extension_points() {
    let mut host = MockHost::new();
    host.load_inline(
        "owned",
        &manifest_with_caps("owned", EXT_CAPS),
        r##"
        leviathan.ui.slot.add{
            region = "main_bar", section = "right", id = "plugin.owned.slot",
            priority = 10, widget = { kind = "text", value = "x" },
        }
        leviathan.ui.overlay{
            id = "owned_overlay",
            widget = { kind = "text", value = "overlay" },
        }
        leviathan.ui.context_menu("repository.diff.context_menu", {
            id = "owned_menu", label = "Owned", command = "owned.run", priority = 1,
        })
        leviathan.ui.graph_decoration("abc123", {
            kind = "marker", shape = "dot", color = "#ffffff",
        })
        leviathan.ui.diff_decoration({
            kind = "line_gutter", file = "src/lib.rs", line = 3, glyph = "!",
        })
        "##,
    )
    .expect("load owned");

    let pre = host.introspect();
    assert!(pre.slots.iter().any(|slot| slot.owner_plugin_id == "owned"));
    assert!(pre.overlays.iter().any(|row| row.plugin_id == "owned"));
    assert!(pre
        .context_menu_items
        .iter()
        .any(|row| row.plugin_id == "owned"));
    assert!(pre
        .graph_decorations
        .iter()
        .any(|row| row.plugin_id == "owned"));
    assert!(pre
        .diff_decorations
        .iter()
        .any(|row| row.plugin_id == "owned"));

    host.unload_plugin("owned").expect("unload");

    let post = host.introspect();
    assert!(post
        .slots
        .iter()
        .all(|slot| slot.owner_plugin_id != "owned"));
    assert!(post.overlays.iter().all(|row| row.plugin_id != "owned"));
    assert!(post
        .context_menu_items
        .iter()
        .all(|row| row.plugin_id != "owned"));
    assert!(post
        .graph_decorations
        .iter()
        .all(|row| row.plugin_id != "owned"));
    assert!(post
        .diff_decorations
        .iter()
        .all(|row| row.plugin_id != "owned"));
}

#[test]
fn v1_screen_state_uses_serialize_and_deserialize_on_reload() {
    let mut host = MockHost::new();
    host.load_inline(
        "screen_state",
        &manifest("screen_state"),
        r#"
        leviathan.ui.screen.register{
            id = "counter",
            init = function() return { n = 0 } end,
            view = function(s) return { kind = "text", value = tostring(s.n) } end,
            update = function(s, evt)
                if evt == "inc" then s.n = s.n + 1 end
                return s
            end,
            serialize = function(s) return { saved = s.n } end,
            deserialize = function(t) return { n = t.saved + 10 } end,
        }
        "#,
    )
    .expect("load screen_state");

    host.open_screen("screen_state", "counter");
    host.dispatch_screen_event("screen_state", "counter", "inc");
    host.dispatch_screen_event("screen_state", "counter", "inc");
    assert_eq!(
        host.screen_state_json("screen_state", "counter")
            .get("n")
            .and_then(|v| v.as_i64()),
        Some(2)
    );

    host.reload_plugin("screen_state").expect("reload");

    assert_eq!(
        host.screen_state_json("screen_state", "counter")
            .get("n")
            .and_then(|v| v.as_i64()),
        Some(12)
    );
}

#[test]
fn v1_screen_navigation_effects_are_typed() {
    let mut host = MockHost::new();
    host.load_inline(
        "screen_nav",
        &manifest("screen_nav"),
        r#"
        leviathan.ui.screen.register{
            id = "main",
            title = "Main",
            init = function(ctx) return { surface = ctx.surface } end,
            view = function(state, ctx) return { kind = "text", value = ctx.surface } end,
            update = function(state, event, value, ctx)
                return {
                    state = state,
                    effects = {
                        { kind = "open_screen", plugin = "screen_nav", screen = "main" },
                        { kind = "navigate", target = { kind = "repository" } },
                    },
                }
            end,
            serialize = function(state) return { surface = state.surface } end,
            deserialize = function(value, ctx) return { surface = value.surface or ctx.surface } end,
            can_close = function(state, ctx) return ctx.surface == "screen" end,
        }
        "#,
    )
    .expect("load screen_nav");

    host.open_screen("screen_nav", "main");
    host.dispatch_screen_event("screen_nav", "main", "go");
    let effects = host.host_mut().take_pending_navigation_effects();
    assert_eq!(
        effects,
        vec![
            crate::plugin::navigation::PluginNavigationEffect::OpenScreen {
                plugin_id: "screen_nav".into(),
                screen_id: "main".into(),
            },
            crate::plugin::navigation::PluginNavigationEffect::NavigateRepository,
        ]
    );
    assert!(host.host().can_close_screen("screen_nav", "main"));
}

#[test]
fn v1_invalid_navigation_effects_emit_diagnostics() {
    let mut host = MockHost::new();
    host.load_inline(
        "bad_nav",
        &manifest("bad_nav"),
        r#"
        leviathan.ui.screen.register{
            id = "main",
            init = function() return {} end,
            view = function() return { kind = "text", value = "bad" } end,
            update = function()
                return { effects = { { kind = "open_screen" }, { kind = "navigate", target = { kind = "nowhere" } } } }
            end,
        }
        "#,
    )
    .expect("load bad_nav");

    host.open_screen("bad_nav", "main");
    host.dispatch_screen_event("bad_nav", "main", "go");
    assert_eq!(host.host_mut().take_pending_navigation_effects().len(), 0);
    assert!(
        host.diagnostics()
            .by_code("navigation.invalid_effect")
            .len()
            >= 2
    );
}

#[test]
fn v1_overlay_context_menu_and_decorations_project_to_devtools() {
    let mut host = MockHost::new();
    host.load_inline(
        "ext",
        &manifest_with_caps("ext", EXT_CAPS),
        r##"
        leviathan.ui.overlay{
            id = "quick_open",
            priority = 7,
            dismissible = false,
            key_events = { "Esc" },
            widget = { kind = "text", value = "overlay" },
        }
        leviathan.ui.context_menu("repository.diff.context_menu", {
            id = "stage", label = "Stage", command = "git.stage", priority = 3,
            condition_capability = "git:write",
        })
        leviathan.ui.graph_decoration("abc123", {
            kind = "badge", text = "WIP", fg = "#ffffff", bg = "#000000",
        })
        leviathan.ui.diff_decoration({
            kind = "line_hint", severity = "info", text = "note",
            file = "src/lib.rs", line = 9,
        })
        "##,
    )
    .expect("load ext");

    let snap = host.introspect();
    let overlay = snap
        .overlays
        .iter()
        .find(|row| row.plugin_id == "ext" && row.id == "quick_open")
        .expect("overlay");
    assert_eq!(overlay.priority, 7);
    assert!(!overlay.dismissible);
    assert_eq!(overlay.key_events, vec!["esc"]);
    assert_eq!(overlay.widget.node.kind(), "text");

    let item = snap
        .context_menu_items
        .iter()
        .find(|row| row.plugin_id == "ext" && row.id == "stage")
        .expect("context menu");
    assert_eq!(item.region, "repository.diff.context_menu");
    assert_eq!(item.condition_capability.as_deref(), Some("git:write"));

    let graph = snap
        .graph_decorations
        .iter()
        .find(|row| row.plugin_id == "ext")
        .expect("graph decoration");
    assert_eq!(graph.commit_hash, "abc123");
    assert_eq!(graph.kind, "badge");

    let diff = snap
        .diff_decorations
        .iter()
        .find(|row| row.plugin_id == "ext")
        .expect("diff decoration");
    assert_eq!(diff.kind, "line_hint");
    assert_eq!(diff.decoration["line"], 9);
}

#[test]
fn status_bar_is_not_a_public_slot_region_until_mounted() {
    let mut host = MockHost::new();
    let err = host
        .load_inline(
            "status",
            &manifest("status"),
            r#"
        assert(leviathan.ui.slot.add{
            region = "status_bar", section = "left", id = "plugin.status.left",
            priority = 1, widget = { kind = "text", value = "status" },
        })
        "#,
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown region: status_bar"), "got: {err}");
}

#[test]
fn repository_region_accepts_graph_and_details_panes() {
    let mut host = MockHost::new();
    host.load_inline(
        "repo_panes",
        &manifest("repo_panes"),
        r#"
        leviathan.ui.slot.add{
            region = "repository", pane = "graph", section = "top",
            id = "plugin.repo.graph", priority = 1,
            widget = { kind = "text", value = "graph" },
        }
        leviathan.ui.slot.add{
            region = "repository", pane = "details", section = "bottom",
            id = "plugin.repo.details", priority = 1,
            widget = { kind = "text", value = "details" },
        }
        "#,
    )
    .expect("repository graph/details panes should register and render");

    let registry = repo_registry(&host);
    assert!(rr::render_top(&registry, rr::Pane::Graph).is_some());
    assert!(rr::render_bottom(&registry, rr::Pane::Details).is_some());
}

#[test]
fn contract_debt_same_slot_id_in_different_sections_should_render_both() {
    let mut host = MockHost::new();
    host.load_inline(
        "identity",
        &manifest("identity"),
        r#"
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "same.id",
            priority = 1, widget = { kind = "text", value = "left" },
        }
        leviathan.ui.slot.add{
            region = "main_bar", section = "right", id = "same.id",
            priority = 1, widget = { kind = "text", value = "right" },
        }
        "#,
    )
    .expect("load identity");

    let registry = main_bar_registry(&host);
    assert!(registry
        .iter_container(Container::Section("left".into()))
        .any(|slot| slot.id == "same.id"));
    assert!(registry
        .iter_container(Container::Section("right".into()))
        .any(|slot| slot.id == "same.id"));
    assert!(!host
        .diagnostics()
        .by_code("schema.slot_id_reused")
        .is_empty());
}

#[test]
fn contract_debt_dynamic_widget_cache_should_use_full_slot_address() {
    let mut host = MockHost::new();
    host.load_inline(
        "dyn_identity",
        &manifest("dyn_identity"),
        r#"
        _G.left_refreshes = 0
        _G.right_refreshes = 0
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "same.dynamic",
            priority = 1,
            widget = function()
                _G.left_refreshes = _G.left_refreshes + 1
                return { kind = "text", value = "left" }
            end,
        }
        leviathan.ui.slot.add{
            region = "main_bar", section = "right", id = "same.dynamic",
            priority = 1,
            widget = function()
                _G.right_refreshes = _G.right_refreshes + 1
                return { kind = "text", value = "right" }
            end,
        }
        "#,
    )
    .expect("load dyn_identity");

    assert_eq!(
        host.read_global_i64("dyn_identity", "left_refreshes"),
        Some(1)
    );
    assert_eq!(
        host.read_global_i64("dyn_identity", "right_refreshes"),
        Some(1)
    );
}

#[test]
fn duplicate_same_slot_address_is_diagnosed() {
    let mut host = MockHost::new();
    host.load_inline(
        "dupe_identity",
        &manifest("dupe_identity"),
        r#"
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "same.address",
            priority = 1, widget = { kind = "text", value = "first" },
        }
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "same.address",
            priority = 2, widget = { kind = "text", value = "second" },
        }
        "#,
    )
    .expect("load dupe_identity");

    assert!(!host
        .diagnostics()
        .by_code("schema.slot_duplicate_address")
        .is_empty());
}

#[test]
fn no_ui_capability_cannot_mutate_ui_and_reports_target() {
    let mut host = MockHost::new();
    let err = host
        .load_inline(
            "no_ui",
            &manifest_with_caps("no_ui", "[]"),
            r#"
        assert(leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.no_ui.slot",
            priority = 1, widget = { kind = "text", value = "x" },
        })
        "#,
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("no_ui"), "got: {err}");
    assert!(err.contains("ui:region:main_bar"), "got: {err}");
    assert!(
        err.contains("main_bar:left:plugin.no_ui.slot"),
        "got: {err}"
    );

    let diagnostics = host.diagnostics().by_code("capability.denied");
    let diag = diagnostics
        .iter()
        .find(|d| d.plugin_id.as_str() == "no_ui")
        .expect("capability denial diagnostic");
    assert_eq!(diag.context["target"], "main_bar:left:plugin.no_ui.slot");
    assert_eq!(diag.context["capability"], "ui:region:main_bar");
    assert!(diag.context["source_location"]
        .as_str()
        .unwrap_or_default()
        .contains("init.lua"));
    assert!(diag.context["remediation"]
        .as_str()
        .unwrap_or_default()
        .contains("ui:region:main_bar"));

    let audit = host.host().audit_log().entries();
    assert!(audit.iter().any(|e| {
        e.plugin_id == "no_ui"
            && e.capability == "ui:region:main_bar"
            && e.target == "main_bar:left:plugin.no_ui.slot"
            && e.outcome == AuditOutcome::Denied
    }));
}

#[test]
fn builtin_replace_and_remove_are_capability_gated() {
    let mut denied = MockHost::new();
    let err = denied
        .load_inline(
            "builtin_denied",
            &manifest_with_caps("builtin_denied", r#"["ui:region:main_bar"]"#),
            r#"
        assert(leviathan.ui.slot.replace(
            { region = "main_bar", section = "left", id = "builtin.repo_info" },
            {
                region = "main_bar", section = "left", id = "builtin.repo_info",
                priority = 20, widget = { kind = "text", value = "x" },
            }
        ))
        "#,
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("ui:replace:builtin"), "got: {err}");

    let mut allowed = MockHost::new();
    allowed
        .load_inline(
            "builtin_allowed",
            &manifest_with_caps(
                "builtin_allowed",
                r#"["ui:region:main_bar", "ui:replace:builtin", "ui:remove:builtin"]"#,
            ),
            r#"
        leviathan.ui.slot.replace(
            { region = "main_bar", section = "left", id = "builtin.repo_info" },
            {
                region = "main_bar", section = "left", id = "builtin.repo_info",
                priority = 20, widget = { kind = "text", value = "x" },
            }
        )
        assert(leviathan.ui.slot.remove{
            region = "main_bar", section = "left", id = "builtin.branch_info",
        })
        "#,
        )
        .expect("builtin mutation with caps");
}

#[test]
fn builtins_appear_in_devtools_contribution_tree() {
    let host = MockHost::new();
    let snap = host.introspect();
    assert!(snap.slots.iter().any(|slot| {
        slot.owner_plugin_id == "builtin"
            && slot.region == "main_bar"
            && slot.container == "left"
            && slot.id == "builtin.repo_info"
            && slot.replace_capability.as_deref()
                == Some("ui:replace:main_bar:left:builtin.repo_info")
            && slot.remove_capability.as_deref()
                == Some("ui:remove:main_bar:left:builtin.repo_info")
    }));
    assert!(snap.contribution_tree.iter().any(|node| {
        node.plugin_id == "builtin"
            && node.kind == "slot"
            && node.location == "main_bar:left"
            && node.id == "builtin.repo_info"
            && node.visible
    }));
}

#[test]
fn builtin_ids_are_reserved_for_replace_api() {
    let mut host = MockHost::new();
    let err = host
        .load_inline(
            "bad_builtin_add",
            &manifest_with_caps("bad_builtin_add", r#"["ui:region:main_bar"]"#),
            r#"
            assert(leviathan.ui.slot.add{
                region = "main_bar", section = "left", id = "builtin.repo_info",
                priority = 1, widget = { kind = "text", value = "bad" },
            })
            "#,
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("use leviathan.ui.slot.replace"), "got: {err}");
}

#[test]
fn documented_builtin_replacement_is_visible_without_native_special_case() {
    let mut host = MockHost::new();
    host.load_inline(
        "replace_repo",
        &manifest_with_caps(
            "replace_repo",
            r#"["ui:region:main_bar", "ui:replace:builtin"]"#,
        ),
        r#"
        local handle, err = leviathan.ui.slot.replace(
            { region = "main_bar", section = "left", id = "builtin.repo_info" },
            {
                region = "main_bar", section = "left", id = "builtin.repo_info",
                priority = 7, widget = { kind = "text", value = "replacement" },
            }
        )
        assert(handle, err)
        "#,
    )
    .expect("replace builtin");

    let registry = main_bar_registry(&host);
    let slot = registry
        .iter_container(Container::Section("left".into()))
        .find(|slot| slot.id == "builtin.repo_info")
        .expect("repo slot");
    assert_eq!(slot.priority, 7);

    let snap = host.introspect();
    let row = snap
        .slots
        .iter()
        .find(|slot| {
            slot.region == "main_bar" && slot.container == "left" && slot.id == "builtin.repo_info"
        })
        .expect("slot summary");
    assert_eq!(row.owner_plugin_id, "replace_repo");
    assert_eq!(row.effective_priority, 7);
}

#[test]
fn default_builtin_registry_order_is_unchanged() {
    let host = MockHost::new();
    let main = main_bar_registry(&host);
    let main_ids: Vec<(&str, Vec<&str>)> = ["left", "center", "right"]
        .into_iter()
        .map(|section| {
            (
                section,
                main.iter_container(Container::Section(section.to_string()))
                    .map(|slot| slot.id.as_str())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        main_ids,
        vec![
            (
                "left",
                vec![
                    "builtin.repo_info",
                    "builtin.separator_chevron",
                    "builtin.branch_info",
                    "builtin.fetch_indicator"
                ]
            ),
            (
                "center",
                vec![
                    "builtin.pull",
                    "builtin.push",
                    "builtin.branch",
                    "builtin.stash",
                    "builtin.pop"
                ]
            ),
            ("right", vec!["builtin.search"]),
        ]
    );

    let tabs = tab_bar_registry(&host);
    let tab_ids: Vec<(&str, Vec<&str>)> = ["left", "center", "right"]
        .into_iter()
        .map(|section| {
            (
                section,
                tabs.iter_container(Container::Section(section.to_string()))
                    .map(|slot| slot.id.as_str())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        tab_ids,
        vec![
            ("left", vec!["builtin.plus_button"]),
            ("center", vec!["builtin.tab_list"]),
            ("right", vec!["builtin.version_label"]),
        ]
    );
}

#[test]
fn per_target_replace_and_remove_grants_work() {
    let mut host = MockHost::new();
    host.load_inline(
        "owner",
        &manifest_with_caps("owner", r#"["ui:region:main_bar"]"#),
        r#"
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.owner.slot",
            priority = 10, widget = { kind = "text", value = "owner" },
        }
        "#,
    )
    .expect("owner load");

    host.load_inline(
        "editor",
        &manifest_with_caps(
            "editor",
            r#"["ui:region:main_bar", "ui:replace:main_bar:left:plugin.owner.slot"]"#,
        ),
        r#"
        leviathan.ui.slot.replace(
            { plugin_id = "owner", region = "main_bar", section = "left", id = "plugin.owner.slot" },
            {
                region = "main_bar", section = "left", id = "plugin.owner.slot",
                priority = 30, widget = { kind = "text", value = "editor" },
            }
        )
        "#,
    )
    .expect("editor can replace target");

    let registry = main_bar_registry(&host);
    let slot = registry
        .iter_container(Container::Section("left".into()))
        .find(|slot| slot.id == "plugin.owner.slot")
        .expect("target slot");
    assert_eq!(slot.priority, 30);

    host.load_inline(
        "remover",
        &manifest_with_caps(
            "remover",
            r#"["ui:region:main_bar", "ui:remove:main_bar:left:plugin.owner.slot"]"#,
        ),
        r#"
        assert(leviathan.ui.slot.remove{
            plugin_id = "owner", region = "main_bar", section = "left", id = "plugin.owner.slot",
        })
        "#,
    )
    .expect("remover can remove target");

    let registry = main_bar_registry(&host);
    assert!(!registry.contains_display_id("plugin.owner.slot"));
}

#[test]
fn contribution_overrides_hide_reorder_survive_reload_and_reset() {
    let mut host = MockHost::new();
    host.load_inline(
        "layout_owner",
        &manifest("layout_owner"),
        r#"
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.layout.slot",
            priority = 80, widget = { kind = "text", value = "layout" },
        }
        "#,
    )
    .expect("load");

    assert!(main_bar_registry(&host).contains_display_id("plugin.layout.slot"));
    assert!(host
        .invoke_command(
            "plugin_ui.toggle_contribution",
            json!({
                "plugin_id": "layout_owner",
                "region": "main_bar",
                "container": "left",
                "id": "plugin.layout.slot",
                "priority": 5,
            }),
        )
        .is_ok());
    assert!(!main_bar_registry(&host).contains_display_id("plugin.layout.slot"));

    let snap = host.introspect();
    let row = snap
        .slots
        .iter()
        .find(|slot| slot.id == "plugin.layout.slot")
        .expect("slot summary remains inspectable");
    assert!(row.hidden);
    assert_eq!(row.effective_priority, 5);

    host.reload_with_str(
        "layout_owner",
        &manifest("layout_owner"),
        r#"
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.layout.slot",
            priority = 99, widget = { kind = "text", value = "updated" },
        }
        "#,
    )
    .expect("reload");
    assert!(!main_bar_registry(&host).contains_display_id("plugin.layout.slot"));

    assert!(host
        .invoke_command("plugin_ui.reset_layout", json!({}))
        .is_ok());
    let registry = main_bar_registry(&host);
    let slot = registry
        .iter_container(Container::Section("left".into()))
        .find(|slot| slot.id == "plugin.layout.slot")
        .expect("slot visible after reset");
    assert_eq!(slot.priority, 99);
}

#[test]
fn contribution_overrides_can_reorder_without_hiding() {
    let mut host = MockHost::new();
    host.load_inline(
        "layout_order",
        &manifest("layout_order"),
        r#"
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.layout.late",
            priority = 80, widget = { kind = "text", value = "late" },
        }
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.layout.early",
            priority = 10, widget = { kind = "text", value = "early" },
        }
        "#,
    )
    .expect("load");

    assert!(host
        .invoke_command(
            "plugin_ui.toggle_contribution",
            json!({
                "plugin_id": "layout_order",
                "region": "main_bar",
                "container": "left",
                "id": "plugin.layout.late",
                "hidden": false,
                "priority": 5,
            }),
        )
        .is_ok());

    let registry = main_bar_registry(&host);
    let ids: Vec<&str> = registry
        .iter_container(Container::Section("left".into()))
        .map(|slot| slot.id.as_str())
        .collect();
    let late = ids
        .iter()
        .position(|id| *id == "plugin.layout.late")
        .expect("late slot");
    let early = ids
        .iter()
        .position(|id| *id == "plugin.layout.early")
        .expect("early slot");
    assert!(late < early, "ids: {ids:?}");
}

#[test]
fn contribution_overrides_can_hide_builtins() {
    let mut host = MockHost::new();
    assert!(main_bar_registry(&host).contains_display_id("builtin.repo_info"));
    assert!(host
        .invoke_command(
            "plugin_ui.toggle_contribution",
            json!({
                "plugin_id": "builtin",
                "region": "main_bar",
                "container": "left",
                "id": "builtin.repo_info",
            }),
        )
        .is_ok());
    assert!(!main_bar_registry(&host).contains_display_id("builtin.repo_info"));
}

#[test]
fn contribution_overrides_survive_host_restart_with_configured_storage_base() {
    let tmp = tempfile::tempdir().expect("tmp");
    let base = tmp.path().join("plugin-storage");
    let args = json!({
        "plugin_id": "builtin",
        "region": "main_bar",
        "container": "left",
        "id": "builtin.repo_info",
        "hidden": true,
        "priority": 3,
    });

    let mut first = crate::plugin::host::PluginHost::new();
    first.set_plugin_storage_base(&base);
    let (outcome, result) = first.invoke_devtools_command("plugin_ui.toggle_contribution", args);
    assert!(outcome.is_ok());
    assert_eq!(result["hidden"].as_bool(), Some(true));
    assert!(base
        .join("state")
        .join("host")
        .join("contribution_overrides.json")
        .exists());

    let mut second = crate::plugin::host::PluginHost::new();
    second.set_plugin_storage_base(&base);
    let (outcome, result) = second.invoke_devtools_command(
        "plugin_ui.inspect_contribution",
        json!({
            "plugin_id": "builtin",
            "region": "main_bar",
            "container": "left",
            "id": "builtin.repo_info",
        }),
    );
    assert!(outcome.is_ok());
    assert_eq!(result["hidden"].as_bool(), Some(true));
    assert_eq!(result["priority"].as_i64(), Some(3));
}

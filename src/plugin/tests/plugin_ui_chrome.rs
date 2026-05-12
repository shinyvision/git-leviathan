use crate::plugin::tests::harness::MockHost;
use crate::plugin::ui::focus::{FocusReason, FocusSnapshot, FocusSurface, RepositoryFocusSurface};
use crate::widgets::chrome::repo_region::{ChromePane, RepoChromeRegistry};

const CHROME_GRAPH: &str = r#"["ui:chrome:repository.graph.chrome"]"#;
const CHROME_DETAILS: &str = r#"["ui:chrome:repository.details.chrome"]"#;
const CHROME_ALL: &str = r#"["ui:chrome:repository.sidebar.chrome", "ui:chrome:repository.graph.chrome", "ui:chrome:repository.details.chrome", "ui:chrome:repository.diff.chrome"]"#;

fn manifest(id: &str, caps: &str) -> String {
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

fn build_chrome_registry(host: &MockHost) -> RepoChromeRegistry {
    let mut registry = RepoChromeRegistry::new();
    host.host().apply_repo_chrome_slots(&mut registry);
    registry
}

#[test]
fn chrome_requires_capability_to_register() {
    let mut host = MockHost::new();
    let res = host.load_inline(
        "no_cap",
        &manifest("no_cap", "[]"),
        r#"
        local handle, err = leviathan.ui.contribute("repository.graph.chrome", {
            id = "border",
            widget = function() return { kind = "container", width = "fill", height = "fill" } end,
        })
        _G.handle = handle
        _G.err = err
        "#,
    );
    assert!(
        res.is_ok(),
        "load with cap-denied contribute should not abort"
    );
    let err = host.read_global_string("no_cap", "err").unwrap_or_default();
    assert!(
        err.contains("capability denied") || err.contains("ui:chrome"),
        "expected capability denial error, got: {err}"
    );
    let registry = build_chrome_registry(&host);
    assert!(registry.is_empty(ChromePane::Graph));
}

#[test]
fn chrome_unknown_point_rejected() {
    let mut host = MockHost::new();
    host.load_inline(
        "unknown_point",
        &manifest("unknown_point", CHROME_GRAPH),
        r#"
        local handle, err = leviathan.ui.contribute("repository.unknown.chrome", {
            id = "border",
            widget = function() return nil end,
        })
        _G.handle = handle
        _G.err = err
        "#,
    )
    .expect("load");
    let err = host
        .read_global_string("unknown_point", "err")
        .unwrap_or_default();
    assert!(
        err.contains("unknown") || err.contains("does not accept"),
        "expected unknown-point error, got: {err}"
    );
}

#[test]
fn chrome_widget_sees_focus_matches_surface() {
    let mut host = MockHost::new();
    host.load_inline(
        "focus_chrome",
        &manifest("focus_chrome", CHROME_GRAPH),
        r#"
        _G.matches_graph = 0
        leviathan.ui.contribute("repository.graph.chrome", {
            id = "border",
            widget = function(ctx)
                _G.matches_graph = ctx.focus.matches_surface and 1 or 0
                if ctx.focus.matches_surface then
                    return { kind = "container", width = "fill", height = "fill" }
                end
                return nil
            end,
        })
        "#,
    )
    .expect("load");

    let graph = FocusSnapshot {
        surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
        reason: FocusReason::Keymap,
    };
    assert!(host.host_mut().sync_focus(graph));
    assert_eq!(
        host.read_global_i64("focus_chrome", "matches_graph"),
        Some(1)
    );

    let details = FocusSnapshot {
        surface: FocusSurface::Repository(RepositoryFocusSurface::Details),
        reason: FocusReason::Keymap,
    };
    assert!(host.host_mut().sync_focus(details));
    assert_eq!(
        host.read_global_i64("focus_chrome", "matches_graph"),
        Some(0)
    );
}

#[test]
fn chrome_layout_unchanged_when_no_chrome_registered() {
    let registry = RepoChromeRegistry::new();
    for pane in [
        ChromePane::Sidebar,
        ChromePane::Graph,
        ChromePane::Details,
        ChromePane::Diff,
    ] {
        assert!(registry.is_empty(pane));
        assert!(registry.render_overlay(pane).is_none());
    }
}

#[test]
fn chrome_layers_sorted_by_priority_then_plugin_then_id() {
    let mut host = MockHost::new();
    host.load_inline(
        "alpha",
        &manifest("alpha", CHROME_GRAPH),
        r#"
        leviathan.ui.contribute("repository.graph.chrome", {
            id = "z", priority = 5,
            widget = function() return { kind = "container", width = "fill", height = "fill" } end,
        })
        leviathan.ui.contribute("repository.graph.chrome", {
            id = "a", priority = 5,
            widget = function() return { kind = "container", width = "fill", height = "fill" } end,
        })
        leviathan.ui.contribute("repository.graph.chrome", {
            id = "low", priority = 1,
            widget = function() return { kind = "container", width = "fill", height = "fill" } end,
        })
        "#,
    )
    .expect("load alpha");
    host.load_inline(
        "zeta",
        &manifest("zeta", CHROME_GRAPH),
        r#"
        leviathan.ui.contribute("repository.graph.chrome", {
            id = "a", priority = 5,
            widget = function() return { kind = "container", width = "fill", height = "fill" } end,
        })
        "#,
    )
    .expect("load zeta");

    let registry = build_chrome_registry(&host);
    let order: Vec<(String, String, i32)> = registry
        .iter(ChromePane::Graph)
        .map(|s| (s.plugin_id.clone(), s.id.clone(), s.priority))
        .collect();
    assert_eq!(
        order,
        vec![
            ("alpha".into(), "low".into(), 1),
            ("alpha".into(), "a".into(), 5),
            ("alpha".into(), "z".into(), 5),
            ("zeta".into(), "a".into(), 5),
        ]
    );
}

#[test]
fn chrome_unload_removes_contributions() {
    let mut host = MockHost::new();
    host.load_inline(
        "owned",
        &manifest("owned", CHROME_ALL),
        r#"
        leviathan.ui.contribute("repository.sidebar.chrome", {
            id = "side", widget = function() return nil end,
        })
        leviathan.ui.contribute("repository.graph.chrome", {
            id = "graph", widget = function() return nil end,
        })
        "#,
    )
    .expect("load owned");

    let pre = host.host().chrome_contributions();
    assert!(pre.iter().any(|(p, _, _, _)| p == "owned"));

    host.unload_plugin("owned").expect("unload");

    let post = host.host().chrome_contributions();
    assert!(post.iter().all(|(p, _, _, _)| p != "owned"));
    let registry = build_chrome_registry(&host);
    assert!(registry.is_empty(ChromePane::Sidebar));
    assert!(registry.is_empty(ChromePane::Graph));
}

#[test]
fn chrome_contribution_handle_format() {
    let mut host = MockHost::new();
    host.load_inline(
        "handle_shape",
        &manifest("handle_shape", CHROME_GRAPH),
        r#"
        local handle = leviathan.ui.contribute("repository.graph.chrome", {
            id = "border",
            widget = function() return nil end,
        })
        _G.handle_id = handle.id
        _G.handle_point = handle.point_id
        _G.handle_plugin = handle.plugin_id
        _G.handle_full = handle.handle
        "#,
    )
    .expect("load");

    assert_eq!(
        host.read_global_string("handle_shape", "handle_id")
            .as_deref(),
        Some("border")
    );
    assert_eq!(
        host.read_global_string("handle_shape", "handle_point")
            .as_deref(),
        Some("repository.graph.chrome")
    );
    assert_eq!(
        host.read_global_string("handle_shape", "handle_plugin")
            .as_deref(),
        Some("handle_shape")
    );
    assert_eq!(
        host.read_global_string("handle_shape", "handle_full")
            .as_deref(),
        Some("repository.graph.chrome:border")
    );
}

#[test]
fn chrome_handle_remove_drops_contribution() {
    let mut host = MockHost::new();
    host.load_inline(
        "rm",
        &manifest("rm", CHROME_DETAILS),
        r#"
        local handle = leviathan.ui.contribute("repository.details.chrome", {
            id = "border",
            widget = function() return { kind = "container", width = "fill", height = "fill" } end,
        })
        local ok, err = handle:remove()
        _G.removed_ok = ok and 1 or 0
        _G.removed_err = err
        "#,
    )
    .expect("load");
    assert_eq!(host.read_global_i64("rm", "removed_ok"), Some(1));
    let registry = build_chrome_registry(&host);
    assert!(registry.is_empty(ChromePane::Details));
}

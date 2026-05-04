use serde_json::json;

use crate::plugin::commands::InvokeOutcome;
use crate::plugin::tests::harness::MockHost;

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

const DOCK_INIT: &str = r#"
local handle, err = leviathan.ui.dock.register({
    id = "panel",
    title = "Dock Panel",
    area = "right",
    default_open = true,
    view = function(ctx)
        _G.ctx_type = ctx.type
        local state = ctx.payload.dock.state or {}
        _G.view_count = state.count or 0
        return { kind = "text", value = "count:" .. tostring(_G.view_count) }
    end,
    update = function(state, event, value)
        state = state or {}
        state.count = (state.count or 0) + 1
        state.last_event = event
        state.last_value = value
        return { state = state }
    end,
})
assert(handle, err)
_G.handle_key = handle.key
"#;

#[test]
fn dock_panel_registers_renders_and_updates_state() {
    let mut host = MockHost::new();
    host.load_inline("dockp", &manifest("dockp", r#"["ui:dock"]"#), DOCK_INIT)
        .expect("load");

    let snap = host.introspect();
    let panel = snap
        .dock_panels
        .iter()
        .find(|panel| panel.plugin_id == "dockp" && panel.id == "panel")
        .expect("dock panel");
    assert_eq!(panel.area, "right");
    assert!(panel.open);
    assert!(panel.selected);
    assert_eq!(
        host.read_global_string("dockp", "handle_key").as_deref(),
        Some("dockp:panel")
    );

    let ast = host
        .host()
        .render_dock_panel("dockp", "panel")
        .expect("dock view");
    assert_eq!(ast.node.kind(), "text");
    assert_eq!(
        host.read_global_string("dockp", "ctx_type").as_deref(),
        Some("DockPanelContext")
    );
    assert_eq!(host.read_global_i64("dockp", "view_count"), Some(0));

    host.host()
        .dispatch_dock_event("dockp", "panel", "click", json!({ "button": "primary" }));
    host.host()
        .render_dock_panel("dockp", "panel")
        .expect("dock view after update");
    assert_eq!(host.read_global_i64("dockp", "view_count"), Some(1));
    let panel = host
        .introspect()
        .dock_panels
        .into_iter()
        .find(|panel| panel.plugin_id == "dockp" && panel.id == "panel")
        .expect("panel after update");
    assert_eq!(panel.state["count"], json!(1));
    assert_eq!(panel.state["last_event"], json!("click"));
}

#[test]
fn dock_reload_preserves_state_and_host_layout() {
    let mut host = MockHost::new();
    host.load_inline("dockp", &manifest("dockp", r#"["ui:dock"]"#), DOCK_INIT)
        .expect("load");
    host.host()
        .dispatch_dock_event("dockp", "panel", "tick", json!(1));
    assert!(matches!(
        host.invoke_command(
            "ui.dock.close",
            json!({ "plugin_id": "dockp", "id": "panel" })
        ),
        InvokeOutcome::Ok
    ));

    host.reload_plugin("dockp").expect("reload");
    let panel = host
        .introspect()
        .dock_panels
        .into_iter()
        .find(|panel| panel.plugin_id == "dockp" && panel.id == "panel")
        .expect("panel after reload");
    assert!(!panel.open);
    assert_eq!(panel.state["count"], json!(1));
    assert!(panel.generation_id > 1);
}

#[test]
fn dock_unload_removes_panel_and_layout_rows() {
    let mut host = MockHost::new();
    host.load_inline("dockp", &manifest("dockp", r#"["ui:dock"]"#), DOCK_INIT)
        .expect("load");
    host.host()
        .dispatch_dock_event("dockp", "panel", "tick", json!(1));
    host.unload_plugin("dockp").expect("unload");

    let snap = host.introspect();
    assert!(snap.dock_panels.is_empty());
    let registered = snap
        .dock_layout
        .get("registered")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(!registered
        .iter()
        .any(|key| key.as_str() == Some("dockp:panel")));
}

#[test]
fn dock_capability_required() {
    let mut host = MockHost::new();
    host.load_inline(
        "dockp",
        &manifest("dockp", "[]"),
        r#"
        local handle, err = leviathan.ui.dock.register({
            id = "panel",
            title = "Dock Panel",
            area = "right",
            view = function() return { kind = "text", value = "nope" } end,
        })
        _G.denied = handle == nil and type(err) == "string" and 1 or 0
        "#,
    )
    .expect("load");
    assert!(host.introspect().dock_panels.is_empty());
    assert_eq!(host.read_global_i64("dockp", "denied"), Some(1));
}

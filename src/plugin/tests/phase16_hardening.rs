use std::time::Instant;

use git_leviathan_plugin_api::descriptor::decoration::{
    DiffDecoration, GraphDecoration, MarkerShape,
};
use git_leviathan_plugin_api::descriptor::region::REGIONS;

use crate::plugin::audit::AuditOutcome;
use crate::plugin::extensions::{
    ContextMenuItemRecord, ExtensionRegistry, GraphDecorationRecord, MAX_EXTENSION_RECORDS_PER_KIND,
};
use crate::plugin::tests::harness::MockHost;
use crate::plugin::ui::widget_ast::decode;
use crate::widgets::chrome::main_bar::{builtins as main_bar_builtins, MainBarRegistry};

fn manifest(id: &str, caps: &[&str]) -> String {
    let caps = caps
        .iter()
        .map(|cap| format!(r#""{cap}""#))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"
id = "{id}"
name = "{id}"
version = "0.1.0"
api_version = "1.0"
capabilities = [{caps}]
"#
    )
}

fn main_bar_registry(host: &MockHost) -> MainBarRegistry {
    let mut registry = MainBarRegistry::new();
    main_bar_builtins::register_all(&mut registry);
    host.host().apply_main_bar_slots(&mut registry);
    registry
}

#[test]
fn public_ui_spec_fuzz_denies_or_errors_without_crashing() {
    let mut host = MockHost::new();
    host.load_inline(
        "hostile_specs",
        &manifest("hostile_specs", &[]),
        r##"
        local bad_specs = {
            nil,
            true,
            "x",
            {},
            { region = "main_bar", section = "left", id = "x", priority = 0 },
            { region = "main_bar", section = "left", id = "builtin.fake", priority = 0, widget = { kind = "text", value = "x" } },
            { region = "repository", section = "top", id = "x", priority = 0, widget = { kind = "text", value = "x" } },
            { region = "main_bar", section = "nope", id = "x", priority = 0, widget = { kind = "text", value = "x" } },
            { region = "main_bar", section = "left", id = "x", priority = 0, widget = { kind = "not_real" } },
        }
        for _, spec in ipairs(bad_specs) do
            pcall(function() leviathan.ui.slot.add(spec) end)
        end

        local calls = {
            function() return leviathan.ui.overlay{ id = "o", widget = { kind = "text", value = "x" } } end,
            function() return leviathan.ui.context_menu("repository.diff.context_menu", { id = "m", label = "M", command = "x" }) end,
            function() return leviathan.ui.graph_decoration("abc", { kind = "marker", shape = "dot", color = "#fff" }) end,
            function() return leviathan.ui.diff_decoration({ kind = "line_gutter", file = "f", line = 1, glyph = "!" }) end,
            function() return leviathan.ui.contribute("repository.graph.row_badge", { id = "g", commit_hash = "abc", kind = "badge", text = "G" }) end,
        }
        for _, call in ipairs(calls) do pcall(call) end

        local asset, err = leviathan.assets.load_svg("../escape.svg")
        assert(asset == nil and err)
        local handle, handle_err = leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "safe.inspect", priority = 0,
            widget = { kind = "text", value = "x" },
        }
        if handle then
            assert(type(handle) == "table")
            assert(type(handle.remove) == "function")
            assert(type(handle.handle) == "string")
            assert(type(handle.__host) == "nil")
        else
            assert(handle_err)
        end
        _G.done = 1
        "##,
    )
    .expect("hostile specs load");

    assert_eq!(host.read_global_i64("hostile_specs", "done"), Some(1));
    let audit = host.host().audit_log().entries();
    assert!(audit.iter().any(|e| {
        e.plugin_id == "hostile_specs"
            && e.capability.starts_with("ui:")
            && e.outcome == AuditOutcome::Denied
    }));
    let registry = main_bar_registry(&host);
    assert!(registry.contains_display_id("builtin.fetch_indicator"));
}

#[test]
fn stress_many_plugins_slots_decorations_and_reloads() {
    let mut host = MockHost::new();
    let caps = [
        "ui:region:main_bar",
        "ui:decoration:graph",
        "ui:decoration:diff",
    ];
    for idx in 0..24 {
        let id = format!("stress_{idx}");
        let init = format!(
            r##"
            for i = 1, 4 do
                leviathan.ui.slot.add{{
                    region = "main_bar", section = "right", id = "plugin.{id}." .. i,
                    priority = i, widget = {{ kind = "text", value = "{id}" .. i }},
                }}
            end
            for i = 1, 4 do
                leviathan.ui.graph_decoration("commit-" .. i, {{
                    id = "g" .. i, kind = "badge", text = "G" .. i,
                }})
                leviathan.ui.diff_decoration({{
                    id = "d" .. i, kind = "line_gutter", file = "src/lib.rs", line = i, glyph = "!",
                }})
            end
            "##
        );
        host.load_inline(&id, &manifest(&id, &caps), &init)
            .expect("stress load");
    }

    for _ in 0..4 {
        host.reload_plugin("stress_0").expect("stress reload");
    }

    let snap = host.introspect();
    assert_eq!(snap.plugins.len(), 24);
    assert!(snap.slots.iter().filter(|s| s.region == "main_bar").count() >= 96);
    let graph_count = host
        .host()
        .extension_graph_decorations_for_commit("commit-1")
        .len();
    assert!(
        graph_count >= 24,
        "graph_count={graph_count} diagnostics={:?}",
        host.diagnostics().entries()
    );
    assert!(
        host.host()
            .extension_diff_decorations_for_line("src/lib.rs", 1)
            .len()
            >= 24
    );
}

#[test]
fn repeated_ui_callback_failures_disable_plugin_and_leave_chrome_usable() {
    let mut host = MockHost::new();
    host.load_inline(
        "broken_click",
        &manifest("broken_click", &["ui:region:main_bar"]),
        r#"
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.broken.click",
            priority = 1, widget = { kind = "text", value = "boom" },
            on_click = function() error("click exploded") end,
        }
        "#,
    )
    .expect("load broken_click");

    for _ in 0..5 {
        host.host_mut().dispatch_slot_click(
            "broken_click",
            "main_bar",
            "left",
            "plugin.broken.click",
            "click",
            serde_json::Value::Null,
        );
    }

    assert!(host.host().is_plugin_disabled("broken_click"));
    assert!(host
        .introspect()
        .plugins
        .iter()
        .all(|p| p.id != "broken_click"));
    let registry = main_bar_registry(&host);
    assert!(registry.contains_display_id("builtin.fetch_indicator"));
    assert!(!registry.contains_display_id("plugin.broken.click"));
    let codes = host
        .diagnostics()
        .entries()
        .iter()
        .map(|d| d.code.clone())
        .collect::<Vec<_>>();
    assert!(codes.iter().any(|c| c == "lua.callback_error"));
    assert!(codes.iter().any(|c| c == "performance.tripped"));
    assert!(codes.iter().any(|c| c == "plugin.disable"));
}

#[test]
fn repeated_screen_callback_failures_disable_plugin_and_clear_screen() {
    let mut host = MockHost::new();
    host.load_inline(
        "broken_screen",
        &manifest("broken_screen", &["ui:screen"]),
        r#"
        leviathan.ui.screen.register{
            id = "main",
            title = "Broken",
            init = function() return {} end,
            update = function() error("screen update exploded") end,
            view = function()
                return { kind = "text", value = "screen" }
            end,
        }
        "#,
    )
    .expect("load broken_screen");

    host.open_screen("broken_screen", "main");
    for _ in 0..5 {
        host.dispatch_screen_event("broken_screen", "main", "tick");
    }

    assert!(host.host().is_plugin_disabled("broken_screen"));
    assert_eq!(host.host().active_screen(), None);
    assert!(host.host().widget_tree().is_none());
    let registry = main_bar_registry(&host);
    assert!(registry.contains_display_id("builtin.fetch_indicator"));
    let codes = host
        .diagnostics()
        .entries()
        .iter()
        .map(|d| d.code.clone())
        .collect::<Vec<_>>();
    assert!(codes.iter().any(|c| c == "lua.callback_error"));
    assert!(codes.iter().any(|c| c == "performance.tripped"));
    assert!(codes.iter().any(|c| c == "plugin.disable"));
}

#[test]
fn unload_during_active_screen_clears_mounted_screen() {
    let mut host = MockHost::new();
    host.load_inline(
        "screen_owner",
        &manifest("screen_owner", &["ui:screen"]),
        r#"
        leviathan.ui.screen.register{
            id = "main",
            title = "Main",
            init = function() return { count = 0 } end,
            update = function(state) return { state = state } end,
            view = function()
                return { kind = "text", value = "screen" }
            end,
        }
        "#,
    )
    .expect("load screen_owner");

    host.open_screen("screen_owner", "main");
    assert_eq!(host.host().active_screen(), Some(("screen_owner", "main")));
    assert!(host.host().widget_tree().is_some());

    host.unload_plugin("screen_owner")
        .expect("unload screen_owner");
    assert_eq!(host.host().active_screen(), None);
    assert!(host.host().widget_tree().is_none());
}

#[test]
fn ui_capability_revocation_unmounts_already_mounted_ui() {
    let mut host = MockHost::new();
    host.load_inline(
        "revoked_ui",
        &manifest("revoked_ui", &["ui:region:main_bar"]),
        r#"
        leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.revoked.slot",
            priority = 1, widget = { kind = "text", value = "revoked" },
        }
        "#,
    )
    .expect("load revoked_ui");
    assert!(main_bar_registry(&host).contains_display_id("plugin.revoked.slot"));

    host.host_mut()
        .revoke_capability("revoked_ui", "0.1.0", "ui:region:main_bar")
        .expect("revoke ui");

    let registry = main_bar_registry(&host);
    assert!(!registry.contains_display_id("plugin.revoked.slot"));
    assert!(registry.contains_display_id("builtin.fetch_indicator"));
    assert!(host
        .introspect()
        .plugins
        .iter()
        .all(|p| p.id != "revoked_ui"));
    assert!(host
        .diagnostics()
        .by_code("capability.revoked_ui_unmounted")
        .iter()
        .any(|d| d.plugin_id.as_str() == "revoked_ui"));
}

#[test]
fn contribution_registry_ceiling_and_sorting_stay_bounded() {
    let registry = ExtensionRegistry::new();
    for idx in 0..(MAX_EXTENSION_RECORDS_PER_KIND + 8) {
        registry.add_context_menu_item(ContextMenuItemRecord {
            plugin_id: format!("p{:04}", idx % 8),
            region: "repository.diff.context_menu".into(),
            id: format!("m{idx:04}"),
            label: "Menu".into(),
            command: "cmd.run".into(),
            priority: (idx % 17) as i32,
            condition_capability: None,
            source_location: None,
        });
    }
    let started = Instant::now();
    let items = registry.context_menu_items("repository.diff.context_menu");
    assert!(started.elapsed().as_secs_f32() < 2.0);
    assert_eq!(items.len(), MAX_EXTENSION_RECORDS_PER_KIND);
    assert!(items.windows(2).all(|pair| {
        pair[0]
            .priority
            .cmp(&pair[1].priority)
            .then(pair[0].plugin_id.cmp(&pair[1].plugin_id))
            .then(pair[0].id.cmp(&pair[1].id))
            != std::cmp::Ordering::Greater
    }));
}

#[test]
fn descriptor_lookup_and_decoration_sorting_are_bounded() {
    let started = Instant::now();
    for _ in 0..50_000 {
        assert!(REGIONS.get("main_bar").is_some());
        assert!(REGIONS.get("repository").is_some());
        assert!(REGIONS.get("tab_bar").is_some());
    }
    assert!(started.elapsed().as_secs_f32() < 2.0);

    let registry = ExtensionRegistry::new();
    for idx in (0..600).rev() {
        registry.add_graph_decoration(GraphDecorationRecord {
            plugin_id: format!("p{:03}", idx % 50),
            id: format!("g{idx:03}"),
            commit_hash: "abc".into(),
            decoration: GraphDecoration::Marker {
                shape: MarkerShape::Dot,
                color: "#fff".into(),
            },
            source_location: None,
        });
    }
    let rows = registry.graph_decorations_for("abc");
    assert_eq!(rows.len(), 600);
    assert!(rows.windows(2).all(|pair| {
        pair[0]
            .plugin_id
            .cmp(&pair[1].plugin_id)
            .then(pair[0].id.cmp(&pair[1].id))
            != std::cmp::Ordering::Greater
    }));
}

#[test]
fn widget_ast_and_image_asset_ceilings_reject_over_limit_inputs() {
    let huge = "x".repeat(70 * 1024);
    let err = decode(&serde_json::json!({ "kind": "text", "value": huge })).unwrap_err();
    assert_eq!(err.code, "widget.string_too_long");

    let bad_asset = "../escape.png";
    let ast = decode(&serde_json::json!({ "kind": "image", "path": bad_asset }));
    assert!(ast.is_ok(), "sandbox enforcement happens in the renderer");

    let _diff = DiffDecoration::LineGutter {
        file: "src/lib.rs".into(),
        line: 1,
        glyph: "!".into(),
        color: None,
    };
}

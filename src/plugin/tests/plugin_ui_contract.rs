use crate::core::TabId;
use crate::plugin::slots::Container;
use crate::plugin::tab_snapshot::{TabSnapshotEntry, TabsSnapshot};
use crate::plugin::tests::harness::MockHost;
use crate::widgets::chrome::main_bar::{builtins as main_bar_builtins, MainBarRegistry};

const SLOT_CAPS: &str = r#"["ui:region:main_bar", "ui:region:repository"]"#;
const ALL_SLOT_CAPS: &str =
    r#"["ui:region:main_bar", "ui:region:tab_bar", "ui:region:repository"]"#;

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

fn main_bar_registry(host: &MockHost) -> MainBarRegistry {
    let mut registry = MainBarRegistry::new();
    main_bar_builtins::register_all(&mut registry);
    host.host().apply_main_bar_slots(&mut registry);
    registry
}

fn tab_snapshot() -> TabsSnapshot {
    TabsSnapshot {
        tabs: vec![TabSnapshotEntry {
            id: TabId(7),
            path: "/tmp/repo".into(),
            name: "repo".into(),
        }],
        active_id: Some(TabId(7)),
        active_path: Some("/tmp/repo".into()),
    }
}

#[test]
fn ui_slot_handle_add_replace_remove_flow() {
    let mut host = MockHost::new();
    host.load_inline(
        "ui_handle",
        &manifest("ui_handle", SLOT_CAPS),
        r#"
        local handle, err = leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.ui.handle",
            priority = 10, widget = { kind = "text", value = "add" },
        }
        assert(handle, err)
        local replaced, replace_err = handle:replace{
            priority = 20, widget = { kind = "text", value = "replace" },
        }
        assert(replaced, replace_err)
        local removed, remove_err = handle:remove()
        assert(removed, remove_err)
        local again, again_err = handle:remove()
        _G.invalid_after_remove = again == nil and type(again_err) == "string" and 1 or 0
        "#,
    )
    .expect("UI handle flow");

    assert_eq!(
        host.read_global_i64("ui_handle", "invalid_after_remove"),
        Some(1)
    );
    let registry = main_bar_registry(&host);
    assert!(!registry.contains_display_id("plugin.ui.handle"));
}

#[test]
fn ui_direct_replace_renders_replacement() {
    let mut host = MockHost::new();
    host.load_inline(
        "ui_replace",
        &manifest("ui_replace", SLOT_CAPS),
        r#"
        local handle, err = leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.ui.replace",
            priority = 10, widget = { kind = "text", value = "add" },
        }
        assert(handle, err)
        local replaced, replace_err = leviathan.ui.slot.replace(handle.address, {
            priority = 30, widget = { kind = "text", value = "replace" },
        })
        assert(replaced, replace_err)
        "#,
    )
    .expect("UI replace");

    let registry = main_bar_registry(&host);
    let slot = registry
        .iter_container(Container::Section("left".into()))
        .find(|slot| slot.id == "plugin.ui.replace")
        .expect("slot");
    assert_eq!(slot.priority, 30);
}

#[test]
fn ui_region_describe_context_and_feature_gates_work() {
    let mut host = MockHost::new();
    host.load_inline(
        "ui_context",
        &manifest("ui_context", "[]"),
        r#"
        _G.has_slot = leviathan.has("ui.slot@1") and 1 or 0
        _G.has_context = leviathan.has("ui.context@1") and 1 or 0
        local regions, regions_err = leviathan.ui.region.list()
        assert(regions, regions_err)
        _G.region_count = #regions
        local desc, desc_err = leviathan.ui.region.describe("repository")
        assert(desc, desc_err)
        _G.repo_panes = #desc.panes
        local missing, missing_err = leviathan.ui.region.describe("nope")
        _G.missing_region = missing == nil and type(missing_err) == "string" and 1 or 0
        local ctx, ctx_err = leviathan.ui.context.current()
        assert(ctx, ctx_err)
        _G.ctx_generation = ctx.generation_id
        _G.ctx_plugin = ctx.plugin_id
        _G.ctx_type = ctx.type
        _G.ctx_surface = ctx.surface
        "#,
    )
    .expect("UI context");

    assert_eq!(host.read_global_i64("ui_context", "has_slot"), Some(1));
    assert_eq!(host.read_global_i64("ui_context", "has_context"), Some(1));
    assert_eq!(host.read_global_i64("ui_context", "region_count"), Some(3));
    assert_eq!(host.read_global_i64("ui_context", "repo_panes"), Some(3));
    assert_eq!(
        host.read_global_i64("ui_context", "missing_region"),
        Some(1)
    );
    assert_eq!(
        host.read_global_i64("ui_context", "ctx_generation"),
        Some(1)
    );
    assert_eq!(
        host.read_global_string("ui_context", "ctx_plugin")
            .as_deref(),
        Some("ui_context")
    );
    assert_eq!(
        host.read_global_string("ui_context", "ctx_type").as_deref(),
        Some("ScreenContext")
    );
    assert_eq!(
        host.read_global_string("ui_context", "ctx_surface")
            .as_deref(),
        Some("screen")
    );
}

#[test]
fn ui_dynamic_widgets_receive_typed_context_for_mounted_regions() {
    let mut host = MockHost::new();
    host.load_inline(
        "ui_ctx_slots",
        &manifest("ui_ctx_slots", ALL_SLOT_CAPS),
        r#"
        local function widget(label)
            return function(ctx)
                local cur = leviathan.ui.context.current()
                _G[label] = table.concat({
                    ctx.type,
                    ctx.surface,
                    ctx.focus.region or "",
                    ctx.focus.pane or "",
                    ctx.focus.section or "",
                    ctx.repository.current_branch_name or "",
                    ctx.tab.name or "",
                    cur.type,
                    cur.surface,
                    tostring(ctx.features["ui.context.typed@1"] == true),
                    ctx.theme.colors.text_primary or "",
                }, "|")
                return { kind = "text", value = label }
            end
        end

        assert(leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.ctx.main",
            priority = 1, widget = widget("main_ctx"),
        })
        assert(leviathan.ui.slot.add{
            region = "tab_bar", section = "right", id = "plugin.ctx.tab",
            priority = 1, widget = widget("tab_ctx"),
        })
        assert(leviathan.ui.slot.add{
            region = "repository", pane = "sidebar", section = "top", id = "plugin.ctx.sidebar",
            priority = 1, widget = widget("sidebar_ctx"),
        })
        assert(leviathan.ui.slot.add{
            region = "repository", pane = "graph", section = "bottom", id = "plugin.ctx.graph",
            priority = 1, widget = widget("graph_ctx"),
        })
        assert(leviathan.ui.slot.add{
            region = "repository", pane = "details", section = "top", id = "plugin.ctx.details",
            priority = 1, widget = widget("details_ctx"),
        })
        "#,
    )
    .expect("current typed context slots");

    let tabs = tab_snapshot();
    host.host_mut().sync_tab_registry(&tabs);
    host.host_mut()
        .sync_repository("repo", "/tmp/repo", "main", "abc123", "origin", &[]);

    let expected = [
        (
            "main_ctx",
            "MainBarContext|main_bar|main_bar||left|main|repo|MainBarContext|main_bar|true|#e1e5f4",
        ),
        (
            "tab_ctx",
            "TabBarContext|tab_bar|tab_bar||right|main|repo|TabBarContext|tab_bar|true|#e1e5f4",
        ),
        (
            "sidebar_ctx",
            "RepositorySidebarContext|repository.sidebar|repository|sidebar|top|main|repo|RepositorySidebarContext|repository.sidebar|true|#e1e5f4",
        ),
        (
            "graph_ctx",
            "RepositoryGraphContext|repository.graph|repository|graph|bottom|main|repo|RepositoryGraphContext|repository.graph|true|#e1e5f4",
        ),
        (
            "details_ctx",
            "RepositoryDetailsContext|repository.details|repository|details|top|main|repo|RepositoryDetailsContext|repository.details|true|#e1e5f4",
        ),
    ];
    for (global, value) in expected {
        assert_eq!(
            host.read_global_string("ui_ctx_slots", global).as_deref(),
            Some(value),
            "{global}"
        );
    }
}

#[test]
fn raw_colors_in_native_chrome_emit_warning_without_style_cap() {
    let mut host = MockHost::new();
    host.load_inline(
        "ui_raw_color",
        &manifest("ui_raw_color", r#"["ui:region:main_bar"]"#),
        r##"
        local handle, err = leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.ui.raw_color",
            priority = 10,
            widget = { kind = "text", value = "raw", color = "#ffffff" },
        }
        assert(handle, err)
        "##,
    )
    .expect("raw color slot loads");

    let warnings = host.diagnostics().by_code("widget.raw_color_chrome");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].context["slot_id"], "plugin.ui.raw_color");
}

#[test]
fn ui_recoverable_errors_return_err_without_failing_load() {
    let mut host = MockHost::new();
    host.load_inline(
        "ui_errors",
        &manifest("ui_errors", "[]"),
        r#"
        local denied, denied_err = leviathan.ui.slot.add{
            region = "main_bar", section = "left", id = "plugin.ui.denied",
            priority = 1, widget = { kind = "text", value = "x" },
        }
        _G.denied_nil = denied == nil and type(denied_err) == "string" and 1 or 0
        _G.denied_mentions_cap = string.find(denied_err or "", "ui:region:main_bar", 1, true) and 1 or 0
        local bad, bad_err = leviathan.ui.slot.add("not a table")
        _G.bad_arg_nil = bad == nil and type(bad_err) == "string" and 1 or 0
        "#,
    )
    .expect("UI recoverable errors should not abort init");

    assert_eq!(host.read_global_i64("ui_errors", "denied_nil"), Some(1));
    assert_eq!(
        host.read_global_i64("ui_errors", "denied_mentions_cap"),
        Some(1)
    );
    assert_eq!(host.read_global_i64("ui_errors", "bad_arg_nil"), Some(1));
}

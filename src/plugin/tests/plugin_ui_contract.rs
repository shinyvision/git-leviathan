use crate::core::TabId;
use crate::plugin::commit_data::{CommitActionAvailability, CommitActions, CommitData};
use crate::plugin::host::RepositorySyncState;
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
fn lua_command_bodies_see_active_selection_context() {
    let mut host = MockHost::new();
    host.load_inline(
        "cmd_ctx",
        &manifest("cmd_ctx", "[]"),
        r#"
        leviathan.command.create("cmd_ctx.capture", {
            run = function()
                local ctx, err = leviathan.ui.context.current()
                assert(ctx, err)
                _G.ctx_kind = ctx.selection.kind
                _G.ctx_commit = ctx.selection.commit and ctx.selection.commit.hash or ""
            end,
        })
        "#,
    )
    .expect("command context plugin loads");

    host.host_mut()
        .sync_selection(crate::plugin::ui::context::SelectionContextSnapshot {
            available: true,
            kind: "commit".to_string(),
            commit: Some(CommitData {
                kind: "commit".to_string(),
                hash: "abc123".to_string(),
                short_hash: "abc123".to_string(),
                summary: "summary".to_string(),
                message: "summary".to_string(),
                author: "Author".to_string(),
                date: String::new(),
                timestamp: Some(0),
                parents: Vec::new(),
                index: Some(0),
                is_merge: false,
                is_merge_in_progress: false,
                actions: CommitActions::with_reword(CommitActionAvailability::enabled()),
            }),
            selected_file_path: None,
        });

    let outcome = host.invoke_command("cmd_ctx.capture", serde_json::json!({}));
    assert!(matches!(
        outcome,
        crate::plugin::commands::InvokeOutcome::Ok
    ));
    assert_eq!(
        host.read_global_string("cmd_ctx", "ctx_kind").as_deref(),
        Some("commit")
    );
    assert_eq!(
        host.read_global_string("cmd_ctx", "ctx_commit").as_deref(),
        Some("abc123")
    );
}

#[test]
fn overlay_scrollable_scroll_y_queues_ui_scroll_effect() {
    let mut host = MockHost::new();
    host.load_inline(
        "overlay_scroll",
        &manifest("overlay_scroll", r#"["ui:overlay"]"#),
        r##"
        leviathan.ui.overlay({
            id = "picker",
            widget = {
                kind = "scrollable",
                id = "list",
                scroll_y = 84,
                child = { kind = "text", value = "body" },
            },
        })
        "##,
    )
    .expect("overlay scroll plugin loads");

    let requests = host.host_mut().take_pending_ui_scrolls();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].id,
        crate::plugin::ui::effects::plugin_scrollable_id(
            "overlay_scroll",
            "overlay:picker",
            "list"
        )
    );
    assert_eq!(requests[0].y, 84.0);
}

#[test]
fn lua_dialog_queues_repository_dialog_request() {
    let mut host = MockHost::new();
    host.load_inline(
        "dialog_request",
        &manifest("dialog_request", r#"["ui:overlay"]"#),
        r#"
        leviathan.ui.dialog({
            id = "confirm",
            title = "Confirm",
            text = "Continue?",
            dismissible = false,
            autofocus = "name",
            data = {
                { id = "branch", value = "main" },
            },
            controls = {
                {
                    id = "name",
                    label = { text = "Name", style = "secondary" },
                    text_input = { placeholder = "Branch", value = "feature" },
                },
            },
            buttons = {
                {
                    id = "confirm",
                    text = "Continue",
                    style = "green",
                    keys = { "Y", "Return" },
                    closes_dialog = false,
                    enabled = false,
                    on_click = function() end,
                },
                {
                    id = "cancel",
                    text = "Cancel",
                    style = "white",
                    keys = { "Esc" },
                    on_click = function() end,
                },
            },
        })
        "#,
    )
    .expect("dialog plugin loads");

    let effects = host.host_mut().take_pending_ui_effects();
    assert_eq!(effects.len(), 1);
    let crate::plugin::ui::effects::PluginUiEffect::OpenRepositoryDialog(request) = &effects[0]
    else {
        panic!("expected repository dialog request");
    };
    assert_eq!(request.plugin_id, "dialog_request");
    assert_eq!(request.dialog_id, "confirm");
    assert_eq!(request.title.as_deref(), Some("Confirm"));
    assert_eq!(request.text, "Continue?");
    assert!(!request.dismissible);
    assert_eq!(request.autofocus.as_deref(), Some("name"));
    assert_eq!(request.data[0].id, "branch");
    assert_eq!(request.controls[0].id, "name");
    assert_eq!(request.buttons[0].style, "green");
    assert_eq!(request.buttons[0].keys, vec!["y", "enter"]);
    assert!(!request.buttons[0].closes_dialog);
    assert!(!request.buttons[0].enabled);
    assert_eq!(request.buttons[1].keys, vec!["esc"]);
    assert!(request.buttons[1].closes_dialog);
    assert!(request.buttons[1].enabled);
}

#[test]
fn lua_dialog_focus_and_press_queue_repository_dialog_effects() {
    let mut host = MockHost::new();
    host.load_inline(
        "dialog_actions",
        &manifest("dialog_actions", r#"["ui:overlay"]"#),
        r#"
        local focus_ok, focus_err = leviathan.ui.dialog.focus_control("native.conflict_checkout", "branch_name")
        assert(focus_ok, focus_err)
        local press_ok, press_err = leviathan.ui.dialog.press_button("native.conflict_checkout", "reset")
        assert(press_ok, press_err)
        "#,
    )
    .expect("dialog action plugin loads");

    let effects = host.host_mut().take_pending_ui_effects();
    assert_eq!(effects.len(), 2);
    assert!(matches!(
        &effects[0],
        crate::plugin::ui::effects::PluginUiEffect::FocusRepositoryDialogControl {
            dialog_id,
            control_id,
        } if dialog_id == "native.conflict_checkout" && control_id == "branch_name"
    ));
    assert!(matches!(
        &effects[1],
        crate::plugin::ui::effects::PluginUiEffect::PressRepositoryDialogButton {
            dialog_id,
            button_id,
        } if dialog_id == "native.conflict_checkout" && button_id == "reset"
    ));
}

#[test]
fn command_context_includes_active_toolbar_dialog() {
    let mut host = MockHost::new();
    host.load_inline(
        "dialog_context",
        &manifest("dialog_context", r#"["ui:overlay"]"#),
        r#"
        leviathan.command.create("dialog_context.capture", {
            title = "Capture",
            context = "repository.graph",
            run = function()
                local ctx = assert(leviathan.ui.context.current())
                _G.dialog_id = ctx.dialog.id
                _G.control_value = ctx.dialog.controls[1].value
                _G.reset_enabled = ctx.dialog.buttons[2].enabled and "yes" or "no"
            end,
        })
        "#,
    )
    .expect("dialog context plugin loads");

    host.host_mut()
        .sync_toolbar_dialog(crate::plugin::ui::context::ToolbarDialogContextSnapshot {
            active: true,
            id: "native.conflict_checkout".into(),
            owner: "native".into(),
            plugin_id: None,
            data: Vec::new(),
            controls: vec![
                crate::plugin::ui::context::ToolbarDialogControlContextSnapshot {
                    id: "branch_name".into(),
                    kind: "text_input".into(),
                    value: Some("topic".into()),
                },
            ],
            buttons: vec![
                crate::plugin::ui::context::ToolbarDialogButtonContextSnapshot {
                    id: "create".into(),
                    text: "Create Branch Here".into(),
                    enabled: true,
                },
                crate::plugin::ui::context::ToolbarDialogButtonContextSnapshot {
                    id: "reset".into(),
                    text: "Reset Local to Here".into(),
                    enabled: true,
                },
            ],
        });

    assert!(host
        .invoke_command("dialog_context.capture", serde_json::Value::Null)
        .is_ok());
    assert_eq!(
        host.read_global_string("dialog_context", "dialog_id")
            .as_deref(),
        Some("native.conflict_checkout")
    );
    assert_eq!(
        host.read_global_string("dialog_context", "control_value")
            .as_deref(),
        Some("topic")
    );
    assert_eq!(
        host.read_global_string("dialog_context", "reset_enabled")
            .as_deref(),
        Some("yes")
    );
}

#[test]
fn lua_dialog_does_not_register_overlay_record() {
    let mut host = MockHost::new();
    host.load_inline(
        "dialog_no_overlay",
        &manifest("dialog_no_overlay", r#"["ui:overlay"]"#),
        r#"
        leviathan.ui.dialog({
            id = "confirm",
            text = "Continue?",
            buttons = {
                {
                    id = "ok",
                    text = "OK",
                    style = "blue",
                    on_click = function() end,
                },
            },
        })
        "#,
    )
    .expect("dialog plugin loads");

    assert!(host
        .introspect()
        .overlays
        .iter()
        .all(|overlay| overlay.plugin_id != "dialog_no_overlay"));
}

#[test]
fn lua_dialog_rejects_invalid_button_style_and_key() {
    let mut bad_style = MockHost::new();
    let style_result = bad_style.load_inline(
        "bad_style",
        &manifest("bad_style", r#"["ui:overlay"]"#),
        r#"
        leviathan.ui.dialog({
            id = "confirm",
            text = "Continue?",
            buttons = {
                { id = "ok", text = "OK", style = "purple" },
            },
        })
        "#,
    );
    assert!(style_result.is_err());

    let mut bad_key = MockHost::new();
    let key_result = bad_key.load_inline(
        "bad_key",
        &manifest("bad_key", r#"["ui:overlay"]"#),
        r#"
        leviathan.ui.dialog({
            id = "confirm",
            text = "Continue?",
            buttons = {
                {
                    id = "ok",
                    text = "OK",
                    style = "blue",
                    keys = { "Ctrl+K" },
                    on_click = function() end,
                },
            },
        })
        "#,
    );
    assert!(key_result.is_err());
}

#[test]
fn lua_dialog_stores_button_callback() {
    let mut host = MockHost::new();
    host.load_inline(
        "dialog_callback",
        &manifest("dialog_callback", r#"["ui:overlay"]"#),
        r#"
        leviathan.ui.dialog({
            id = "confirm",
            text = "Continue?",
            buttons = {
                {
                    id = "ok",
                    text = "OK",
                    style = "green",
                    on_click = function() _G.clicked = 1 end,
                },
            },
        })
        "#,
    )
    .expect("dialog plugin loads");

    assert!(host
        .host()
        .has_dialog_callback("dialog_callback", "confirm", "ok"));
}

#[test]
fn dialog_button_callback_receives_button_id_argument() {
    let mut host = MockHost::new();
    host.load_inline(
        "dialog_callback_arg",
        &manifest("dialog_callback_arg", r#"["ui:overlay"]"#),
        r#"
        leviathan.ui.dialog({
            id = "confirm",
            text = "Continue?",
            buttons = {
                {
                    id = "ok",
                    text = "OK",
                    style = "green",
                    on_click = function(button_id) _G.clicked_button = button_id end,
                },
            },
        })
        "#,
    )
    .expect("dialog plugin loads");

    host.host_mut()
        .dispatch_dialog_button_callback("dialog_callback_arg", "confirm", "ok");

    assert_eq!(
        host.read_global_string("dialog_callback_arg", "clicked_button")
            .as_deref(),
        Some("ok")
    );
}

#[test]
fn dialog_button_callback_error_records_diagnostic() {
    let mut host = MockHost::new();
    host.load_inline(
        "dialog_callback_error",
        &manifest("dialog_callback_error", r#"["ui:overlay"]"#),
        r#"
        leviathan.ui.dialog({
            id = "confirm",
            text = "Continue?",
            buttons = {
                {
                    id = "ok",
                    text = "OK",
                    style = "green",
                    on_click = function() error("boom") end,
                },
            },
        })
        "#,
    )
    .expect("dialog plugin loads");

    host.host_mut()
        .dispatch_dialog_button_callback("dialog_callback_error", "confirm", "ok");

    let diagnostics = host.diagnostics().by_code("lua.callback_error");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].plugin_id.as_str(), "dialog_callback_error");
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
    host.host_mut().sync_repository(RepositorySyncState {
        repo_name: "repo",
        workdir_path: "/tmp/repo",
        current_branch_name: "main",
        head_hash: "abc123",
        default_remote_name: "origin",
        remote_names: &[],
        refs: &[],
    });

    let expected = [
        (
            "main_ctx",
            "MainBarContext|main_bar||||main|repo|MainBarContext|main_bar|true|#e1e5f4",
        ),
        (
            "tab_ctx",
            "TabBarContext|tab_bar||||main|repo|TabBarContext|tab_bar|true|#e1e5f4",
        ),
        (
            "sidebar_ctx",
            "RepositorySidebarContext|repository.sidebar||||main|repo|RepositorySidebarContext|repository.sidebar|true|#e1e5f4",
        ),
        (
            "graph_ctx",
            "RepositoryGraphContext|repository.graph||||main|repo|RepositoryGraphContext|repository.graph|true|#e1e5f4",
        ),
        (
            "details_ctx",
            "RepositoryDetailsContext|repository.details||||main|repo|RepositoryDetailsContext|repository.details|true|#e1e5f4",
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

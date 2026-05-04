use std::cell::RefCell;
use std::rc::Rc;

use git_leviathan_plugin_api::descriptor::decoration::{DiffDecoration, GraphDecoration};
use git_leviathan_plugin_api::descriptor::extension_point::extension_point;
use git_leviathan_plugin_api::descriptor::region::REGIONS;
use mlua::{Function, Lua, LuaSerdeExt, Table, Value as LuaValue};

use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::extensions::{
    normalize_overlay_key_event, ContextMenuItemRecord, DecorationProviderCallbacks,
    DecorationProviderRecord, DecorationProviderTarget, DiffDecorationRecord, ExtensionRegistry,
    GraphDecorationRecord, OverlayCallbacks, OverlayRecord,
};
use crate::plugin::host::region_mount::REGION_MOUNT_REGISTRY;
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};
use crate::plugin::ui::widget_ast;

/// Mount the four extension points functions onto the existing `leviathan.ui`
/// table.
///
/// `ui` must be the table the host installed under `leviathan.ui`
/// — we extend it in-place rather than allocating a new table.
pub fn install(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
    overlay_callbacks: Rc<RefCell<OverlayCallbacks>>,
    decoration_provider_callbacks: Rc<RefCell<DecorationProviderCallbacks>>,
) -> mlua::Result<()> {
    install_contribute(
        lua,
        ui,
        ledger.clone(),
        Rc::clone(&guard),
        registry.clone(),
        Rc::clone(&overlay_callbacks),
        Rc::clone(&decoration_provider_callbacks),
    )?;
    install_overlay(
        lua,
        ui,
        ledger.clone(),
        Rc::clone(&guard),
        registry.clone(),
        Rc::clone(&overlay_callbacks),
    )?;
    install_remove_overlay(
        lua,
        ui,
        ledger.clone(),
        Rc::clone(&guard),
        registry.clone(),
        overlay_callbacks,
    )?;
    install_context_menu(lua, ui, ledger.clone(), Rc::clone(&guard), registry.clone())?;
    install_graph_decoration(lua, ui, ledger.clone(), Rc::clone(&guard), registry.clone())?;
    install_diff_decoration(lua, ui, ledger, guard, registry)?;
    Ok(())
}

#[derive(Clone)]
struct ContributionApiState {
    plugin_id: String,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
    overlay_callbacks: Rc<RefCell<OverlayCallbacks>>,
    decoration_provider_callbacks: Rc<RefCell<DecorationProviderCallbacks>>,
}

#[derive(Clone)]
enum ContributionRemoveTarget {
    Overlay,
    ContextMenu,
    GraphStatic { commit_hash: String },
    DiffStatic,
    DecorationProvider { target: DecorationProviderTarget },
}

#[derive(Clone)]
struct ContributionHandleData {
    point_id: String,
    id: String,
    handle: String,
    resource_kind: PluginResourceKind,
    remove_target: ContributionRemoveTarget,
}

fn install_contribute(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
    overlay_callbacks: Rc<RefCell<OverlayCallbacks>>,
    decoration_provider_callbacks: Rc<RefCell<DecorationProviderCallbacks>>,
) -> mlua::Result<()> {
    let state = ContributionApiState {
        plugin_id: ledger.plugin_id().as_str().to_string(),
        ledger,
        guard,
        registry,
        overlay_callbacks,
        decoration_provider_callbacks,
    };
    ui.set(
        "contribute",
        lua.create_function(
            move |lua_inner, (point_id, spec): (String, Table)| match contribute(
                lua_inner, &state, &point_id, spec,
            ) {
                Ok(handle) => Ok((Some(handle), None::<String>)),
                Err(err) => Ok((None::<Table>, Some(err))),
            },
        )?,
    )?;
    Ok(())
}

fn contribute(
    lua: &Lua,
    state: &ContributionApiState,
    point_id: &str,
    spec: Table,
) -> Result<Table, String> {
    extension_point(point_id).ok_or_else(|| format!("unknown extension point: {point_id}"))?;
    match point_id {
        "repository.diff.context_menu" | "repository.graph.context_menu" => {
            add_context_menu_contribution(lua, state, point_id, spec)
        }
        "repository.graph.row_badge" => add_graph_contribution(lua, state, point_id, spec),
        "repository.diff.line_gutter" => add_diff_contribution(lua, state, point_id, spec),
        "overlays" => add_overlay_contribution(lua, state, spec),
        _ => Err(format!(
            "extension point `{point_id}` does not accept ui.contribute yet"
        )),
    }
}

fn add_overlay_contribution(
    lua: &Lua,
    state: &ContributionApiState,
    spec: Table,
) -> Result<Table, String> {
    let id: String = spec.get("id").map_err(|e| e.to_string())?;
    let source = ResourceLedger::source_location(lua);
    state
        .guard
        .check_named_for_target(
            "ui:overlay",
            &format!("overlay:{id}"),
            "leviathan.ui.contribute",
            source.as_deref(),
            "Declare and grant `ui:overlay`.",
        )
        .map_err(|e| e.to_string())?;
    let dismissible: bool = spec
        .get::<Option<bool>>("dismissible")
        .map_err(|e| e.to_string())?
        .unwrap_or(true);
    let priority: i32 = spec
        .get::<Option<i32>>("priority")
        .map_err(|e| e.to_string())?
        .unwrap_or(0);
    let key_events = read_overlay_key_events(&spec).map_err(|e| e.to_string())?;
    let widget_value: LuaValue = spec.get("widget").map_err(|e| e.to_string())?;
    let widget_json: serde_json::Value = lua
        .from_value(widget_value)
        .map_err(|e| format!("invalid overlay widget: {e}"))?;
    let widget =
        widget_ast::decode(&widget_json).map_err(|e| format!("invalid overlay widget: {e}"))?;
    let handle = format!("overlay:{id}");
    state
        .ledger
        .remove_by_kind_handle(PluginResourceKind::Overlay, &handle);
    state
        .ledger
        .record(PluginResourceKind::Overlay, handle.clone(), source.clone());
    state.overlay_callbacks.borrow_mut().remove(&id);
    let callback: Option<Function> = match spec
        .get::<Option<Function>>("on_event")
        .map_err(|e| e.to_string())?
    {
        Some(callback) => Some(callback),
        None => spec
            .get::<Option<Function>>("update")
            .map_err(|e| e.to_string())?,
    };
    if let Some(callback) = callback {
        let key = lua
            .create_registry_value(callback)
            .map_err(|e| e.to_string())?;
        state.ledger.record(
            PluginResourceKind::LuaRegistryKey,
            format!("overlay:{id}:on_event"),
            source.clone(),
        );
        state.overlay_callbacks.borrow_mut().insert(id.clone(), key);
    }
    state.registry.add_overlay(OverlayRecord {
        plugin_id: state.plugin_id.clone(),
        id: id.clone(),
        priority,
        dismissible,
        key_events,
        widget,
        source_location: source,
    });
    make_contribution_handle(
        lua,
        state.clone(),
        ContributionHandleData {
            point_id: "overlays".into(),
            id,
            handle,
            resource_kind: PluginResourceKind::Overlay,
            remove_target: ContributionRemoveTarget::Overlay,
        },
    )
    .map_err(|e| e.to_string())
}

fn add_context_menu_contribution(
    lua: &Lua,
    state: &ContributionApiState,
    point_id: &str,
    item: Table,
) -> Result<Table, String> {
    validate_context_menu_region(point_id).map_err(|e| e.to_string())?;
    let id: String = item.get("id").map_err(|e| e.to_string())?;
    let source = ResourceLedger::source_location(lua);
    let specific_cap = format!("ui:context_menu:{point_id}");
    state
        .guard
        .check_any_named_for_target(
            &[specific_cap],
            &format!("context_menu:{point_id}:{id}"),
            "leviathan.ui.contribute",
            source.as_deref(),
            &format!("Declare and grant `ui:context_menu:{point_id}`."),
        )
        .map_err(|e| e.to_string())?;
    let label: String = item.get("label").map_err(|e| e.to_string())?;
    let command: String = item.get("command").map_err(|e| e.to_string())?;
    let priority: i32 = item
        .get::<Option<i32>>("priority")
        .map_err(|e| e.to_string())?
        .unwrap_or(0);
    let condition_capability: Option<String> = item
        .get("condition_capability")
        .map_err(|e| e.to_string())?;
    let handle = format!("context_menu:{point_id}:{id}");
    state
        .ledger
        .remove_by_kind_handle(PluginResourceKind::ContextMenuItem, &handle);
    state.ledger.record(
        PluginResourceKind::ContextMenuItem,
        handle.clone(),
        source.clone(),
    );
    state.registry.add_context_menu_item(ContextMenuItemRecord {
        plugin_id: state.plugin_id.clone(),
        region: point_id.to_string(),
        id: id.clone(),
        label,
        command,
        priority,
        condition_capability,
        source_location: source,
    });
    make_contribution_handle(
        lua,
        state.clone(),
        ContributionHandleData {
            point_id: point_id.into(),
            id,
            handle,
            resource_kind: PluginResourceKind::ContextMenuItem,
            remove_target: ContributionRemoveTarget::ContextMenu,
        },
    )
    .map_err(|e| e.to_string())
}

fn add_graph_contribution(
    lua: &Lua,
    state: &ContributionApiState,
    point_id: &str,
    spec: Table,
) -> Result<Table, String> {
    if spec
        .get::<Option<Function>>("provider")
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return add_decoration_provider(
            lua,
            state,
            point_id,
            spec,
            DecorationProviderTarget::GraphRowBadge,
        );
    }
    let commit_hash: String = spec.get("commit_hash").map_err(|e| e.to_string())?;
    if commit_hash.is_empty() {
        return Err("commit_hash must be a non-empty string".into());
    }
    check_decoration_capability(state, point_id, "graph", "leviathan.ui.contribute", lua)?;
    let explicit_id: String = spec.get("id").map_err(|e| e.to_string())?;
    let source = ResourceLedger::source_location(lua);
    let parsed = parse_graph_decoration(lua, &spec)?;
    if parsed.kind() != "badge" {
        return Err("repository.graph.row_badge requires a badge decoration".into());
    }
    let handle = format!("graph_decoration:{point_id}:{commit_hash}:{explicit_id}");
    state
        .ledger
        .remove_by_kind_handle(PluginResourceKind::GraphDecoration, &handle);
    state.ledger.record(
        PluginResourceKind::GraphDecoration,
        handle.clone(),
        source.clone(),
    );
    state.registry.add_graph_decoration(GraphDecorationRecord {
        plugin_id: state.plugin_id.clone(),
        id: explicit_id.clone(),
        commit_hash: commit_hash.clone(),
        decoration: parsed,
        source_location: source,
    });
    make_contribution_handle(
        lua,
        state.clone(),
        ContributionHandleData {
            point_id: point_id.into(),
            id: explicit_id,
            handle,
            resource_kind: PluginResourceKind::GraphDecoration,
            remove_target: ContributionRemoveTarget::GraphStatic { commit_hash },
        },
    )
    .map_err(|e| e.to_string())
}

fn add_diff_contribution(
    lua: &Lua,
    state: &ContributionApiState,
    point_id: &str,
    spec: Table,
) -> Result<Table, String> {
    if spec
        .get::<Option<Function>>("provider")
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return add_decoration_provider(
            lua,
            state,
            point_id,
            spec,
            DecorationProviderTarget::DiffLineGutter,
        );
    }
    check_decoration_capability(state, point_id, "diff", "leviathan.ui.contribute", lua)?;
    let explicit_id: String = spec.get("id").map_err(|e| e.to_string())?;
    let source = ResourceLedger::source_location(lua);
    let parsed = parse_diff_decoration(lua, &spec)?;
    if parsed.kind() != "line_gutter" {
        return Err("repository.diff.line_gutter requires a line_gutter decoration".into());
    }
    let handle = format!("diff_decoration:{point_id}:{explicit_id}");
    state
        .ledger
        .remove_by_kind_handle(PluginResourceKind::DiffDecoration, &handle);
    state.ledger.record(
        PluginResourceKind::DiffDecoration,
        handle.clone(),
        source.clone(),
    );
    state.registry.add_diff_decoration(DiffDecorationRecord {
        plugin_id: state.plugin_id.clone(),
        id: explicit_id.clone(),
        decoration: parsed,
        source_location: source,
    });
    make_contribution_handle(
        lua,
        state.clone(),
        ContributionHandleData {
            point_id: point_id.into(),
            id: explicit_id,
            handle,
            resource_kind: PluginResourceKind::DiffDecoration,
            remove_target: ContributionRemoveTarget::DiffStatic,
        },
    )
    .map_err(|e| e.to_string())
}

fn add_decoration_provider(
    lua: &Lua,
    state: &ContributionApiState,
    point_id: &str,
    spec: Table,
    target: DecorationProviderTarget,
) -> Result<Table, String> {
    let id: String = spec.get("id").map_err(|e| e.to_string())?;
    let provider: Function = spec.get("provider").map_err(|e| e.to_string())?;
    let family = match target {
        DecorationProviderTarget::GraphRowBadge => "graph",
        DecorationProviderTarget::DiffLineGutter => "diff",
    };
    check_decoration_capability(state, point_id, family, "leviathan.ui.contribute", lua)?;
    let source = ResourceLedger::source_location(lua);
    let key = lua
        .create_registry_value(provider)
        .map_err(|e| e.to_string())?;
    let handle = format!("{point_id}:{id}");
    state
        .ledger
        .remove_by_kind_handle(resource_kind_for_target(target), &handle);
    state.ledger.record(
        resource_kind_for_target(target),
        handle.clone(),
        source.clone(),
    );
    state.ledger.record(
        PluginResourceKind::LuaRegistryKey,
        format!("{handle}:provider"),
        source.clone(),
    );
    state
        .decoration_provider_callbacks
        .borrow_mut()
        .insert(handle.clone(), key);
    state
        .registry
        .add_decoration_provider(DecorationProviderRecord {
            plugin_id: state.plugin_id.clone(),
            id: id.clone(),
            point_id: point_id.into(),
            target,
            source_location: source,
        });
    make_contribution_handle(
        lua,
        state.clone(),
        ContributionHandleData {
            point_id: point_id.into(),
            id,
            handle,
            resource_kind: resource_kind_for_target(target),
            remove_target: ContributionRemoveTarget::DecorationProvider { target },
        },
    )
    .map_err(|e| e.to_string())
}

fn make_contribution_handle(
    lua: &Lua,
    state: ContributionApiState,
    data: ContributionHandleData,
) -> mlua::Result<Table> {
    let handle = lua.create_table()?;
    handle.set("plugin_id", state.plugin_id.clone())?;
    handle.set("point_id", data.point_id.clone())?;
    handle.set("id", data.id.clone())?;
    handle.set("handle", data.handle.clone())?;
    let remove_state = state.clone();
    let remove_data = data.clone();
    handle.set(
        "remove",
        lua.create_function(move |_, _self: Table| {
            match remove_contribution(&remove_state, &remove_data) {
                Ok(()) => Ok((Some(true), None::<String>)),
                Err(err) => Ok((None::<bool>, Some(err))),
            }
        })?,
    )?;
    Ok(handle)
}

fn remove_contribution(
    state: &ContributionApiState,
    data: &ContributionHandleData,
) -> Result<(), String> {
    if !state
        .ledger
        .contains_kind_handle(data.resource_kind, &data.handle)
    {
        return Err(format!("contribution handle is not live: {}", data.handle));
    }
    match &data.remove_target {
        ContributionRemoveTarget::Overlay => {
            state.registry.remove_overlay(&state.plugin_id, &data.id);
            state.overlay_callbacks.borrow_mut().remove(&data.id);
            state.ledger.remove_by_kind_handle(
                PluginResourceKind::LuaRegistryKey,
                &format!("overlay:{}:on_event", data.id),
            );
        }
        ContributionRemoveTarget::ContextMenu => {
            state
                .registry
                .remove_context_menu_item(&state.plugin_id, &data.point_id, &data.id);
        }
        ContributionRemoveTarget::GraphStatic { commit_hash } => {
            state
                .registry
                .remove_graph_decoration(&state.plugin_id, commit_hash, &data.id);
        }
        ContributionRemoveTarget::DiffStatic => {
            state
                .registry
                .remove_diff_decoration(&state.plugin_id, &data.id);
        }
        ContributionRemoveTarget::DecorationProvider { target } => {
            state
                .registry
                .remove_decoration_provider(&state.plugin_id, &data.point_id, &data.id);
            state
                .decoration_provider_callbacks
                .borrow_mut()
                .remove(&data.handle);
            state.ledger.remove_by_kind_handle(
                PluginResourceKind::LuaRegistryKey,
                &format!("{}:provider", data.handle),
            );
            let _ = target;
        }
    }
    state
        .ledger
        .remove_by_kind_handle(data.resource_kind, &data.handle);
    Ok(())
}

fn check_decoration_capability(
    state: &ContributionApiState,
    point_id: &str,
    family: &str,
    api_name: &str,
    lua: &Lua,
) -> Result<(), String> {
    let source = ResourceLedger::source_location(lua);
    let caps = match family {
        "graph" => vec!["ui:decoration:graph".to_string()],
        "diff" => vec!["ui:decoration:diff".to_string()],
        _ => return Err(format!("unknown decoration family: {family}")),
    };
    state
        .guard
        .check_any_named_for_target(
            &caps,
            point_id,
            api_name,
            source.as_deref(),
            &format!("Declare and grant `{}`.", caps[0]),
        )
        .map_err(|e| e.to_string())
}

fn resource_kind_for_target(target: DecorationProviderTarget) -> PluginResourceKind {
    match target {
        DecorationProviderTarget::GraphRowBadge => PluginResourceKind::GraphDecoration,
        DecorationProviderTarget::DiffLineGutter => PluginResourceKind::DiffDecoration,
    }
}

fn parse_graph_decoration(lua: &Lua, spec: &Table) -> Result<GraphDecoration, String> {
    let value = match spec
        .get::<Option<LuaValue>>("decoration")
        .map_err(|e| e.to_string())?
    {
        Some(v) => v,
        None => LuaValue::Table(spec.clone()),
    };
    let json: serde_json::Value = lua
        .from_value(value)
        .map_err(|e| format!("invalid graph decoration: {e}"))?;
    serde_json::from_value(json).map_err(|e| format!("invalid graph decoration: {e}"))
}

fn parse_diff_decoration(lua: &Lua, spec: &Table) -> Result<DiffDecoration, String> {
    let value = match spec
        .get::<Option<LuaValue>>("decoration")
        .map_err(|e| e.to_string())?
    {
        Some(v) => v,
        None => LuaValue::Table(spec.clone()),
    };
    let json: serde_json::Value = lua
        .from_value(value)
        .map_err(|e| format!("invalid diff decoration: {e}"))?;
    serde_json::from_value(json).map_err(|e| format!("invalid diff decoration: {e}"))
}

fn install_overlay(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
    overlay_callbacks: Rc<RefCell<OverlayCallbacks>>,
) -> mlua::Result<()> {
    let plugin_id = ledger.plugin_id().as_str().to_string();
    ui.set(
        "overlay",
        lua.create_function(move |lua_inner, spec: Table| {
            let id: String = spec.get("id")?;
            let source = ResourceLedger::source_location(lua_inner);
            guard
                .check_named_for_target(
                    "ui:overlay",
                    &format!("overlay:{id}"),
                    "leviathan.ui.overlay",
                    source.as_deref(),
                    "Declare and grant `ui:overlay`.",
                )
                .map_err(mlua::Error::external)?;
            let dismissible: bool = spec.get::<Option<bool>>("dismissible")?.unwrap_or(true);
            let priority: i32 = spec.get::<Option<i32>>("priority")?.unwrap_or(0);
            let key_events = read_overlay_key_events(&spec)?;
            let widget_value: LuaValue = spec.get("widget")?;
            let widget_json: serde_json::Value = lua_inner
                .from_value(widget_value)
                .map_err(|e| mlua::Error::external(format!("invalid overlay widget: {e}")))?;
            let widget = widget_ast::decode(&widget_json)
                .map_err(|e| mlua::Error::external(format!("invalid overlay widget: {e}")))?;
            let handle = format!("overlay:{id}");
            ledger.remove_by_kind_handle(PluginResourceKind::Overlay, &handle);
            ledger.record(PluginResourceKind::Overlay, handle, source.clone());
            overlay_callbacks.borrow_mut().remove(&id);
            let callback: Option<Function> = match spec.get::<Option<Function>>("on_event")? {
                Some(callback) => Some(callback),
                None => spec.get::<Option<Function>>("update")?,
            };
            if let Some(callback) = callback {
                let key = lua_inner.create_registry_value(callback)?;
                ledger.record(
                    PluginResourceKind::LuaRegistryKey,
                    format!("overlay:{id}:on_event"),
                    source.clone(),
                );
                overlay_callbacks.borrow_mut().insert(id.clone(), key);
            }
            registry.add_overlay(OverlayRecord {
                plugin_id: plugin_id.clone(),
                id,
                priority,
                dismissible,
                key_events,
                widget,
                source_location: source,
            });
            Ok(())
        })?,
    )?;
    Ok(())
}

fn install_remove_overlay(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
    overlay_callbacks: Rc<RefCell<OverlayCallbacks>>,
) -> mlua::Result<()> {
    let plugin_id = ledger.plugin_id().as_str().to_string();
    let remove = lua.create_function(move |lua_inner, id: String| {
        let source = ResourceLedger::source_location(lua_inner);
        guard
            .check_named_for_target(
                "ui:overlay",
                &format!("overlay:{id}"),
                "leviathan.ui.remove_overlay",
                source.as_deref(),
                "Declare and grant `ui:overlay`.",
            )
            .map_err(mlua::Error::external)?;
        registry.remove_overlay(&plugin_id, &id);
        overlay_callbacks.borrow_mut().remove(&id);
        ledger.remove_by_kind_handle(PluginResourceKind::Overlay, &format!("overlay:{id}"));
        ledger.remove_by_kind_handle(
            PluginResourceKind::LuaRegistryKey,
            &format!("overlay:{id}:on_event"),
        );
        Ok(())
    })?;
    ui.set("remove_overlay", remove)?;
    Ok(())
}

fn install_context_menu(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
) -> mlua::Result<()> {
    let plugin_id = ledger.plugin_id().as_str().to_string();
    ui.set(
        "context_menu",
        lua.create_function(move |lua_inner, (region, item): (String, Table)| {
            validate_context_menu_region(&region).map_err(mlua::Error::external)?;
            let id: String = item.get("id")?;
            let source = ResourceLedger::source_location(lua_inner);
            let specific_cap = format!("ui:context_menu:{region}");
            guard
                .check_any_named_for_target(
                    &[specific_cap],
                    &format!("context_menu:{region}:{id}"),
                    "leviathan.ui.context_menu",
                    source.as_deref(),
                    &format!("Declare and grant `ui:context_menu:{region}`."),
                )
                .map_err(mlua::Error::external)?;
            let label: String = item.get("label")?;
            let command: String = item.get("command")?;
            let priority: i32 = item.get::<Option<i32>>("priority")?.unwrap_or(0);
            let condition_capability: Option<String> = item.get("condition_capability")?;
            let handle = format!("context_menu:{region}:{id}");
            ledger.remove_by_handle_prefix(&handle);
            ledger.record(PluginResourceKind::ContextMenuItem, handle, source.clone());
            registry.add_context_menu_item(ContextMenuItemRecord {
                plugin_id: plugin_id.clone(),
                region,
                id,
                label,
                command,
                priority,
                condition_capability,
                source_location: source,
            });
            Ok(())
        })?,
    )?;
    Ok(())
}

fn install_graph_decoration(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
) -> mlua::Result<()> {
    let plugin_id = ledger.plugin_id().as_str().to_string();
    ui.set(
        "graph_decoration",
        lua.create_function(
            move |lua_inner, (commit_hash, decoration): (String, Table)| {
                if commit_hash.is_empty() {
                    return Err(mlua::Error::external(
                        "graph_decoration: commit_hash must be a non-empty string",
                    ));
                }
                let explicit_id: Option<String> = decoration.get("id")?;
                let source = ResourceLedger::source_location(lua_inner);
                let id_for_target = explicit_id.as_deref().unwrap_or("auto").to_string();
                guard
                    .check_any_named_for_target(
                        &["ui:decoration:graph".to_string()],
                        &format!("graph_decoration:{commit_hash}:{id_for_target}"),
                        "leviathan.ui.graph_decoration",
                        source.as_deref(),
                        "Declare and grant `ui:decoration:graph`.",
                    )
                    .map_err(mlua::Error::external)?;
                let value: serde_json::Value = lua_inner
                    .from_value(LuaValue::Table(decoration))
                    .map_err(|e| mlua::Error::external(format!("invalid graph decoration: {e}")))?;
                let parsed: GraphDecoration = serde_json::from_value(value)
                    .map_err(|e| mlua::Error::external(format!("invalid graph decoration: {e}")))?;
                let id = explicit_id.unwrap_or_else(|| format!("{}:{commit_hash}", parsed.kind()));
                let handle = format!("graph_decoration:{commit_hash}:{id}");
                ledger.remove_by_kind_handle(PluginResourceKind::GraphDecoration, &handle);
                ledger.record(PluginResourceKind::GraphDecoration, handle, source.clone());
                registry.add_graph_decoration(GraphDecorationRecord {
                    plugin_id: plugin_id.clone(),
                    id,
                    commit_hash,
                    decoration: parsed,
                    source_location: source,
                });
                Ok(())
            },
        )?,
    )?;
    Ok(())
}

fn install_diff_decoration(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
) -> mlua::Result<()> {
    let plugin_id = ledger.plugin_id().as_str().to_string();
    ui.set(
        "diff_decoration",
        lua.create_function(move |lua_inner, decoration: Table| {
            let explicit_id: Option<String> = decoration.get("id")?;
            let source = ResourceLedger::source_location(lua_inner);
            guard
                .check_any_named_for_target(
                    &["ui:decoration:diff".to_string()],
                    &format!(
                        "diff_decoration:{}",
                        explicit_id.as_deref().unwrap_or("auto")
                    ),
                    "leviathan.ui.diff_decoration",
                    source.as_deref(),
                    "Declare and grant `ui:decoration:diff`.",
                )
                .map_err(mlua::Error::external)?;
            let value: serde_json::Value = lua_inner
                .from_value(LuaValue::Table(decoration))
                .map_err(|e| mlua::Error::external(format!("invalid diff decoration: {e}")))?;
            let parsed: DiffDecoration = serde_json::from_value(value)
                .map_err(|e| mlua::Error::external(format!("invalid diff decoration: {e}")))?;
            let id = explicit_id.unwrap_or_else(|| diff_decoration_default_id(&parsed));
            let handle = format!("diff_decoration:{id}");
            ledger.remove_by_kind_handle(PluginResourceKind::DiffDecoration, &handle);
            ledger.record(PluginResourceKind::DiffDecoration, handle, source.clone());
            registry.add_diff_decoration(DiffDecorationRecord {
                plugin_id: plugin_id.clone(),
                id,
                decoration: parsed,
                source_location: source,
            });
            Ok(())
        })?,
    )?;
    Ok(())
}

fn diff_decoration_default_id(d: &DiffDecoration) -> String {
    match d {
        DiffDecoration::LineHint { file, line, .. } => format!("line_hint:{file}:{line}"),
        DiffDecoration::HunkBadge { hunk_id, .. } => format!("hunk_badge:{hunk_id}"),
        DiffDecoration::LineGutter { file, line, .. } => format!("line_gutter:{file}:{line}"),
    }
}

fn validate_context_menu_region(region: &str) -> Result<(), String> {
    if REGION_MOUNT_REGISTRY.is_virtual_context_menu_region(region) {
        return Ok(());
    }
    // Resolve descriptor-backed context menus by longest prefix.
    use git_leviathan_plugin_api::descriptor::region::RegionKind;
    for split in (1..=region.matches('.').count() + 1).rev() {
        let (head, tail) = nth_dot_split(region, split);
        let Some(descriptor) = REGIONS.get(head) else {
            continue;
        };
        let tail = tail
            .ok_or_else(|| format!("expected '<region>.<pane?>.context_menu' (got '{region}')"))?;
        return match &descriptor.kind {
            RegionKind::Chrome { sections, .. } => {
                if tail != "context_menu" {
                    return Err(format!(
                        "context_menu only valid on a *.context_menu section (got '{tail}')"
                    ));
                }
                if !sections.contains(&"context_menu") {
                    return Err(format!("region '{head}' has no 'context_menu' section"));
                }
                Ok(())
            }
            RegionKind::Content { panes } => {
                // Allow either `<region>.<pane>.context_menu` or, when
                // there's exactly one pane, `<region>.context_menu`.
                let (pane, section) = match tail.split_once('.') {
                    Some((p, s)) => (p, s),
                    None if panes.len() == 1 => (panes[0].name, tail),
                    None => {
                        return Err(format!(
                            "expected '{head}.<pane>.context_menu' (got '{region}')"
                        ))
                    }
                };
                if section != "context_menu" {
                    return Err(format!(
                        "context_menu only valid on a *.context_menu section (got '{section}')"
                    ));
                }
                descriptor.validate_address(Some(pane), Some(section))
            }
        };
    }
    Err(format!("unknown region in '{region}'"))
}

fn read_overlay_key_events(spec: &Table) -> mlua::Result<Vec<String>> {
    let Some(table) = spec.get::<Option<Table>>("key_events")? else {
        return Ok(Vec::new());
    };

    let mut keys = Vec::new();
    for raw in table.sequence_values::<String>() {
        let raw = raw?;
        let Some(normalized) = normalize_overlay_key_event(&raw) else {
            return Err(mlua::Error::external(format!(
                "overlay.key_events contains unsupported key `{raw}`"
            )));
        };
        if !keys.iter().any(|key| key == normalized) {
            keys.push(normalized.to_string());
        }
    }
    Ok(keys)
}

/// Split `s` at the n-th `.` from the start. `n=1` returns
/// `("repository", Some("diff.context_menu"))`. Returns
/// `(s, None)` when the index exceeds the dot count.
fn nth_dot_split(s: &str, n: usize) -> (&str, Option<&str>) {
    let mut indices = s
        .char_indices()
        .filter_map(|(i, c)| (c == '.').then_some(i));
    match indices.nth(n - 1) {
        Some(idx) => (&s[..idx], Some(&s[idx + 1..])),
        None => (s, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_context_menu_accepts_known_addresses() {
        assert!(validate_context_menu_region("repository.diff.context_menu").is_ok());
        assert!(validate_context_menu_region("repository.graph.context_menu").is_ok());
    }

    #[test]
    fn validate_context_menu_rejects_unknown() {
        let err = validate_context_menu_region("repository.diff.toolbar").unwrap_err();
        assert!(err.contains("context_menu"), "got: {err}");

        let err = validate_context_menu_region("nope.context_menu").unwrap_err();
        assert!(err.contains("unknown region"), "got: {err}");
    }
}

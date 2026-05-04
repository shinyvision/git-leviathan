use std::cell::RefCell;
use std::rc::Rc;

use git_leviathan_plugin_api::descriptor::region::{RegionKind, REGIONS};
use mlua::{Lua, LuaSerdeExt, Table, Value as LuaValue};

use super::factory::{compose_container, read_address_with_id, read_spec, slot_handle};
use super::{BuildState, DynamicWidgetCall, RawSlotOp, WidgetSource};
use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};
use crate::plugin::ui::widget_ast;

#[derive(Clone)]
struct SlotApiState {
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    diagnostics: DiagnosticStore,
}

#[derive(Clone)]
struct SlotHandleData {
    plugin_id: String,
    region: String,
    pane: Option<String>,
    section: String,
    id: String,
    handle: String,
}

pub fn install(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    diagnostics: DiagnosticStore,
    ui_context: crate::plugin::ui::context::UiContextStore,
    ui: &Table,
) -> mlua::Result<()> {
    let state = SlotApiState {
        build,
        ledger,
        guard,
        diagnostics,
    };
    let slot = lua.create_table()?;
    install_slot(lua, state.clone(), &slot)?;
    ui.set("slot", slot)?;

    let region = lua.create_table()?;
    install_region(lua, &region)?;
    ui.set("region", region)?;

    let context = lua.create_table()?;
    install_context(lua, ui_context, &context)?;
    ui.set("context", context)?;
    Ok(())
}

fn install_slot(lua: &Lua, state: SlotApiState, slot: &Table) -> mlua::Result<()> {
    let add_state = state.clone();
    slot.set(
        "add",
        lua.create_function(move |lua_inner, value: LuaValue| {
            match expect_table(value, "spec").and_then(|spec| add_slot(lua_inner, &add_state, spec))
            {
                Ok(handle) => Ok((Some(handle), None::<String>)),
                Err(err) => Ok((None::<Table>, Some(err))),
            }
        })?,
    )?;

    let remove_state = state.clone();
    slot.set(
        "remove",
        lua.create_function(move |lua_inner, value: LuaValue| {
            match expect_table(value, "address").and_then(|target| {
                let data = handle_data_from_address(&remove_state, &target)?;
                remove_slot(lua_inner, &remove_state, &data, false)
            }) {
                Ok(()) => Ok((Some(true), None::<String>)),
                Err(err) => Ok((None::<bool>, Some(err))),
            }
        })?,
    )?;

    let replace_state = state.clone();
    slot.set(
        "replace",
        lua.create_function(move |lua_inner, (target, spec): (LuaValue, LuaValue)| {
            match expect_table(target, "address").and_then(|address| {
                let data = handle_data_from_address(&replace_state, &address)?;
                expect_table(spec, "spec")
                    .and_then(|spec| replace_slot(lua_inner, &replace_state, &data, spec, false))?;
                make_handle(lua_inner, replace_state.clone(), data).map_err(|e| e.to_string())
            }) {
                Ok(handle) => Ok((Some(handle), None::<String>)),
                Err(err) => Ok((None::<Table>, Some(err))),
            }
        })?,
    )?;
    Ok(())
}

fn install_region(lua: &Lua, region: &Table) -> mlua::Result<()> {
    let names: Vec<String> = REGIONS.iter().map(|d| d.name.to_string()).collect();
    region.set(
        "list",
        lua.create_function(move |_, ()| Ok((names.clone(), None::<String>)))?,
    )?;
    region.set(
        "describe",
        lua.create_function(move |lua_inner, name: String| {
            let Some(desc) = REGIONS.get(&name) else {
                return Ok((None::<Table>, Some(format!("unknown region: {name}"))));
            };
            let table = lua_inner.create_table()?;
            table.set("name", desc.name)?;
            match &desc.kind {
                RegionKind::Chrome { sections, .. } => {
                    table.set("kind", "chrome")?;
                    table.set("sections", string_array(lua_inner, sections)?)?;
                }
                RegionKind::Content { panes } => {
                    table.set("kind", "content")?;
                    let pane_rows = lua_inner.create_table()?;
                    for (idx, pane) in panes.iter().enumerate() {
                        let pane_row = lua_inner.create_table()?;
                        pane_row.set("name", pane.name)?;
                        pane_row.set("sections", string_array(lua_inner, pane.sections)?)?;
                        pane_rows.set(idx + 1, pane_row)?;
                    }
                    table.set("panes", pane_rows)?;
                }
            }
            Ok((Some(table), None::<String>))
        })?,
    )?;
    Ok(())
}

fn install_context(
    lua: &Lua,
    ui_context: crate::plugin::ui::context::UiContextStore,
    context: &Table,
) -> mlua::Result<()> {
    context.set(
        "current",
        lua.create_function(move |lua_inner, ()| {
            let value = ui_context.get();
            let table = lua_inner.to_value(&value)?;
            Ok((Some(table), None::<String>))
        })?,
    )?;
    Ok(())
}

fn add_slot(lua: &Lua, state: &SlotApiState, spec: Table) -> Result<Table, String> {
    let region: String = spec.get("region").map_err(|e| e.to_string())?;
    let desc = REGIONS
        .get(&region)
        .ok_or_else(|| format!("unknown region: {region}"))?;
    let (pane, section, id) = read_address_with_id(&spec, desc).map_err(|e| e.to_string())?;
    if id.starts_with("builtin.") {
        return Err(format!(
            "slot id `{id}` is reserved; use leviathan.ui.slot.replace for built-ins"
        ));
    }
    let container = compose_container(pane.as_deref(), &section);
    let target = slot_target(desc.name, &container, &id);
    let source = ResourceLedger::source_location(lua);
    check_region_capability(
        &state.guard,
        desc.name,
        &target,
        "leviathan.ui.slot.add",
        source.as_deref(),
    )?;
    let handle = slot_handle(desc.name, &container, &id);
    let had_live = state
        .ledger
        .contains_kind_handle(PluginResourceKind::Slot, &handle);
    let raw = match read_spec(lua, &spec, desc, &state.ledger, DynamicWidgetCall::Context) {
        Ok(raw) => raw,
        Err(err) => {
            if !had_live {
                state
                    .ledger
                    .remove_by_kind_handle(PluginResourceKind::Slot, &handle);
            }
            return Err(err.to_string());
        }
    };
    warn_raw_colors_in_chrome(state, desc.name, &container, &id, &raw.widget);
    state.build.borrow_mut().slot_ops.push(RawSlotOp::Add(raw));
    make_handle(
        lua,
        state.clone(),
        SlotHandleData {
            plugin_id: state.ledger.plugin_id().to_string(),
            region: desc.name.to_string(),
            pane,
            section,
            handle,
            id,
        },
    )
    .map_err(|e| e.to_string())
}

fn remove_slot(
    lua: &Lua,
    state: &SlotApiState,
    data: &SlotHandleData,
    require_live: bool,
) -> Result<(), String> {
    if require_live
        && !state
            .ledger
            .contains_kind_handle(PluginResourceKind::Slot, &data.handle)
    {
        return Err(format!("slot handle is not live: {}", data.handle));
    }
    let desc = REGIONS
        .get(&data.region)
        .ok_or_else(|| format!("unknown region: {}", data.region))?;
    let container = compose_container(data.pane.as_deref(), &data.section);
    let target = slot_target(desc.name, &container, &data.id);
    let source = ResourceLedger::source_location(lua);
    check_region_capability(
        &state.guard,
        desc.name,
        &target,
        "leviathan.ui.slot.remove",
        source.as_deref(),
    )?;
    check_slot_mutation_capability(
        &state.guard,
        "remove",
        Some(&data.plugin_id),
        desc.name,
        &container,
        &data.id,
        &target,
        "leviathan.ui.slot.remove",
        source.as_deref(),
    )?;
    state
        .ledger
        .remove_by_kind_handle(PluginResourceKind::Slot, &data.handle);
    state.build.borrow_mut().slot_ops.push(RawSlotOp::Remove {
        target_plugin_id: Some(data.plugin_id.clone()),
        region: desc.name.to_string(),
        container,
        id: data.id.clone(),
        source_location: source,
    });
    Ok(())
}

fn replace_slot(
    lua: &Lua,
    state: &SlotApiState,
    data: &SlotHandleData,
    spec: Table,
    require_live: bool,
) -> Result<(), String> {
    if require_live
        && !state
            .ledger
            .contains_kind_handle(PluginResourceKind::Slot, &data.handle)
    {
        return Err(format!("slot handle is not live: {}", data.handle));
    }
    let desc = REGIONS
        .get(&data.region)
        .ok_or_else(|| format!("unknown region: {}", data.region))?;
    normalize_spec_address(&spec, data)?;
    let container = compose_container(data.pane.as_deref(), &data.section);
    let target = slot_target(desc.name, &container, &data.id);
    let source = ResourceLedger::source_location(lua);
    check_region_capability(
        &state.guard,
        desc.name,
        &target,
        "leviathan.ui.slot.replace",
        source.as_deref(),
    )?;
    check_slot_mutation_capability(
        &state.guard,
        "replace",
        Some(&data.plugin_id),
        desc.name,
        &container,
        &data.id,
        &target,
        "leviathan.ui.slot.replace",
        source.as_deref(),
    )?;
    let had_live = state
        .ledger
        .contains_kind_handle(PluginResourceKind::Slot, &data.handle);
    let raw = match read_spec(lua, &spec, desc, &state.ledger, DynamicWidgetCall::Context) {
        Ok(raw) => raw,
        Err(err) => {
            if !had_live {
                state
                    .ledger
                    .remove_by_kind_handle(PluginResourceKind::Slot, &data.handle);
            }
            return Err(err.to_string());
        }
    };
    warn_raw_colors_in_chrome(state, desc.name, &container, &data.id, &raw.widget);
    state.build.borrow_mut().slot_ops.push(RawSlotOp::Replace {
        target_plugin_id: Some(data.plugin_id.clone()),
        region: desc.name.to_string(),
        container,
        id: data.id.clone(),
        spec: raw,
        source_location: source,
    });
    Ok(())
}

fn make_handle(lua: &Lua, state: SlotApiState, data: SlotHandleData) -> mlua::Result<Table> {
    let handle = lua.create_table()?;
    handle.set("plugin_id", data.plugin_id.clone())?;
    handle.set("region", data.region.clone())?;
    if let Some(pane) = &data.pane {
        handle.set("pane", pane.clone())?;
    }
    handle.set("section", data.section.clone())?;
    handle.set("id", data.id.clone())?;
    handle.set("handle", data.handle.clone())?;
    handle.set("address", address_table(lua, &data)?)?;

    let remove_state = state.clone();
    let remove_data = data.clone();
    handle.set(
        "remove",
        lua.create_function(move |lua_inner, _self: Table| {
            match remove_slot(lua_inner, &remove_state, &remove_data, true) {
                Ok(()) => Ok((Some(true), None::<String>)),
                Err(err) => Ok((None::<bool>, Some(err))),
            }
        })?,
    )?;

    let replace_state = state.clone();
    let replace_data = data.clone();
    handle.set(
        "replace",
        lua.create_function(
            move |lua_inner, (this, value): (Table, LuaValue)| match expect_table(value, "spec")
                .and_then(|spec| replace_slot(lua_inner, &replace_state, &replace_data, spec, true))
            {
                Ok(()) => Ok((Some(this), None::<String>)),
                Err(err) => Ok((None::<Table>, Some(err))),
            },
        )?,
    )?;
    Ok(handle)
}

fn handle_data_from_address(
    state: &SlotApiState,
    address: &Table,
) -> Result<SlotHandleData, String> {
    let region: String = address.get("region").map_err(|e| e.to_string())?;
    let desc = REGIONS
        .get(&region)
        .ok_or_else(|| format!("unknown region: {region}"))?;
    let (pane, section, id) = read_address_with_id(address, desc).map_err(|e| e.to_string())?;
    let plugin_id: Option<String> = address.get("plugin_id").map_err(|e| e.to_string())?;
    let resolved =
        plugin_id.unwrap_or_else(|| default_target_plugin_id(state.guard.plugin_id(), &id));
    let container = compose_container(pane.as_deref(), &section);
    Ok(SlotHandleData {
        plugin_id: resolved,
        region: desc.name.to_string(),
        pane,
        section,
        id: id.clone(),
        handle: slot_handle(desc.name, &container, &id),
    })
}

fn normalize_spec_address(spec: &Table, data: &SlotHandleData) -> Result<(), String> {
    set_or_match(spec, "region", &data.region)?;
    if let Some(pane) = &data.pane {
        set_or_match(spec, "pane", pane)?;
    } else if let Ok(Some(pane)) = spec.get::<Option<String>>("pane") {
        return Err(format!("spec pane `{pane}` does not match target"));
    }
    set_or_match(spec, "section", &data.section)?;
    set_or_match(spec, "id", &data.id)
}

fn warn_raw_colors_in_chrome(
    state: &SlotApiState,
    region: &str,
    container: &str,
    id: &str,
    widget: &WidgetSource,
) {
    if !matches!(region, "main_bar" | "tab_bar" | "status_bar")
        || state.guard.requested("ui:style:raw_color")
    {
        return;
    }
    let WidgetSource::Static(ast) = widget else {
        return;
    };
    let paths = widget_ast::raw_color_paths(ast);
    if paths.is_empty() {
        return;
    }
    state.diagnostics.record(
        PluginDiagnostic::new(
            state.ledger.plugin_id(),
            DiagnosticSeverity::Warning,
            "widget.raw_color_chrome",
            "raw colors in native chrome should use theme tokens or declare ui:style:raw_color",
        )
        .with_generation(state.ledger.generation_id())
        .with_source(PluginSourceSpan::ApiFunction {
            name: "leviathan.ui.slot.add".into(),
        })
        .with_context(serde_json::json!({
            "region": region,
            "container": container,
            "slot_id": id,
            "paths": paths,
        })),
    );
}

fn set_or_match(spec: &Table, key: &str, expected: &str) -> Result<(), String> {
    match spec.get::<Option<String>>(key).map_err(|e| e.to_string())? {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(format!(
            "spec {key} `{actual}` does not match target `{expected}`"
        )),
        None => spec.set(key, expected).map_err(|e| e.to_string()),
    }
}

fn expect_table(value: LuaValue, name: &str) -> Result<Table, String> {
    match value {
        LuaValue::Table(table) => Ok(table),
        LuaValue::Nil => Err(format!("{name} must be a table")),
        other => Err(format!("{name} must be a table, got {}", other.type_name())),
    }
}

fn address_table(lua: &Lua, data: &SlotHandleData) -> mlua::Result<Table> {
    let address = lua.create_table()?;
    address.set("plugin_id", data.plugin_id.clone())?;
    address.set("region", data.region.clone())?;
    if let Some(pane) = &data.pane {
        address.set("pane", pane.clone())?;
    }
    address.set("section", data.section.clone())?;
    address.set("id", data.id.clone())?;
    Ok(address)
}

fn string_array(lua: &Lua, values: &[&'static str]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (idx, value) in values.iter().enumerate() {
        table.set(idx + 1, *value)?;
    }
    Ok(table)
}

fn check_region_capability(
    guard: &CapabilityGuard,
    region: &str,
    target: &str,
    api_name: &str,
    source_location: Option<&str>,
) -> Result<(), String> {
    guard
        .check_named_for_target(
            &format!("ui:region:{region}"),
            target,
            api_name,
            source_location,
            &format!("Declare and grant `ui:region:{region}`."),
        )
        .map_err(|e| e.to_string())
}

fn check_slot_mutation_capability(
    guard: &CapabilityGuard,
    op: &str,
    target_plugin_id: Option<&str>,
    region: &str,
    container: &str,
    id: &str,
    target: &str,
    api_name: &str,
    source_location: Option<&str>,
) -> Result<(), String> {
    let is_builtin = target_plugin_id == Some(crate::plugin::slots::BUILTIN_PLUGIN_ID)
        || (target_plugin_id.is_none() && id.starts_with("builtin."));
    let is_other_plugin = target_plugin_id
        .map(|owner| owner != guard.plugin_id() && owner != crate::plugin::slots::BUILTIN_PLUGIN_ID)
        .unwrap_or(false);
    if !is_builtin && !is_other_plugin {
        return Ok(());
    }
    let mut caps = Vec::new();
    if is_builtin {
        caps.push(format!("ui:{op}:builtin"));
    }
    caps.push(format!("ui:{op}:{region}:{container}:{id}"));
    guard
        .check_any_named_for_target(
            &caps,
            target,
            api_name,
            source_location,
            &format!("Declare and grant `{}`.", caps.join("` or `")),
        )
        .map_err(|e| e.to_string())
}

fn default_target_plugin_id(requester_plugin_id: &str, id: &str) -> String {
    if id.starts_with("builtin.") {
        crate::plugin::slots::BUILTIN_PLUGIN_ID.to_string()
    } else {
        requester_plugin_id.to_string()
    }
}

fn slot_target(region: &str, container: &str, id: &str) -> String {
    format!("{region}:{container}:{id}")
}

use std::cell::RefCell;
use std::rc::Rc;

use git_leviathan_plugin_api::descriptor::region::RegionDescriptor;
use mlua::{Function, Lua, LuaSerdeExt, Table, Value as LuaValue};

use crate::plugin::resources::{PluginResourceKind, ResourceLedger};
use crate::plugin::ui::widget_ast;

use super::{BuildState, RawSlotOp, RawSlotSpec, WidgetSource};

pub fn make_region_handle(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    desc: &'static RegionDescriptor,
) -> mlua::Result<Table> {
    let tbl = lua.create_table()?;

    let b = Rc::clone(&build);
    let ledger_for_add = ledger.clone();
    tbl.set(
        "add",
        lua.create_function(move |lua, spec: Table| {
            let raw = read_spec(lua, &spec, desc, &ledger_for_add)?;
            b.borrow_mut().slot_ops.push(RawSlotOp::Add(raw));
            Ok(())
        })?,
    )?;

    let b = Rc::clone(&build);
    let ledger_for_remove = ledger.clone();
    tbl.set(
        "remove",
        lua.create_function(move |_, args: Table| {
            let (pane, section, id) = read_address_with_id(&args, desc)?;
            let container = compose_container(pane.as_deref(), &section);
            let handle = slot_handle(desc.name, &container, &id);
            ledger_for_remove.remove_by_kind_handle(PluginResourceKind::Slot, &handle);
            b.borrow_mut().slot_ops.push(RawSlotOp::Remove {
                region: desc.name.to_string(),
                container,
                id,
            });
            Ok(())
        })?,
    )?;

    let b = Rc::clone(&build);
    let ledger_for_replace = ledger;
    tbl.set(
        "replace",
        lua.create_function(move |lua, (target, spec): (Table, Table)| {
            let (pane, section, id) = read_address_with_id(&target, desc)?;
            let container = compose_container(pane.as_deref(), &section);
            let mut raw = read_spec(lua, &spec, desc, &ledger_for_replace)?;
            if raw.container != container {
                return Err(mlua::Error::external(format!(
                    "{}.replace: spec container '{}' != target '{}'",
                    desc.name, raw.container, container
                )));
            }
            let spec_handle = slot_handle(&raw.region, &raw.container, &raw.id);
            ledger_for_replace.remove_by_kind_handle(PluginResourceKind::Slot, &spec_handle);
            raw.id = id.clone();
            let handle = slot_handle(desc.name, &container, &id);
            ledger_for_replace.remove_by_kind_handle(PluginResourceKind::Slot, &handle);
            ledger_for_replace.record(
                PluginResourceKind::Slot,
                handle,
                raw.source_location.clone(),
            );
            b.borrow_mut().slot_ops.push(RawSlotOp::Replace {
                region: desc.name.to_string(),
                container,
                id,
                spec: raw,
            });
            Ok(())
        })?,
    )?;

    Ok(tbl)
}

fn read_address_with_id(
    t: &Table,
    desc: &RegionDescriptor,
) -> mlua::Result<(Option<String>, String, String)> {
    let pane: Option<String> = t.get("pane")?;
    let section: String = t.get("section")?;
    let id: String = t.get("id")?;
    desc.validate_address(pane.as_deref(), Some(&section))
        .map_err(mlua::Error::external)?;
    Ok((pane, section, id))
}

fn compose_container(pane: Option<&str>, section: &str) -> String {
    match pane {
        Some(p) => format!("{p}.{section}"),
        None => section.to_string(),
    }
}

fn slot_handle(region: &str, container: &str, id: &str) -> String {
    format!("{region}:{container}:{id}")
}

fn read_spec(
    lua: &Lua,
    spec: &Table,
    desc: &RegionDescriptor,
    ledger: &ResourceLedger,
) -> mlua::Result<RawSlotSpec> {
    let id: String = spec.get("id")?;
    let pane: Option<String> = spec.get("pane")?;
    let section: String = spec.get("section")?;
    desc.validate_address(pane.as_deref(), Some(&section))
        .map_err(mlua::Error::external)?;
    let priority: i32 = spec.get("priority")?;
    let container = compose_container(pane.as_deref(), &section);
    let handle = slot_handle(desc.name, &container, &id);
    let source_location = ResourceLedger::source_location(lua);
    ledger.remove_by_kind_handle(PluginResourceKind::Slot, &handle);
    ledger.record(
        PluginResourceKind::Slot,
        handle.clone(),
        source_location.clone(),
    );
    let widget = read_widget(lua, spec, ledger, &handle, source_location.clone())?;
    let on_click_fn: Option<Function> = spec.get("on_click")?;
    let on_click = on_click_fn
        .map(|f| {
            let key = lua.create_registry_value(f)?;
            ledger.record(
                PluginResourceKind::LuaRegistryKey,
                format!("{handle}:on_click"),
                source_location.clone(),
            );
            Ok::<mlua::RegistryKey, mlua::Error>(key)
        })
        .transpose()?;
    Ok(RawSlotSpec {
        id,
        region: desc.name.to_string(),
        container,
        priority,
        widget,
        on_click,
        source_location,
    })
}

fn read_widget(
    lua: &Lua,
    spec: &Table,
    ledger: &ResourceLedger,
    slot_handle: &str,
    source_location: Option<String>,
) -> mlua::Result<WidgetSource> {
    let v: LuaValue = spec.get("widget")?;
    Ok(match v {
        LuaValue::Function(f) => {
            let key = lua.create_registry_value(f)?;
            ledger.record(
                PluginResourceKind::LuaRegistryKey,
                format!("{slot_handle}:widget"),
                source_location,
            );
            WidgetSource::Dynamic(key)
        }
        other => {
            let json: serde_json::Value = lua
                .from_value(other)
                .map_err(|e| mlua::Error::external(format!("invalid widget tree: {e}")))?;
            // Decode the static tree into the typed `WidgetAst`. Field-
            // level errors (unknown kind, type mismatch, depth/node/string
            // limits) surface here at boundary time and abort the slot
            // registration so the plugin author sees a precise reason.
            let ast = widget_ast::decode(&json)
                .map_err(|e| mlua::Error::external(format!("invalid widget tree: {e}")))?;
            WidgetSource::Static(ast)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_leviathan_plugin_api::descriptor::region::REGIONS;
    use mlua::Lua;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn fresh() -> (Lua, Rc<RefCell<BuildState>>) {
        let lua = Lua::new();
        let build = Rc::new(RefCell::new(BuildState::default()));
        (lua, build)
    }

    #[test]
    fn handle_add_records_op() {
        let (lua, build) = fresh();
        let desc = REGIONS.get("main_bar").unwrap();
        let ledger = ResourceLedger::new(
            "test".into(),
            crate::plugin::resources::GenerationId::new(1),
        );
        let handle = make_region_handle(&lua, Rc::clone(&build), ledger, desc).unwrap();
        lua.globals().set("h", handle).unwrap();
        lua.load(r#"h.add{ id = "x", section = "left", priority = 0, widget = { kind = "text", text = "hi" } }"#)
            .exec().unwrap();
        let ops = &build.borrow().slot_ops;
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn handle_rejects_bad_section() {
        let (lua, build) = fresh();
        let desc = REGIONS.get("main_bar").unwrap();
        let ledger = ResourceLedger::new(
            "test".into(),
            crate::plugin::resources::GenerationId::new(1),
        );
        let handle = make_region_handle(&lua, Rc::clone(&build), ledger, desc).unwrap();
        lua.globals().set("h", handle).unwrap();
        let err = lua.load(r#"h.add{ id = "x", section = "nope", priority = 0, widget = { kind = "text", text = "hi" } }"#)
            .exec().unwrap_err().to_string();
        assert!(err.contains("unknown section 'nope'"), "got: {err}");
    }

    #[test]
    fn factory_rejects_unknown_widget_kind() {
        let (lua, build) = fresh();
        let desc = REGIONS.get("main_bar").unwrap();
        let ledger = ResourceLedger::new(
            "test".into(),
            crate::plugin::resources::GenerationId::new(1),
        );
        let handle = make_region_handle(&lua, Rc::clone(&build), ledger, desc).unwrap();
        lua.globals().set("h", handle).unwrap();
        let err = lua
            .load(r#"h.add{ id = "x", section = "left", priority = 0, widget = { kind = "rwo", value = "hi" } }"#)
            .exec()
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("invalid widget tree") || err.contains("unknown variant"),
            "got: {err}"
        );
    }

    #[test]
    fn content_region_requires_pane() {
        let (lua, build) = fresh();
        let desc = REGIONS.get("repository").unwrap();
        let ledger = ResourceLedger::new(
            "test".into(),
            crate::plugin::resources::GenerationId::new(1),
        );
        let handle = make_region_handle(&lua, Rc::clone(&build), ledger, desc).unwrap();
        lua.globals().set("h", handle).unwrap();
        let err = lua.load(r#"h.add{ id = "x", section = "top", priority = 0, widget = { kind = "text", text = "hi" } }"#)
            .exec().unwrap_err().to_string();
        assert!(err.contains("unknown pane"), "got: {err}");
    }
}

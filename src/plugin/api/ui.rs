//! `leviathan.ui` slot hooks and screen registration.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Table};

use git_leviathan_plugin_api::descriptor::region::REGIONS;

use super::factory::{compose_container, read_address_with_id, read_spec, slot_handle};
use super::{BuildState, RawSlotOp, ScreenDef};
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};

pub fn install(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    leviathan: &Table,
) -> mlua::Result<()> {
    let ui = lua.create_table()?;

    let names_owned: Vec<String> = REGIONS.iter().map(|d| d.name.to_string()).collect();
    ui.set(
        "list_regions",
        lua.create_function(move |_, ()| Ok(names_owned.clone()))?,
    )?;

    install_regions_api(lua, Rc::clone(&build), ledger.clone(), &ui)?;
    install_screen_register(lua, build, ledger, &ui)?;
    leviathan.set("ui", ui)?;
    Ok(())
}

fn install_regions_api(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    ui: &Table,
) -> mlua::Result<()> {
    let regions = lua.create_table()?;

    let b = Rc::clone(&build);
    let ledger_for_add = ledger.clone();
    regions.set(
        "add_slot",
        lua.create_function(move |lua_inner, spec: Table| {
            let region: String = spec.get("region")?;
            let desc = REGIONS
                .get(&region)
                .ok_or_else(|| mlua::Error::external(format!("unknown region: {region}")))?;
            let raw = read_spec(lua_inner, &spec, desc, &ledger_for_add)?;
            b.borrow_mut().slot_ops.push(RawSlotOp::Add(raw));
            Ok(())
        })?,
    )?;

    let b = Rc::clone(&build);
    let ledger_for_remove = ledger.clone();
    let remove_plugin_id = ledger_for_remove.plugin_id().to_string();
    regions.set(
        "remove_slot",
        lua.create_function(move |_, target: Table| {
            let region: String = target.get("region")?;
            let desc = REGIONS
                .get(&region)
                .ok_or_else(|| mlua::Error::external(format!("unknown region: {region}")))?;
            let (pane, section, id) = read_address_with_id(&target, desc)?;
            let container = compose_container(pane.as_deref(), &section);
            let handle = slot_handle(desc.name, &container, &id);
            ledger_for_remove.remove_by_kind_handle(PluginResourceKind::Slot, &handle);
            b.borrow_mut().slot_ops.push(RawSlotOp::Remove {
                plugin_id: remove_plugin_id.clone(),
                region: desc.name.to_string(),
                container,
                id,
            });
            Ok(())
        })?,
    )?;

    let b = Rc::clone(&build);
    regions.set(
        "replace_slot",
        lua.create_function(move |lua_inner, (target, spec): (Table, Table)| {
            let region: String = target.get("region")?;
            let desc = REGIONS
                .get(&region)
                .ok_or_else(|| mlua::Error::external(format!("unknown region: {region}")))?;
            let (pane, section, id) = read_address_with_id(&target, desc)?;
            let container = compose_container(pane.as_deref(), &section);
            let mut raw = read_spec(lua_inner, &spec, desc, &ledger)?;
            if raw.container != container {
                return Err(mlua::Error::external(format!(
                    "{}.replace_slot: spec container '{}' != target '{}'",
                    desc.name, raw.container, container
                )));
            }
            let spec_handle = slot_handle(&raw.region, &raw.container, &raw.id);
            ledger.remove_by_kind_handle(PluginResourceKind::Slot, &spec_handle);
            raw.id = id.clone();
            let handle = slot_handle(desc.name, &container, &id);
            ledger.remove_by_kind_handle(PluginResourceKind::Slot, &handle);
            ledger.record(
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

    ui.set("regions", regions)?;
    Ok(())
}

fn install_screen_register(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    ui: &Table,
) -> mlua::Result<()> {
    ui.set(
        "register_screen",
        lua.create_function(move |lua_inner, spec: Table| {
            let id: String = spec.get("id")?;
            let init: Function = spec.get("init")?;
            let view: Function = spec.get("view")?;
            let update: Function = spec.get("update")?;
            let serialize_opt: Option<Function> = spec.get("serialize")?;
            let deserialize_opt: Option<Function> = spec.get("deserialize")?;
            let source = ResourceLedger::source_location(lua_inner);
            let prefix = format!("screen:{id}");
            ledger.remove_by_handle_prefix(&prefix);
            ledger.record(PluginResourceKind::Screen, prefix.clone(), source.clone());
            let def = ScreenDef {
                init: register_screen_key(
                    lua_inner,
                    &ledger,
                    &prefix,
                    "init",
                    init,
                    source.clone(),
                )?,
                view: register_screen_key(
                    lua_inner,
                    &ledger,
                    &prefix,
                    "view",
                    view,
                    source.clone(),
                )?,
                update: register_screen_key(
                    lua_inner,
                    &ledger,
                    &prefix,
                    "update",
                    update,
                    source.clone(),
                )?,
                serialize: serialize_opt
                    .map(|f| {
                        register_screen_key(
                            lua_inner,
                            &ledger,
                            &prefix,
                            "serialize",
                            f,
                            source.clone(),
                        )
                    })
                    .transpose()?,
                deserialize: deserialize_opt
                    .map(|f| {
                        register_screen_key(
                            lua_inner,
                            &ledger,
                            &prefix,
                            "deserialize",
                            f,
                            source.clone(),
                        )
                    })
                    .transpose()?,
            };
            build.borrow_mut().screens.insert(id, def);
            Ok(())
        })?,
    )?;
    Ok(())
}

fn register_screen_key(
    lua: &Lua,
    ledger: &ResourceLedger,
    prefix: &str,
    name: &str,
    function: Function,
    source: Option<String>,
) -> mlua::Result<mlua::RegistryKey> {
    let key = lua.create_registry_value(function)?;
    ledger.record(
        PluginResourceKind::LuaRegistryKey,
        format!("{prefix}:{name}"),
        source,
    );
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn install_test_harness() -> (Lua, Rc<RefCell<BuildState>>) {
        let lua = Lua::new();
        let build = Rc::new(RefCell::new(BuildState::default()));
        let leviathan = lua.create_table().unwrap();
        let ledger = ResourceLedger::new(
            "test".into(),
            crate::plugin::resources::GenerationId::new(1),
        );
        super::install(&lua, Rc::clone(&build), ledger, &leviathan).unwrap();
        lua.globals().set("leviathan", leviathan).unwrap();
        (lua, build)
    }

    #[test]
    fn direct_region_handles_are_not_installed() {
        let (lua, _) = install_test_harness();
        let ok: bool = lua
            .load(r#"return leviathan.ui["main_bar"] == nil and leviathan.ui["region"] == nil"#)
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn list_regions_returns_all() {
        let (lua, _) = install_test_harness();
        let names: Vec<String> = lua
            .load("return leviathan.ui.list_regions()")
            .eval()
            .unwrap();
        assert_eq!(
            names,
            vec![
                "main_bar",
                "tab_bar",
                "status_bar",
                "repository",
                "repository.graph",
                "repository.details",
                "repository.diff",
            ]
        );
    }

    #[test]
    fn regions_api_adds_slot() {
        let (lua, build) = install_test_harness();
        lua.load(
            r#"
            leviathan.ui.regions.add_slot{
                region = "main_bar",
                section = "left",
                id = "x",
                priority = 0,
                widget = { kind = "text", value = "hi" },
            }
        "#,
        )
        .exec()
        .unwrap();
        assert_eq!(build.borrow().slot_ops.len(), 1);
    }
}

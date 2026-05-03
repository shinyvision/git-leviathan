//! `leviathan.ui` — region-scoped slot hooks + screen registration.
//!
//! Each region listed in [`REGIONS`] gets a `{add, remove, replace}`
//! handle on `leviathan.ui.<region>` produced by
//! [`make_region_handle`](super::factory::make_region_handle). Plugins
//! address slots uniformly via `{ section = … }` (chrome) or
//! `{ pane = …, section = … }` (content).

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Table};

use git_leviathan_plugin_api::descriptor::region::REGIONS;

use super::factory::make_region_handle;
use super::{BuildState, ScreenDef};
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};

pub fn install(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    leviathan: &Table,
) -> mlua::Result<()> {
    let ui = lua.create_table()?;

    for desc in REGIONS.iter() {
        let handle = make_region_handle(lua, Rc::clone(&build), ledger.clone(), desc)?;
        // Region names may contain dots (e.g. `repository.graph`) — so
        // the Lua handle table lives under the nested path
        // `leviathan.ui.repository.graph`. Walk the segments,
        // creating intermediate tables as needed. The final segment
        // owns the region handle.
        place_region_handle(lua, &ui, desc.name, handle)?;
    }

    let names_owned: Vec<String> = REGIONS.iter().map(|d| d.name.to_string()).collect();
    ui.set(
        "list_regions",
        lua.create_function(move |_, ()| Ok(names_owned.clone()))?,
    )?;

    let ui_ref = ui.clone();
    ui.set(
        "region",
        lua.create_function(move |_, name: String| -> mlua::Result<Table> {
            // Look up by descriptor name — handles the dotted form
            // (e.g. `repository.graph`) without forcing callers to
            // know whether it's nested.
            lookup_region_handle(&ui_ref, &name)
                .map_err(|_| mlua::Error::external(format!("unknown region: {name}")))
        })?,
    )?;

    install_screen_register(lua, build, ledger, &ui)?;
    leviathan.set("ui", ui)?;
    Ok(())
}

/// Place a region handle at the dotted path `name` under `root`.
///
/// `name` is taken from the descriptor table verbatim, so segments
/// match `leviathan.ui[<segment>]` Lua lookups. For a flat name like
/// `main_bar`, the handle goes straight under `ui.main_bar`. For a
/// dotted name like `repository.graph`, we place the handle's
/// `add/remove/replace` fields onto an existing or freshly-created
/// `repository.graph` sub-table — the parent `repository` is a peer
/// region, not an intermediate.
fn place_region_handle(
    lua: &Lua,
    root: &Table,
    name: &str,
    handle: Table,
) -> mlua::Result<()> {
    let segments: Vec<&str> = name.split('.').collect();
    let mut current = root.clone();
    for segment in &segments[..segments.len() - 1] {
        let next: mlua::Value = current.get(*segment)?;
        match next {
            mlua::Value::Table(t) => current = t,
            mlua::Value::Nil => {
                let fresh = lua.create_table()?;
                current.set(*segment, fresh.clone())?;
                current = fresh;
            }
            _ => {
                return Err(mlua::Error::external(format!(
                    "region path collision at '{segment}': non-table value already present"
                )));
            }
        }
    }
    let last = segments[segments.len() - 1];
    // Merge the handle's fields onto whatever already lives at
    // `last` so a previously-installed intermediate sub-table (e.g.
    // a child like `repository.graph` placed before `repository`)
    // keeps its entries.
    let existing: mlua::Value = current.get(last)?;
    match existing {
        mlua::Value::Table(t) => {
            for pair in handle.pairs::<mlua::Value, mlua::Value>() {
                let (k, v) = pair?;
                t.set(k, v)?;
            }
        }
        mlua::Value::Nil => {
            current.set(last, handle)?;
        }
        _ => {
            return Err(mlua::Error::external(format!(
                "region path collision at '{last}': non-table value already present"
            )));
        }
    }
    Ok(())
}

fn lookup_region_handle(root: &Table, name: &str) -> mlua::Result<Table> {
    let mut current = root.clone();
    for (i, segment) in name.split('.').enumerate() {
        let value: mlua::Value = current.get(segment)?;
        match value {
            mlua::Value::Table(t) => current = t,
            _ => {
                return Err(mlua::Error::external(format!(
                    "no region table at segment {i} of '{name}'"
                )));
            }
        }
    }
    Ok(current)
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
    fn all_regions_have_add_remove_replace() {
        let (lua, _) = install_test_harness();
        for region in ["main_bar", "tab_bar", "repository"] {
            for verb in ["add", "remove", "replace"] {
                let chunk = format!("return type(leviathan.ui.{region}.{verb}) == 'function'");
                let ok: bool = lua.load(&chunk).eval().unwrap();
                assert!(ok, "{region}.{verb} missing");
            }
        }
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
    fn region_lookup_returns_handle() {
        let (lua, build) = install_test_harness();
        lua.load(r#"
            local h = leviathan.ui.region("main_bar")
            h.add{ id = "x", section = "left", priority = 0, widget = { kind = "text", value = "hi" } }
        "#).exec().unwrap();
        assert_eq!(build.borrow().slot_ops.len(), 1);
    }
}

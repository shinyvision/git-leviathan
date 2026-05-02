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

pub fn install(lua: &Lua, build: Rc<RefCell<BuildState>>, leviathan: &Table) -> mlua::Result<()> {
    let ui = lua.create_table()?;

    for desc in REGIONS.iter() {
        let handle = make_region_handle(lua, Rc::clone(&build), desc)?;
        ui.set(desc.name, handle)?;
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
            ui_ref
                .get::<Table>(name.as_str())
                .map_err(|_| mlua::Error::external(format!("unknown region: {name}")))
        })?,
    )?;

    install_screen_register(lua, build, &ui)?;
    leviathan.set("ui", ui)?;
    Ok(())
}

fn install_screen_register(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ui: &Table,
) -> mlua::Result<()> {
    ui.set(
        "register_screen",
        lua.create_function(move |lua_inner, spec: Table| {
            let id: String = spec.get("id")?;
            let init: Function = spec.get("init")?;
            let view: Function = spec.get("view")?;
            let update: Function = spec.get("update")?;
            let def = ScreenDef {
                init: lua_inner.create_registry_value(init)?,
                view: lua_inner.create_registry_value(view)?,
                update: lua_inner.create_registry_value(update)?,
            };
            build.borrow_mut().screens.insert(id, def);
            Ok(())
        })?,
    )?;
    Ok(())
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
        super::install(&lua, Rc::clone(&build), &leviathan).unwrap();
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
        let names: Vec<String> = lua.load("return leviathan.ui.list_regions()").eval().unwrap();
        assert_eq!(names, vec!["main_bar", "tab_bar", "repository"]);
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

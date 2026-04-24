//! `leviathan.ui` — main-bar slot hooks + screen registration.
//!
//! Two APIs:
//!
//! 1. **`main_bar.add / remove / replace`** — the hook API. Any plugin
//!    can contribute to *or* mutate the main bar: add new slots in any
//!    section, remove a built-in, or swap one out.
//!
//! 2. **`register_screen { id, init, view, update }`** — declares a full
//!    plugin-owned screen.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, LuaSerdeExt, Table, Value as LuaValue};

use super::{BuildState, RawSlotOp, RawSlotSpec, ScreenDef, WidgetSource};

pub fn install(lua: &Lua, build: Rc<RefCell<BuildState>>, leviathan: &Table) -> mlua::Result<()> {
    let ui = lua.create_table()?;

    install_main_bar_hooks(lua, Rc::clone(&build), &ui)?;
    install_screen_register(lua, Rc::clone(&build), &ui)?;

    leviathan.set("ui", ui)?;
    Ok(())
}

fn install_main_bar_hooks(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ui: &Table,
) -> mlua::Result<()> {
    let tbl = lua.create_table()?;

    let build_for_add = Rc::clone(&build);
    tbl.set(
        "add",
        lua.create_function(move |lua_inner, spec: Table| {
            let raw = read_raw_slot_spec(lua_inner, spec)?;
            build_for_add.borrow_mut().slot_ops.push(RawSlotOp::Add(raw));
            Ok(())
        })?,
    )?;

    let build_for_remove = Rc::clone(&build);
    tbl.set(
        "remove",
        lua.create_function(move |_, id: String| {
            build_for_remove.borrow_mut().slot_ops.push(RawSlotOp::Remove(id));
            Ok(())
        })?,
    )?;

    let build_for_replace = Rc::clone(&build);
    tbl.set(
        "replace",
        lua.create_function(move |lua_inner, (id, spec): (String, Table)| {
            let raw = read_raw_slot_spec(lua_inner, spec)?;
            build_for_replace
                .borrow_mut()
                .slot_ops
                .push(RawSlotOp::Replace(id, raw));
            Ok(())
        })?,
    )?;

    ui.set("main_bar", tbl)?;
    Ok(())
}

fn read_raw_slot_spec(lua: &Lua, spec: Table) -> mlua::Result<RawSlotSpec> {
    let id: String = spec.get("id")?;
    let section: String = spec.get("section")?;
    let priority: i32 = spec.get("priority")?;

    // `widget` is either the plugin's widget-tree table or a Lua
    // function that returns one. A table is serialised to JSON now so it
    // can outlive the BuildState; a function is stashed as a registry
    // key the host re-invokes whenever plugin state might have changed.
    // Missing widget = empty static node; host renders as an error text
    // at use time.
    let widget_val: LuaValue = spec.get("widget")?;
    let widget = match widget_val {
        LuaValue::Function(f) => WidgetSource::Dynamic(lua.create_registry_value(f)?),
        other => {
            let v: serde_json::Value = lua.from_value(other).map_err(|e| {
                mlua::Error::external(format!("main_bar.add: invalid widget tree: {e}"))
            })?;
            WidgetSource::Static(v)
        }
    };

    let on_click_fn: Option<Function> = spec.get::<Option<Function>>("on_click")?;
    let on_click = on_click_fn
        .map(|f| lua.create_registry_value(f))
        .transpose()?;

    Ok(RawSlotSpec {
        id,
        section,
        priority,
        widget,
        on_click,
    })
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

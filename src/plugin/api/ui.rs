//! `leviathan.ui` — region-scoped slot hooks + screen registration.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, LuaSerdeExt, Table, Value as LuaValue};

use super::{BuildState, RawSlotOp, RawSlotSpec, ScreenDef, WidgetSource};

pub fn install(lua: &Lua, build: Rc<RefCell<BuildState>>, leviathan: &Table) -> mlua::Result<()> {
    let ui = lua.create_table()?;

    install_regions_api(lua, Rc::clone(&build), &ui)?;
    install_main_bar_compat(lua, Rc::clone(&build), &ui)?;
    install_tab_bar_compat(lua, Rc::clone(&build), &ui)?;
    install_screen_register(lua, Rc::clone(&build), &ui)?;

    leviathan.set("ui", ui)?;
    Ok(())
}

/// Generic region-scoped surface. Plugins that target tab_bar /
/// repository panes use this. Main-bar slots can use it too.
fn install_regions_api(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ui: &Table,
) -> mlua::Result<()> {
    let tbl = lua.create_table()?;

    let b = Rc::clone(&build);
    tbl.set(
        "add_slot",
        lua.create_function(move |lua_inner, spec: Table| {
            let raw = read_raw_slot_spec(lua_inner, spec)?;
            b.borrow_mut().slot_ops.push(RawSlotOp::Add(raw));
            Ok(())
        })?,
    )?;

    let b = Rc::clone(&build);
    tbl.set(
        "remove_slot",
        lua.create_function(move |_, args: Table| {
            let region: String = args.get("region")?;
            let container = read_container_field(&args)?;
            let id: String = args.get("id")?;
            b.borrow_mut().slot_ops.push(RawSlotOp::Remove { region, container, id });
            Ok(())
        })?,
    )?;

    let b = Rc::clone(&build);
    tbl.set(
        "replace_slot",
        lua.create_function(move |lua_inner, (target, spec): (Table, Table)| {
            let region: String = target.get("region")?;
            let container = read_container_field(&target)?;
            let id: String = target.get("id")?;
            let mut raw = read_raw_slot_spec(lua_inner, spec)?;
            if raw.region != region || raw.container != container {
                return Err(mlua::Error::external(format!(
                    "regions.replace_slot: spec address mismatches target ({}/{} vs {}/{})",
                    raw.region, raw.container, region, container
                )));
            }
            raw.id = id.clone();
            b.borrow_mut()
                .slot_ops
                .push(RawSlotOp::Replace { region, container, id, spec: raw });
            Ok(())
        })?,
    )?;

    ui.set("regions", tbl)?;
    Ok(())
}

/// Back-compat: `leviathan.ui.main_bar.{add,remove,replace}` keeps
/// working. Each lowers to a `regions.*` op against `region="main_bar"`.
fn install_main_bar_compat(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ui: &Table,
) -> mlua::Result<()> {
    let tbl = lua.create_table()?;

    let b = Rc::clone(&build);
    tbl.set(
        "add",
        lua.create_function(move |lua_inner, spec: Table| {
            let raw = read_main_bar_slot_spec(lua_inner, spec)?;
            b.borrow_mut().slot_ops.push(RawSlotOp::Add(raw));
            Ok(())
        })?,
    )?;

    let b = Rc::clone(&build);
    tbl.set(
        "remove",
        lua.create_function(move |_lua, id: String| {
            // Pre-region API didn't take a section. Use empty container
            // string as a sentinel; the applier in host.rs scans every
            // container in the main_bar region.
            b.borrow_mut().slot_ops.push(RawSlotOp::Remove {
                region: "main_bar".to_string(),
                container: String::new(),
                id,
            });
            Ok(())
        })?,
    )?;

    let b = Rc::clone(&build);
    tbl.set(
        "replace",
        lua.create_function(move |lua_inner, (id, spec): (String, Table)| {
            let mut raw = read_main_bar_slot_spec(lua_inner, spec)?;
            let container = raw.container.clone();
            raw.id = id.clone();
            b.borrow_mut().slot_ops.push(RawSlotOp::Replace {
                region: "main_bar".to_string(),
                container,
                id,
                spec: raw,
            });
            Ok(())
        })?,
    )?;

    ui.set("main_bar", tbl)?;
    Ok(())
}

/// `leviathan.ui.tab_bar.{add,remove,replace}` — same nicer surface
/// `main_bar` got, hardcoded against `region="tab_bar"`. Spec carries
/// `section` (left/center/right). `id`-keyed lookups; `replace` lets a
/// plugin swap a `builtin.<name>` slot the same way the dancing-banana
/// demo replaces `builtin.fetch_indicator`.
fn install_tab_bar_compat(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ui: &Table,
) -> mlua::Result<()> {
    let tbl = lua.create_table()?;

    let b = Rc::clone(&build);
    tbl.set(
        "add",
        lua.create_function(move |lua_inner, spec: Table| {
            let raw = read_tab_bar_slot_spec(lua_inner, spec)?;
            b.borrow_mut().slot_ops.push(RawSlotOp::Add(raw));
            Ok(())
        })?,
    )?;

    let b = Rc::clone(&build);
    tbl.set(
        "remove",
        lua.create_function(move |_lua, id: String| {
            b.borrow_mut().slot_ops.push(RawSlotOp::Remove {
                region: "tab_bar".to_string(),
                container: String::new(),
                id,
            });
            Ok(())
        })?,
    )?;

    let b = Rc::clone(&build);
    tbl.set(
        "replace",
        lua.create_function(move |lua_inner, (id, spec): (String, Table)| {
            let mut raw = read_tab_bar_slot_spec(lua_inner, spec)?;
            let container = raw.container.clone();
            raw.id = id.clone();
            b.borrow_mut().slot_ops.push(RawSlotOp::Replace {
                region: "tab_bar".to_string(),
                container,
                id,
                spec: raw,
            });
            Ok(())
        })?,
    )?;

    ui.set("tab_bar", tbl)?;
    Ok(())
}

fn read_tab_bar_slot_spec(lua: &Lua, spec: Table) -> mlua::Result<RawSlotSpec> {
    let id: String = spec.get("id")?;
    let section: String = spec.get("section")?;
    let priority: i32 = spec.get("priority")?;
    let widget = read_widget_field(lua, &spec)?;
    let on_click_fn: Option<Function> = spec.get::<Option<Function>>("on_click")?;
    let on_click = on_click_fn
        .map(|f| lua.create_registry_value(f))
        .transpose()?;
    Ok(RawSlotSpec {
        id,
        region: "tab_bar".to_string(),
        container: section,
        priority,
        widget,
        on_click,
    })
}

fn read_container_field(args: &Table) -> mlua::Result<String> {
    // Either `section = "left"` (chrome) or `pane = "sidebar", section = "top"`
    // (content). The host stores both as a single container key:
    // `{section}` or `{pane}.{section}`.
    let section: Option<String> = args.get("section")?;
    let pane: Option<String> = args.get("pane")?;
    match (pane, section) {
        (None, Some(s)) => Ok(s),
        (Some(p), Some(s)) => Ok(format!("{p}.{s}")),
        (Some(_), None) => Err(mlua::Error::external(
            "regions.*: when `pane` is given, `section` is required",
        )),
        (None, None) => Err(mlua::Error::external(
            "regions.*: missing `section` (and optional `pane`)",
        )),
    }
}

fn read_raw_slot_spec(lua: &Lua, spec: Table) -> mlua::Result<RawSlotSpec> {
    let id: String = spec.get("id")?;
    let region: String = spec.get("region")?;
    let container = read_container_field(&spec)?;
    let priority: i32 = spec.get("priority")?;
    let widget = read_widget_field(lua, &spec)?;
    let on_click_fn: Option<Function> = spec.get::<Option<Function>>("on_click")?;
    let on_click = on_click_fn
        .map(|f| lua.create_registry_value(f))
        .transpose()?;
    Ok(RawSlotSpec {
        id,
        region,
        container,
        priority,
        widget,
        on_click,
    })
}

fn read_main_bar_slot_spec(lua: &Lua, spec: Table) -> mlua::Result<RawSlotSpec> {
    let id: String = spec.get("id")?;
    let section: String = spec.get("section")?;
    let priority: i32 = spec.get("priority")?;
    let widget = read_widget_field(lua, &spec)?;
    let on_click_fn: Option<Function> = spec.get::<Option<Function>>("on_click")?;
    let on_click = on_click_fn
        .map(|f| lua.create_registry_value(f))
        .transpose()?;
    Ok(RawSlotSpec {
        id,
        region: "main_bar".to_string(),
        container: section,
        priority,
        widget,
        on_click,
    })
}

fn read_widget_field(lua: &Lua, spec: &Table) -> mlua::Result<WidgetSource> {
    let v: LuaValue = spec.get("widget")?;
    Ok(match v {
        LuaValue::Function(f) => WidgetSource::Dynamic(lua.create_registry_value(f)?),
        other => {
            let json: serde_json::Value = lua.from_value(other).map_err(|e| {
                mlua::Error::external(format!("invalid widget tree: {e}"))
            })?;
            WidgetSource::Static(json)
        }
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

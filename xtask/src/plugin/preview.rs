use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use git_leviathan_plugin_api::descriptor::region::REGIONS;
use git_leviathan_plugin_api::descriptor::widget::WIDGETS;
use git_leviathan_plugin_api::manifest::PluginManifest;
use mlua::{Function, Lua, LuaSerdeExt, Table, Value as LuaValue};
use serde_json::{json, Map, Value};

use super::{path_label, set_package_path, DynResult};

#[derive(Default)]
struct PreviewState {
    contributions: Vec<Value>,
    commands: Vec<Value>,
    autocmds: Vec<Value>,
    diagnostics: Vec<Value>,
    settings_schema: Value,
    settings_values: Value,
}

pub(super) fn plugin_preview(args: &[String]) -> DynResult<()> {
    let mut path = None;
    let mut context_path = None;
    let mut out_path = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--context" => {
                i += 1;
                context_path = Some(PathBuf::from(args.get(i).ok_or("missing --context value")?));
            }
            "--out" => {
                i += 1;
                out_path = Some(PathBuf::from(args.get(i).ok_or("missing --out value")?));
            }
            value if value.starts_with("--") => return Err(format!("unknown flag: {value}").into()),
            value => {
                if path.replace(PathBuf::from(value)).is_some() {
                    return Err("plugin preview accepts at most one [path]".into());
                }
            }
        }
        i += 1;
    }

    let path = path.unwrap_or_else(|| PathBuf::from("."));
    let context = match context_path {
        Some(path) => serde_json::from_str(&fs::read_to_string(path)?)?,
        None => default_context(),
    };
    let snapshot = run_preview(&path, context)?;
    let rendered = serde_json::to_string_pretty(&snapshot)?;
    if let Some(out_path) = out_path {
        fs::write(&out_path, rendered)?;
        println!("wrote {}", out_path.display());
    } else {
        println!("{rendered}");
    }
    Ok(())
}

pub(super) fn run_preview(path: &Path, context: Value) -> DynResult<Value> {
    let manifest_raw = fs::read_to_string(path.join("plugin.toml"))?;
    let manifest: PluginManifest = toml::from_str(&manifest_raw)?;
    let init_path = path.join("init.lua");
    let source = fs::read_to_string(&init_path)?;

    let lua = Lua::new();
    let state = Rc::new(RefCell::new(PreviewState::default()));
    install_preview_api(&lua, state.clone(), context.clone())?;
    set_package_path(&lua, path)?;
    if let Err(e) = lua.load(&source).set_name(path_label(&init_path)).exec() {
        state.borrow_mut().diagnostics.push(json!({
            "level": "error",
            "code": "preview.lua_error",
            "message": e.to_string(),
        }));
    }

    let state = state.borrow();
    Ok(json!({
        "format": "git-leviathan-plugin-preview-v1",
        "plugin": {
            "id": manifest.id,
            "name": manifest.name,
            "version": manifest.version,
            "api_version": format!("{}.{}", manifest.api_version.major, manifest.api_version.minor),
            "capabilities": manifest.capabilities.into_iter().map(String::from).collect::<Vec<_>>(),
        },
        "context": context,
        "regions": REGIONS.names(),
        "contributions": state.contributions,
        "commands": state.commands,
        "autocmds": state.autocmds,
        "diagnostics": state.diagnostics,
    }))
}

fn install_preview_api(
    lua: &Lua,
    state: Rc<RefCell<PreviewState>>,
    context: Value,
) -> DynResult<()> {
    let globals = lua.globals();
    let leviathan = lua.create_table()?;
    globals.set("leviathan", leviathan.clone())?;
    leviathan.set(
        "log",
        lua.create_function(|_, message: String| {
            eprintln!("plugin preview: {message}");
            Ok(())
        })?,
    )?;
    leviathan.set("has", lua.create_function(|_, _: String| Ok(true))?)?;

    let ui = lua.create_table()?;
    leviathan.set("ui", ui.clone())?;
    install_slot_api(lua, &ui, state.clone(), context.clone())?;
    install_region_api(lua, &ui)?;
    install_context_api(lua, &ui, context.clone())?;
    install_extension_api(lua, &ui, state.clone(), context.clone())?;
    install_dock_api(lua, &ui, state.clone(), context.clone())?;
    install_screen_api(lua, &ui, state.clone(), context.clone())?;
    install_settings_api(lua, &leviathan, state.clone())?;
    install_settings_ui_api(lua, &ui, state.clone(), context.clone())?;
    install_command_api(lua, &leviathan, state.clone())?;
    install_autocmd_api(lua, &leviathan, state)?;
    install_noop_namespaces(lua, &leviathan)?;
    Ok(())
}

fn install_slot_api(
    lua: &Lua,
    ui: &Table,
    state: Rc<RefCell<PreviewState>>,
    context: Value,
) -> DynResult<()> {
    let slot = lua.create_table()?;
    let add_state = state.clone();
    let add_context = context.clone();
    slot.set(
        "add",
        lua.create_function(move |lua, spec: LuaValue| {
            let record = contribution_record(lua, "slot", spec, &add_context)?;
            validate_widget_record(&record, &add_state);
            add_state.borrow_mut().contributions.push(record.clone());
            handle_for(lua, record)
        })?,
    )?;
    slot.set(
        "replace",
        lua.create_function(move |_, _: mlua::MultiValue| Ok(true))?,
    )?;
    slot.set(
        "remove",
        lua.create_function(move |_, _: LuaValue| Ok(true))?,
    )?;
    ui.set("slot", slot)?;
    Ok(())
}

fn install_region_api(lua: &Lua, ui: &Table) -> DynResult<()> {
    let region = lua.create_table()?;
    region.set(
        "list",
        lua.create_function(|lua, _: ()| lua.to_value(&REGIONS.names()))?,
    )?;
    region.set(
        "describe",
        lua.create_function(|lua, name: String| {
            let Some(desc) = git_leviathan_plugin_api::descriptor::api::describe()
                .regions
                .into_iter()
                .find(|region| region.name == name)
            else {
                return Ok((LuaValue::Nil, Some(format!("unknown region `{name}`"))));
            };
            Ok((lua.to_value(&desc)?, Option::<String>::None))
        })?,
    )?;
    ui.set("region", region)?;
    Ok(())
}

fn install_context_api(lua: &Lua, ui: &Table, context: Value) -> DynResult<()> {
    let ctx = lua.create_table()?;
    ctx.set(
        "current",
        lua.create_function(move |lua, _: ()| {
            Ok((lua.to_value(&context)?, Option::<String>::None))
        })?,
    )?;
    ui.set("context", ctx)?;
    Ok(())
}

fn install_extension_api(
    lua: &Lua,
    ui: &Table,
    state: Rc<RefCell<PreviewState>>,
    context: Value,
) -> DynResult<()> {
    for name in ["overlay", "diff_decoration"] {
        let state = state.clone();
        let context = context.clone();
        ui.set(
            name,
            lua.create_function(move |lua, spec: LuaValue| {
                let record = contribution_record(lua, name, spec, &context)?;
                validate_widget_record(&record, &state);
                state.borrow_mut().contributions.push(record);
                Ok(())
            })?,
        )?;
    }
    let state_for_graph = state.clone();
    let graph_context = context.clone();
    ui.set(
        "graph_decoration",
        lua.create_function(move |lua, (commit_hash, spec): (String, LuaValue)| {
            let mut record = contribution_record(lua, "graph_decoration", spec, &graph_context)?;
            record["commit_hash"] = json!(commit_hash);
            state_for_graph.borrow_mut().contributions.push(record);
            Ok(())
        })?,
    )?;
    let state_for_menu = state.clone();
    ui.set(
        "context_menu",
        lua.create_function(move |lua, (region, item): (String, LuaValue)| {
            let mut record = contribution_record(lua, "context_menu", item, &context)?;
            record["region"] = json!(region);
            state_for_menu.borrow_mut().contributions.push(record);
            Ok(())
        })?,
    )?;
    let state_for_contribute = state;
    ui.set(
        "contribute",
        lua.create_function(move |lua, (point_id, spec): (String, LuaValue)| {
            let mut record = contribution_record(lua, "contribution", spec, &json!({}))?;
            record["point_id"] = json!(point_id);
            state_for_contribute
                .borrow_mut()
                .contributions
                .push(record.clone());
            handle_for(lua, record)
        })?,
    )?;
    Ok(())
}

fn install_dock_api(
    lua: &Lua,
    ui: &Table,
    state: Rc<RefCell<PreviewState>>,
    context: Value,
) -> DynResult<()> {
    let dock = lua.create_table()?;
    dock.set(
        "register",
        lua.create_function(move |lua, spec: LuaValue| {
            let record = contribution_record(lua, "dock_panel", spec, &context)?;
            validate_widget_record(&record, &state);
            state.borrow_mut().contributions.push(record.clone());
            handle_for(lua, record)
        })?,
    )?;
    ui.set("dock", dock)?;
    Ok(())
}

fn install_screen_api(
    lua: &Lua,
    ui: &Table,
    state: Rc<RefCell<PreviewState>>,
    context: Value,
) -> DynResult<()> {
    let screen = lua.create_table()?;
    screen.set(
        "register",
        lua.create_function(move |lua, spec: Table| {
            let id = spec.get::<Option<String>>("id")?.unwrap_or_default();
            let view = spec.get::<Option<Function>>("view")?;
            let widget = match view {
                Some(view) => {
                    let state_arg = lua.create_table()?;
                    let ctx = lua.to_value(&context)?;
                    value_to_json(lua, view.call::<LuaValue>((state_arg, ctx))?, &context)?
                }
                None => Value::Null,
            };
            state.borrow_mut().contributions.push(json!({
                "kind": "screen",
                "id": id,
                "widget": widget,
            }));
            Ok(())
        })?,
    )?;
    ui.set("screen", screen)?;
    Ok(())
}

fn install_settings_api(
    lua: &Lua,
    leviathan: &Table,
    state: Rc<RefCell<PreviewState>>,
) -> DynResult<()> {
    let settings = lua.create_table()?;
    let define_state = state.clone();
    settings.set(
        "define_schema",
        lua.create_function(move |lua, schema: LuaValue| {
            let schema_json = value_to_json(lua, schema, &json!({}))?;
            let values = settings_defaults(&schema_json);
            let mut state = define_state.borrow_mut();
            state.settings_schema = schema_json;
            state.settings_values = values;
            Ok((true, Option::<String>::None))
        })?,
    )?;
    let get_state = state.clone();
    settings.set(
        "get",
        lua.create_function(move |lua, _: ()| {
            let values = get_state.borrow().settings_values.clone();
            lua.to_value(&values)
        })?,
    )?;
    let set_state = state;
    settings.set(
        "set",
        lua.create_function(move |lua, values: LuaValue| {
            set_state.borrow_mut().settings_values = value_to_json(lua, values, &json!({}))?;
            Ok((true, Option::<String>::None))
        })?,
    )?;
    settings.set("on_change", lua.create_function(|_, _: Function| Ok(()))?)?;
    leviathan.set("settings", settings)?;
    Ok(())
}

fn install_settings_ui_api(
    lua: &Lua,
    ui: &Table,
    state: Rc<RefCell<PreviewState>>,
    context: Value,
) -> DynResult<()> {
    let settings = lua.create_table()?;
    settings.set(
        "register",
        lua.create_function(move |lua, spec: Table| {
            let view = spec.get::<Option<Function>>("view")?;
            let ctx = settings_context(&context, &state.borrow());
            let widget = match view {
                Some(view) => {
                    value_to_json(lua, view.call::<LuaValue>(lua.to_value(&ctx)?)?, &ctx)?
                }
                None => Value::Null,
            };
            state.borrow_mut().contributions.push(json!({
                "kind": "settings_panel",
                "widget": widget,
            }));
            Ok(())
        })?,
    )?;
    ui.set("settings", settings)?;
    Ok(())
}

fn settings_defaults(schema: &Value) -> Value {
    let mut values = Map::new();
    if let Some(fields) = schema.as_object() {
        for (name, spec) in fields {
            values.insert(
                name.clone(),
                spec.get("default").cloned().unwrap_or(Value::Null),
            );
        }
    }
    Value::Object(values)
}

fn settings_context(base: &Value, state: &PreviewState) -> Value {
    let mut ctx = base.as_object().cloned().unwrap_or_default();
    ctx.insert("type".into(), json!("SettingsContext"));
    ctx.insert("surface".into(), json!("settings"));
    ctx.insert("schema".into(), state.settings_schema.clone());
    ctx.insert("values".into(), state.settings_values.clone());
    Value::Object(ctx)
}

fn install_command_api(
    lua: &Lua,
    leviathan: &Table,
    state: Rc<RefCell<PreviewState>>,
) -> DynResult<()> {
    let command = lua.create_table()?;
    let create_state = state.clone();
    command.set(
        "create",
        lua.create_function(move |lua, (name, spec): (String, LuaValue)| {
            create_state.borrow_mut().commands.push(json!({
                "name": name,
                "spec": value_to_json(lua, spec, &json!({}))?,
            }));
            Ok(())
        })?,
    )?;
    command.set(
        "invoke",
        lua.create_function(|_, _: mlua::MultiValue| Ok(true))?,
    )?;
    command.set(
        "list",
        lua.create_function(|lua, _: ()| lua.to_value(&Vec::<Value>::new()))?,
    )?;
    leviathan.set("command", command)?;
    Ok(())
}

fn install_autocmd_api(
    lua: &Lua,
    leviathan: &Table,
    state: Rc<RefCell<PreviewState>>,
) -> DynResult<()> {
    let autocmd = lua.create_table()?;
    autocmd.set(
        "create",
        lua.create_function(move |lua, args: mlua::MultiValue| {
            let mut values = Vec::new();
            for value in args {
                values.push(value_to_json(lua, value, &json!({}))?);
            }
            state.borrow_mut().autocmds.push(json!({
                "args": values,
            }));
            Ok(1_i64)
        })?,
    )?;
    autocmd.set(
        "group",
        lua.create_function(|_, _: mlua::MultiValue| Ok(1_i64))?,
    )?;
    autocmd.set(
        "delete",
        lua.create_function(|_, _: mlua::MultiValue| Ok(true))?,
    )?;
    leviathan.set("autocmd", autocmd)?;
    Ok(())
}

fn install_noop_namespaces(lua: &Lua, leviathan: &Table) -> DynResult<()> {
    for name in [
        "api",
        "assets",
        "fs",
        "env",
        "repository",
        "tab_registry",
        "services",
        "persist",
        "settings",
        "secrets",
        "health",
        "runtime",
        "async",
        "timer",
        "keymap",
        "git",
    ] {
        if matches!(leviathan.get::<LuaValue>(name)?, LuaValue::Nil) {
            let table = lua.create_table()?;
            table.set(
                "create",
                lua.create_function(|_, _: mlua::MultiValue| Ok(true))?,
            )?;
            table.set(
                "register",
                lua.create_function(|_, _: mlua::MultiValue| Ok(true))?,
            )?;
            table.set(
                "set",
                lua.create_function(|_, _: mlua::MultiValue| Ok(true))?,
            )?;
            table.set(
                "get",
                lua.create_function(|_, _: mlua::MultiValue| Ok(LuaValue::Nil))?,
            )?;
            table.set(
                "list",
                lua.create_function(|lua, _: mlua::MultiValue| lua.to_value(&Vec::<Value>::new()))?,
            )?;
            leviathan.set(name, table)?;
        }
    }
    Ok(())
}

fn contribution_record(
    lua: &Lua,
    kind: &str,
    spec: LuaValue,
    context: &Value,
) -> mlua::Result<Value> {
    let mut value = match spec {
        LuaValue::Table(table) => contribution_table_to_json(lua, table, context)?,
        other => value_to_json(lua, other, context)?,
    };
    value["kind"] = json!(kind);
    Ok(value)
}

fn handle_for(lua: &Lua, record: Value) -> mlua::Result<(Table, Option<String>)> {
    let handle = lua.create_table()?;
    handle.set("address", lua.to_value(&record)?)?;
    handle.set(
        "remove",
        lua.create_function(|_, _: ()| Ok((true, Option::<String>::None)))?,
    )?;
    handle.set(
        "replace",
        lua.create_function(|_, _: LuaValue| Ok((true, Option::<String>::None)))?,
    )?;
    Ok((handle, None))
}

fn value_to_json(lua: &Lua, value: LuaValue, context: &Value) -> mlua::Result<Value> {
    match value {
        LuaValue::Nil => Ok(Value::Null),
        LuaValue::Boolean(v) => Ok(json!(v)),
        LuaValue::Integer(v) => Ok(json!(v)),
        LuaValue::Number(v) => Ok(json!(v)),
        LuaValue::String(v) => Ok(json!(v.to_string_lossy())),
        LuaValue::Function(_) => Ok(json!("<function>")),
        LuaValue::Table(table) => table_to_json(lua, table, context),
        _ => Ok(json!("<unsupported>")),
    }
}

fn table_to_json(lua: &Lua, table: Table, context: &Value) -> mlua::Result<Value> {
    table_to_json_with_rendered_view(lua, table, context, false)
}

fn contribution_table_to_json(lua: &Lua, table: Table, context: &Value) -> mlua::Result<Value> {
    table_to_json_with_rendered_view(lua, table, context, true)
}

fn table_to_json_with_rendered_view(
    lua: &Lua,
    table: Table,
    context: &Value,
    render_view: bool,
) -> mlua::Result<Value> {
    let mut array_entries: Vec<(usize, Value)> = Vec::new();
    let mut object = Map::new();
    let mut all_array = true;
    for pair in table.pairs::<LuaValue, LuaValue>() {
        let (key, value) = pair?;
        match key {
            LuaValue::Integer(index) if index > 0 => {
                array_entries.push((index as usize, value_to_json(lua, value, context)?));
            }
            LuaValue::String(key) => {
                all_array = false;
                let key = key.to_string_lossy();
                let json_value = if key == "widget" || (render_view && key == "view") {
                    widget_value_to_json(lua, value, context)?
                } else {
                    value_to_json(lua, value, context)?
                };
                object.insert(key, json_value);
            }
            other => {
                all_array = false;
                object.insert(format!("{other:?}"), value_to_json(lua, value, context)?);
            }
        }
    }
    if all_array && !array_entries.is_empty() {
        array_entries.sort_by_key(|(index, _)| *index);
        let expected: Vec<usize> = (1..=array_entries.len()).collect();
        let actual: Vec<usize> = array_entries.iter().map(|(index, _)| *index).collect();
        if actual == expected {
            return Ok(Value::Array(
                array_entries.into_iter().map(|(_, value)| value).collect(),
            ));
        }
    }
    for (index, value) in array_entries {
        object.insert(index.to_string(), value);
    }
    Ok(Value::Object(object))
}

fn widget_value_to_json(lua: &Lua, value: LuaValue, context: &Value) -> mlua::Result<Value> {
    match value {
        LuaValue::Function(f) => {
            let arg = lua.to_value(context)?;
            value_to_json(lua, f.call::<LuaValue>(arg)?, context)
        }
        other => value_to_json(lua, other, context),
    }
}

fn validate_widget_record(record: &Value, state: &Rc<RefCell<PreviewState>>) {
    let mut diagnostics = Vec::new();
    if let Some(widget) = record.get("widget") {
        validate_widget(widget, "widget", &mut diagnostics);
    } else if let Some(widget) = record.get("view") {
        validate_widget(widget, "view", &mut diagnostics);
    }
    state.borrow_mut().diagnostics.extend(diagnostics);
}

fn validate_widget(widget: &Value, path: &str, diagnostics: &mut Vec<Value>) {
    let Some(kind) = widget.get("kind").and_then(|v| v.as_str()) else {
        diagnostics.push(json!({
            "level": "error",
            "code": "preview.widget.missing_kind",
            "path": path,
            "message": "widget is missing string field `kind`",
        }));
        return;
    };
    if WIDGETS.get(kind).is_none() {
        diagnostics.push(json!({
            "level": "error",
            "code": "preview.widget.unknown_kind",
            "path": path,
            "message": format!("unknown widget kind `{kind}`"),
        }));
    }
    for child_key in ["child", "widget"] {
        if let Some(child) = widget.get(child_key).filter(|v| v.is_object()) {
            validate_widget(child, &format!("{path}.{child_key}"), diagnostics);
        }
    }
    for list_key in ["children", "tabs"] {
        if let Some(items) = widget.get(list_key).and_then(|v| v.as_array()) {
            for (index, item) in items.iter().enumerate() {
                let child = item.get("child").unwrap_or(item);
                if child.is_object() {
                    validate_widget(
                        child,
                        &format!("{path}.{list_key}[{}]", index + 1),
                        diagnostics,
                    );
                }
            }
        }
    }
}

pub(super) fn default_context() -> Value {
    json!({
        "version": 1,
        "type": "ScreenContext",
        "plugin_id": "<preview>",
        "generation_id": 1,
        "surface": "screen",
        "features": { "ui.context.typed@1": true },
        "theme": {
            "name": "leviathan",
            "colors": { "text_primary": "#e1e5f4", "text_secondary": "#9aa4bf" },
            "dimensions": {},
            "fonts": {}
        },
        "repository": {
            "is_open": true,
            "name": "preview-repo",
            "workdir_path": "/preview/repo",
            "current_branch_name": "main",
            "head_hash": "0000000",
            "default_remote_name": "origin",
            "has_remote": true
        },
        "tab": { "is_open": true, "id": 1, "path": "/preview/repo", "name": "preview-repo", "index": 0, "count": 1 },
        "selection": { "available": false, "kind": "", "selected_commit_id": null, "selected_file_path": null },
        "focus": { "surface": "screen" },
        "viewport": { "known": true, "width": 1200, "height": 800 },
        "payload": {}
    })
}

//! `leviathan.settings` — schema-backed plugin settings.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, Table, Value as LuaValue};
use serde_json::Value;

use crate::plugin::api::persist::PersistContext;
use crate::plugin::settings::{
    read_settings, validate_settings, with_defaults, write_settings, SettingsFile,
};

pub fn install(lua: &Lua, ctx: PersistContext, leviathan: &Table) -> mlua::Result<()> {
    let settings = lua.create_table()?;
    let path = ctx.storage.settings_path();
    let callbacks: Rc<RefCell<Vec<(Function, RegistryKey)>>> = Rc::new(RefCell::new(Vec::new()));

    let define_path = path.clone();
    settings.set(
        "define_schema",
        lua.create_function(move |lua_inner, schema: LuaValue| {
            let schema_json: Value = lua_inner
                .from_value(schema)
                .map_err(mlua::Error::external)?;
            let mut file = read_settings(&define_path).map_err(mlua::Error::external)?;
            let values = prune_to_schema(&schema_json, &with_defaults(&schema_json, &file.values));
            if let Err(errors) = validate_settings(&schema_json, &values) {
                return Ok((false, Some(errors.join("; "))));
            }
            file.schema = schema_json;
            file.values = values;
            write_settings(&define_path, &file).map_err(mlua::Error::external)?;
            Ok((true, Option::<String>::None))
        })?,
    )?;

    let get_path = path.clone();
    settings.set(
        "get",
        lua.create_function(move |lua_inner, ()| {
            let file = read_settings(&get_path).map_err(mlua::Error::external)?;
            let values = with_defaults(&file.schema, &file.values);
            lua_inner.to_value(&values)
        })?,
    )?;

    let set_path = path.clone();
    let set_callbacks = Rc::clone(&callbacks);
    settings.set(
        "set",
        lua.create_function(move |lua_inner, values: LuaValue| {
            let incoming: Value = lua_inner
                .from_value(values)
                .map_err(mlua::Error::external)?;
            let mut file = read_settings(&set_path).map_err(mlua::Error::external)?;
            let current = with_defaults(&file.schema, &file.values);
            let proposed = merge_object(current, incoming);
            if let Err(errors) = validate_settings(&file.schema, &proposed) {
                return Ok((false, Some(errors.join("; "))));
            }
            file.values = proposed.clone();
            write_settings(&set_path, &file).map_err(mlua::Error::external)?;
            let callback_arg = lua_inner.to_value(&proposed)?;
            for (_, key) in set_callbacks.borrow().iter() {
                let callback = lua_inner.registry_value::<Function>(key)?;
                callback.call::<()>(callback_arg.clone())?;
            }
            Ok((true, Option::<String>::None))
        })?,
    )?;

    let on_change_callbacks = Rc::clone(&callbacks);
    settings.set(
        "on_change",
        lua.create_function(move |lua_inner, callback: Function| {
            let key = lua_inner.create_registry_value(&callback)?;
            let mut callbacks = on_change_callbacks.borrow_mut();
            match callbacks
                .iter_mut()
                .find(|(existing, _)| *existing == callback)
            {
                Some(entry) => entry.1 = key,
                None => callbacks.push((callback, key)),
            }
            Ok(())
        })?,
    )?;

    leviathan.set("settings", settings)?;
    Ok(())
}

fn merge_object(mut current: Value, incoming: Value) -> Value {
    if let (Some(current), Some(incoming)) = (current.as_object_mut(), incoming.as_object()) {
        for (key, value) in incoming {
            current.insert(key.clone(), value.clone());
        }
    } else {
        current = incoming;
    }
    current
}

fn prune_to_schema(schema: &Value, values: &Value) -> Value {
    let Some(schema) = schema.as_object() else {
        return SettingsFile::default().values;
    };
    let Some(values) = values.as_object() else {
        return serde_json::json!({});
    };
    let mut out = serde_json::Map::new();
    for key in schema.keys() {
        if let Some(value) = values.get(key) {
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::storage::PluginStoragePaths;

    fn install_settings(lua: &Lua, dir: &std::path::Path) {
        let leviathan = lua.create_table().unwrap();
        let storage = PluginStoragePaths {
            plugin_root: dir.to_path_buf(),
            state_dir: dir.to_path_buf(),
            config_dir: dir.to_path_buf(),
            cache_dir: dir.to_path_buf(),
            secrets_dir: dir.to_path_buf(),
        };
        install(lua, PersistContext { storage }, &leviathan).unwrap();
        lua.globals().set("leviathan", leviathan).unwrap();
    }

    #[test]
    fn on_change_does_not_double_register_same_callback() {
        let tmp = tempfile::tempdir().unwrap();
        let lua = Lua::new();
        install_settings(&lua, tmp.path());
        let fired: i64 = lua
            .load(
                r#"
                leviathan.settings.define_schema({
                  n = { type = "integer", default = 0 },
                })
                _G.count = 0
                local cb = function() _G.count = _G.count + 1 end
                leviathan.settings.on_change(cb)
                leviathan.settings.on_change(cb)
                leviathan.settings.set({ n = 1 })
                return _G.count
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(fired, 1);
    }

    #[test]
    fn on_change_fires_each_distinct_callback() {
        let tmp = tempfile::tempdir().unwrap();
        let lua = Lua::new();
        install_settings(&lua, tmp.path());
        let fired: i64 = lua
            .load(
                r#"
                leviathan.settings.define_schema({
                  n = { type = "integer", default = 0 },
                })
                _G.count = 0
                leviathan.settings.on_change(function() _G.count = _G.count + 1 end)
                leviathan.settings.on_change(function() _G.count = _G.count + 1 end)
                leviathan.settings.set({ n = 1 })
                return _G.count
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(fired, 2);
    }
}

//! leviathan.persist.open(name, opts) -> store userdata
//!
//! `name` is a plugin-local store identifier; opens (or creates)
//! `<state_dir>/<name>.json` under the plugin's per-plugin state-dir
//! root. `opts.version` defaults to 1; `opts.migrations` is an array of
//! `{ from, to, transform = function(old) ... return new end }` tables.
//!
//! Migrations are run inline at open-time by calling each transform's Lua
//! function, since `Migration::transform` is a `Fn(Value)->Value` closure
//! that can't capture a `&Lua` reference. The result is then handed to
//! `PersistStore::from_loaded`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use mlua::{Function, Lua, LuaSerdeExt, Table, UserData, UserDataMethods, Value as LuaValue};

use crate::plugin::persist::PersistStore;

pub struct PersistContext {
    pub state_dir: PathBuf,
}

struct PersistStoreUserdata {
    inner: Rc<RefCell<PersistStore>>,
}

impl UserData for PersistStoreUserdata {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get", |lua, this, key: String| {
            match this.inner.borrow().get_value(&key) {
                Some(v) => lua.to_value(&v),
                None => Ok(LuaValue::Nil),
            }
        });
        methods.add_method("set", |lua, this, (key, value): (String, LuaValue)| {
            let json: serde_json::Value =
                lua.from_value(value).map_err(mlua::Error::external)?;
            this.inner
                .borrow_mut()
                .set_value(&key, json)
                .map_err(mlua::Error::external)?;
            Ok(())
        });
        methods.add_method("version", |_, this, ()| Ok(this.inner.borrow().version()));
    }
}

pub fn install(lua: &Lua, ctx: PersistContext, leviathan: &Table) -> mlua::Result<()> {
    let persist = lua.create_table()?;
    let state_dir = ctx.state_dir.clone();

    persist.set(
        "open",
        lua.create_function(move |lua_inner, (name, opts): (String, Option<Table>)| {
            let path = state_dir.join(format!("{name}.json"));
            let target_version = opts
                .as_ref()
                .and_then(|o| o.get::<Option<u32>>("version").ok().flatten())
                .unwrap_or(1);

            let mut lua_migrations: Vec<(u32, u32, Function)> = Vec::new();
            if let Some(opts) = opts {
                if let Ok(Some(migs_tbl)) = opts.get::<Option<Table>>("migrations") {
                    for pair in migs_tbl.sequence_values::<Table>() {
                        let m = pair?;
                        lua_migrations.push((
                            m.get("from")?,
                            m.get("to")?,
                            m.get("transform")?,
                        ));
                    }
                }
            }

            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(mlua::Error::external)?;
            }
            let (mut current_version, mut current_data): (u32, serde_json::Value) =
                if path.exists() {
                    let raw = std::fs::read_to_string(&path).map_err(mlua::Error::external)?;
                    let file: serde_json::Value =
                        serde_json::from_str(&raw).map_err(mlua::Error::external)?;
                    let v = file
                        .get("version")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(target_version as u64) as u32;
                    let d = file
                        .get("data")
                        .cloned()
                        .unwrap_or(serde_json::json!({}));
                    (v, d)
                } else {
                    (target_version, serde_json::json!({}))
                };

            while current_version < target_version {
                let mig = lua_migrations
                    .iter()
                    .find(|(from, _, _)| *from == current_version)
                    .ok_or_else(|| {
                        mlua::Error::external(format!(
                            "no migration from {current_version} to {target_version}"
                        ))
                    })?;
                let lv: LuaValue = lua_inner.to_value(&current_data)?;
                let result: LuaValue = mig.2.call(lv)?;
                current_data = lua_inner.from_value(result)?;
                current_version = mig.1;
            }

            let file =
                serde_json::json!({ "version": current_version, "data": current_data });
            let raw = serde_json::to_string_pretty(&file).map_err(mlua::Error::external)?;
            std::fs::write(&path, raw).map_err(mlua::Error::external)?;

            let store = PersistStore::from_loaded(path, current_version, current_data);
            Ok(PersistStoreUserdata {
                inner: Rc::new(RefCell::new(store)),
            })
        })?,
    )?;

    leviathan.set("persist", persist)?;
    Ok(())
}

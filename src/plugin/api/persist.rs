//! leviathan.persist.open(name, opts) -> store userdata
//!
//! `name` is a plugin-local store identifier; opens (or creates)
//! `<state_dir>/<name>.json` under the plugin's per-plugin state-dir
//! root. `opts.version` defaults to 1; `opts.surface` defaults to
//! `"state"` and can select `"config"`, `"cache"`, or per-repo
//! `"repo"` state. `opts.migrations` is an array of `{ from, to,
//! transform = function(old) ... return new end }` tables.
//!
//! Migrations are run inline at open-time by calling each transform's Lua
//! function, since `Migration::transform` is a `Fn(Value)->Value` closure
//! that can't capture a `&Lua` reference. The result is then handed to
//! `PersistStore::from_loaded`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use mlua::{
    Function, Lua, LuaSerdeExt, Table, UserData, UserDataMethods, Value as LuaValue, Variadic,
};

use crate::plugin::persist::{
    load_store_file_or_default, write_store_file_atomic, PersistStore, StoreFile,
};
use crate::plugin::storage::{PluginStoragePaths, StorageSurface};

#[derive(Clone)]
pub struct PersistContext {
    pub storage: PluginStoragePaths,
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
            let json: serde_json::Value = lua.from_value(value).map_err(mlua::Error::external)?;
            this.inner
                .borrow_mut()
                .set_value(&key, json)
                .map_err(mlua::Error::external)?;
            Ok(())
        });
        methods.add_method("version", |_, this, ()| Ok(this.inner.borrow().version()));
        methods.add_method("delete", |_, this, key: String| {
            this.inner
                .borrow_mut()
                .delete_value(&key)
                .map_err(mlua::Error::external)?;
            Ok(())
        });
    }
}

struct PersistTransactionUserdata {
    data: Rc<RefCell<serde_json::Value>>,
}

impl UserData for PersistTransactionUserdata {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get", |lua, this, key: String| {
            match this.data.borrow().get(&key).cloned() {
                Some(v) => lua.to_value(&v),
                None => Ok(LuaValue::Nil),
            }
        });
        methods.add_method("set", |lua, this, (key, value): (String, LuaValue)| {
            let json: serde_json::Value = lua.from_value(value).map_err(mlua::Error::external)?;
            if let serde_json::Value::Object(map) = &mut *this.data.borrow_mut() {
                map.insert(key, json);
            } else {
                *this.data.borrow_mut() = serde_json::json!({ key: json });
            }
            Ok(())
        });
        methods.add_method("delete", |_, this, key: String| {
            if let serde_json::Value::Object(map) = &mut *this.data.borrow_mut() {
                map.remove(&key);
            }
            Ok(())
        });
    }
}

struct OpenPlan {
    path: PathBuf,
    store_name: String,
    target_version: u32,
    lua_migrations: Vec<(u32, u32, Function)>,
}

pub fn install(lua: &Lua, ctx: PersistContext, leviathan: &Table) -> mlua::Result<()> {
    let persist = lua.create_table()?;
    let open_ctx = ctx.clone();

    persist.set(
        "open",
        lua.create_function(move |lua_inner, (name, opts): (String, Option<Table>)| {
            let plan = build_open_plan(lua_inner, &open_ctx, name, opts)?;
            let loaded = load_and_migrate(lua_inner, &open_ctx, &plan)?;
            let file = StoreFile {
                version: loaded.version,
                data: loaded.data.clone(),
            };
            write_store_file_atomic(&plan.path, &file).map_err(mlua::Error::external)?;

            let store = PersistStore::from_loaded(plan.path, loaded.version, loaded.data);
            Ok(PersistStoreUserdata {
                inner: Rc::new(RefCell::new(store)),
            })
        })?,
    )?;

    let tx_ctx = ctx.clone();
    persist.set(
        "transaction",
        lua.create_function(move |lua_inner, args: Variadic<LuaValue>| {
            let (opts, callback) = parse_transaction_args(args)?;
            let name = opts
                .as_ref()
                .and_then(|t| t.get::<Option<String>>("name").ok().flatten())
                .unwrap_or_else(|| "default".to_string());
            let plan = build_open_plan(lua_inner, &tx_ctx, name, opts)?;
            let loaded = load_and_migrate(lua_inner, &tx_ctx, &plan)?;
            let data = Rc::new(RefCell::new(loaded.data));
            let tx = PersistTransactionUserdata {
                data: Rc::clone(&data),
            };

            callback.call::<()>(tx)?;

            let file = StoreFile {
                version: loaded.version,
                data: data.borrow().clone(),
            };
            write_store_file_atomic(&plan.path, &file).map_err(mlua::Error::external)?;
            Ok(true)
        })?,
    )?;

    leviathan.set("persist", persist)?;
    Ok(())
}

struct LoadedStore {
    version: u32,
    data: serde_json::Value,
}

fn build_open_plan(
    _lua: &Lua,
    ctx: &PersistContext,
    name: String,
    opts: Option<Table>,
) -> mlua::Result<OpenPlan> {
    let target_version = opts
        .as_ref()
        .and_then(|o| o.get::<Option<u32>>("version").ok().flatten())
        .unwrap_or(1);
    let surface = opts
        .as_ref()
        .and_then(|o| o.get::<Option<String>>("surface").ok().flatten())
        .map(|s| StorageSurface::parse(&s))
        .transpose()
        .map_err(mlua::Error::external)?
        .unwrap_or(StorageSurface::State);
    let repo = opts
        .as_ref()
        .and_then(|o| o.get::<Option<String>>("repo").ok().flatten());
    let path = ctx
        .storage
        .path_for_store(surface, &name, repo.as_deref())
        .map_err(mlua::Error::external)?;

    let mut lua_migrations: Vec<(u32, u32, Function)> = Vec::new();
    if let Some(opts) = opts {
        if let Ok(Some(migs_tbl)) = opts.get::<Option<Table>>("migrations") {
            for pair in migs_tbl.sequence_values::<Table>() {
                let m = pair?;
                lua_migrations.push((m.get("from")?, m.get("to")?, m.get("transform")?));
            }
        }
    }

    Ok(OpenPlan {
        path,
        store_name: name,
        target_version,
        lua_migrations,
    })
}

fn parse_transaction_args(args: Variadic<LuaValue>) -> mlua::Result<(Option<Table>, Function)> {
    match args.as_slice() {
        [LuaValue::Function(f)] => Ok((None, f.clone())),
        [LuaValue::Table(t), LuaValue::Function(f)] => Ok((Some(t.clone()), f.clone())),
        _ => Err(mlua::Error::external(
            "leviathan.persist.transaction expects function or opts, function",
        )),
    }
}

fn load_and_migrate(lua: &Lua, ctx: &PersistContext, plan: &OpenPlan) -> mlua::Result<LoadedStore> {
    let mut file = load_store_file_or_default(&plan.path, plan.target_version)
        .map_err(mlua::Error::external)?;

    while file.version < plan.target_version {
        if let Some((_, to, func)) = plan
            .lua_migrations
            .iter()
            .find(|(from, to, _)| *from == file.version && *to <= plan.target_version)
        {
            let lv: LuaValue = lua.to_value(&file.data)?;
            let result: LuaValue = func.call(lv)?;
            file.data = lua.from_value(result)?;
            if *to <= file.version {
                return Err(mlua::Error::external(format!(
                    "migration {}->{} did not advance version",
                    file.version, to
                )));
            }
            file.version = *to;
            continue;
        }

        let Some(file_migration) =
            find_file_migration(ctx, &plan.store_name, file.version, plan.target_version)
        else {
            return Err(mlua::Error::external(format!(
                "no migration from {} to {}",
                file.version, plan.target_version
            )));
        };
        let src = std::fs::read_to_string(&file_migration.path).map_err(mlua::Error::external)?;
        let chunk_name = file_migration.path.display().to_string();
        let value: LuaValue = lua.load(&src).set_name(chunk_name.clone()).eval()?;
        let func = match value {
            LuaValue::Function(f) => f,
            LuaValue::Table(t) => t.get::<Function>("migrate")?,
            other => {
                return Err(mlua::Error::external(format!(
                    "migration file {chunk_name} returned {}, expected function or table.migrate",
                    other.type_name()
                )));
            }
        };
        let old: LuaValue = lua.to_value(&file.data)?;
        let result: LuaValue = func.call(old)?;
        file.data = lua.from_value(result)?;
        file.version = file_migration.to;
    }

    if file.version != plan.target_version {
        return Err(mlua::Error::external(format!(
            "store version {} does not match requested {}",
            file.version, plan.target_version
        )));
    }

    Ok(LoadedStore {
        version: file.version,
        data: file.data,
    })
}

struct FileMigration {
    path: PathBuf,
    to: u32,
}

fn find_file_migration(
    ctx: &PersistContext,
    store_name: &str,
    from: u32,
    target: u32,
) -> Option<FileMigration> {
    let migrations_root = ctx.storage.plugin_root.join("migrations");
    let direct_candidates = [
        migrations_root
            .join(store_name)
            .join(format!("{from}_to_{target}.lua")),
        migrations_root
            .join(store_name)
            .join(format!("v{from}_to_v{target}.lua")),
        migrations_root.join(format!("{store_name}_{from}_to_{target}.lua")),
        migrations_root.join(format!("{store_name}_v{from}_to_v{target}.lua")),
    ];
    for path in direct_candidates {
        if path.is_file() {
            return Some(FileMigration { path, to: target });
        }
    }

    let dir = migrations_root.join(store_name);
    let entries = std::fs::read_dir(dir).ok()?;
    let mut matches = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("lua") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some((candidate_from, candidate_to)) = parse_migration_stem(stem) else {
            continue;
        };
        if candidate_from == from && candidate_to > from && candidate_to <= target {
            matches.push(FileMigration {
                path,
                to: candidate_to,
            });
        }
    }
    matches.sort_by(|a, b| a.to.cmp(&b.to).then(a.path.cmp(&b.path)));
    matches.into_iter().next()
}

fn parse_migration_stem(stem: &str) -> Option<(u32, u32)> {
    let stem = stem.strip_prefix('v').unwrap_or(stem);
    let (from, to) = stem.split_once("_to_")?;
    let to = to.strip_prefix('v').unwrap_or(to);
    Some((from.parse().ok()?, to.parse().ok()?))
}

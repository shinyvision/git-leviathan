//! `leviathan.secrets` — plugin-local secret metadata and values.

use mlua::{Lua, Table};

use crate::plugin::api::persist::PersistContext;
use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::secrets::{FileSecretStore, SecretBackend};

pub fn install(
    lua: &Lua,
    ctx: PersistContext,
    guard: std::rc::Rc<CapabilityGuard>,
    leviathan: &Table,
) -> mlua::Result<()> {
    let secrets = lua.create_table()?;
    let store = FileSecretStore::new(ctx.storage.secrets_dir);

    let set_store = store.clone();
    let set_guard = std::rc::Rc::clone(&guard);
    secrets.set(
        "set",
        lua.create_function(move |_, (key, value): (String, String)| {
            set_guard
                .check_named("credentials")
                .map_err(mlua::Error::external)?;
            set_store.set(&key, &value).map_err(mlua::Error::external)?;
            Ok(())
        })?,
    )?;

    let get_store = store.clone();
    let get_guard = std::rc::Rc::clone(&guard);
    secrets.set(
        "get",
        lua.create_function(move |_, key: String| {
            get_guard
                .check_named("credentials")
                .map_err(mlua::Error::external)?;
            get_store.get(&key).map_err(mlua::Error::external)
        })?,
    )?;

    let delete_store = store.clone();
    let delete_guard = std::rc::Rc::clone(&guard);
    secrets.set(
        "delete",
        lua.create_function(move |_, key: String| {
            delete_guard
                .check_named("credentials")
                .map_err(mlua::Error::external)?;
            delete_store.delete(&key).map_err(mlua::Error::external)?;
            Ok(())
        })?,
    )?;

    let list_guard = guard;
    secrets.set(
        "list",
        lua.create_function(move |lua_inner, ()| {
            list_guard
                .check_named("credentials")
                .map_err(mlua::Error::external)?;
            let keys = store.list().map_err(mlua::Error::external)?;
            lua_inner.create_sequence_from(keys)
        })?,
    )?;

    leviathan.set("secrets", secrets)?;
    Ok(())
}

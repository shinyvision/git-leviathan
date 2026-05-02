//! User commands. `leviathan.api.create_user_command(name, fn)`
//! registers a named coroutine-driven entry point. The host's
//! `invoke_user_command(plugin_id, name)` resumes the wrapping thread
//! once; if it yielded, it's parked in the per-plugin
//! [`DeferredQueue::coroutines`] bucket and resumed on subsequent ticks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Function, Lua, RegistryKey, Table};

#[derive(Default)]
pub struct UserCommands {
    pub commands: HashMap<String, RegistryKey>,
}

impl UserCommands {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn install(
    lua: &Lua,
    commands: Rc<RefCell<UserCommands>>,
    api: &Table,
) -> mlua::Result<()> {
    api.set(
        "create_user_command",
        lua.create_function(move |lua_inner, (name, f): (String, Function)| {
            let key = lua_inner.create_registry_value(f)?;
            commands.borrow_mut().commands.insert(name, key);
            Ok(())
        })?,
    )?;
    Ok(())
}

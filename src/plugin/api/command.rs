//! User commands.
//!
//! Two surfaces:
//!
//! - `leviathan.api.create_user_command(name, fn)` — Phase 1 v1 shim
//!   that registers a no-args, non-destructive, `global`-context
//!   command. Kept verbatim so bundled plugins keep working.
//! - `leviathan.command.{create, invoke, list}` — Phase 8 typed
//!   surface. `create` captures a [`crate::plugin::commands::RawCommand`]
//!   into the per-plugin `BuildState`; the host moves it into the
//!   shared command registry once init completes. `invoke` and
//!   `list` borrow the live registry through the shared
//!   [`crate::plugin::commands::CommandDispatchEnv`] handle so every
//!   entry point routes through the same dispatcher.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, Table, Value as LuaValue};

use crate::plugin::commands::{
    dispatch_command, CommandDispatchEnv, InvokeOutcome, RawCommand, RawCommandArg,
};
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};

use super::BuildState;

/// Compatibility shim. v1 plugins call
/// `leviathan.api.create_user_command(name, fn)`; we route those
/// through the same Phase 8 path by emitting a [`RawCommand`] with
/// no args, no capabilities, and `context = "global"`. This means
/// every command — host, plugin, legacy — lives in the same registry.
#[derive(Default)]
pub struct UserCommands {
    /// Lua registry-key index keyed by command name. Kept around for
    /// the legacy `host.invoke_user_command` entry point that pre-dates
    /// the unified registry; the entries here are also mirrored as
    /// `RawCommand` rows on `BuildState.commands` so the host
    /// installs them into the unified registry.
    pub commands: HashMap<String, RegistryKey>,
}

pub fn install(
    lua: &Lua,
    commands: Rc<RefCell<UserCommands>>,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    api: &Table,
    leviathan: &Table,
    dispatch: CommandDispatchEnv,
) -> mlua::Result<()> {
    install_legacy_create_user_command(
        lua,
        Rc::clone(&commands),
        Rc::clone(&build),
        ledger.clone(),
        api,
    )?;

    let command_tbl = lua.create_table()?;
    install_command_create(lua, Rc::clone(&build), ledger, &command_tbl)?;
    install_command_invoke(lua, dispatch.clone(), &command_tbl)?;
    install_command_list(lua, dispatch, &command_tbl)?;

    leviathan.set("command", command_tbl)?;
    Ok(())
}

fn install_legacy_create_user_command(
    lua: &Lua,
    commands: Rc<RefCell<UserCommands>>,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    api: &Table,
) -> mlua::Result<()> {
    api.set(
        "create_user_command",
        lua.create_function(move |lua_inner, (name, f): (String, Function)| {
            let key = lua_inner.create_registry_value(f.clone())?;
            let source = ResourceLedger::source_location(lua_inner);
            ledger.remove_by_kind_handle(PluginResourceKind::Command, &name);
            ledger.remove_by_kind_handle(
                PluginResourceKind::LuaRegistryKey,
                &format!("command:{name}"),
            );
            ledger.record(PluginResourceKind::Command, name.clone(), source.clone());
            ledger.record(
                PluginResourceKind::LuaRegistryKey,
                format!("command:{name}"),
                source.clone(),
            );
            commands.borrow_mut().commands.insert(name.clone(), key);
            // Also record the v1 shim into the typed pending list so
            // the host installs it into the unified registry as a
            // global, no-args, non-destructive command — preserving
            // every property the v1 contract promised.
            let typed_key = lua_inner.create_registry_value(f)?;
            build.borrow_mut().commands.push(RawCommand {
                name,
                title: None,
                description: None,
                context: None,
                destructive: false,
                capabilities: Vec::new(),
                args: Vec::new(),
                callback: typed_key,
                source_location: source,
            });
            Ok(())
        })?,
    )?;
    Ok(())
}

fn install_command_create(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    command: &Table,
) -> mlua::Result<()> {
    command.set(
        "create",
        lua.create_function(move |lua_inner, (name, spec): (String, Table)| {
            let title: Option<String> = spec.get::<Option<String>>("title")?;
            let description: Option<String> = spec.get::<Option<String>>("description")?;
            let context: Option<String> = spec.get::<Option<String>>("context")?;
            let destructive: bool = spec.get::<Option<bool>>("destructive")?.unwrap_or(false);
            let capabilities: Vec<String> = match spec.get::<Option<Table>>("capabilities")? {
                Some(t) => {
                    let mut v = Vec::new();
                    for value in t.sequence_values::<String>() {
                        v.push(value?);
                    }
                    v
                }
                None => Vec::new(),
            };
            let args: Vec<RawCommandArg> = match spec.get::<Option<Table>>("args")? {
                Some(t) => decode_args(lua_inner, t)?,
                None => Vec::new(),
            };
            let callback: Function = spec.get("run")?;
            let key = lua_inner.create_registry_value(callback)?;
            let source = ResourceLedger::source_location(lua_inner);
            ledger.remove_by_kind_handle(PluginResourceKind::Command, &name);
            ledger.remove_by_kind_handle(
                PluginResourceKind::LuaRegistryKey,
                &format!("command:{name}"),
            );
            ledger.record(PluginResourceKind::Command, name.clone(), source.clone());
            ledger.record(
                PluginResourceKind::LuaRegistryKey,
                format!("command:{name}"),
                source.clone(),
            );
            build.borrow_mut().commands.push(RawCommand {
                name,
                title,
                description,
                context,
                destructive,
                capabilities,
                args,
                callback: key,
                source_location: source,
            });
            Ok(())
        })?,
    )?;
    Ok(())
}

fn decode_args(lua: &Lua, args: Table) -> mlua::Result<Vec<RawCommandArg>> {
    let mut out = Vec::new();
    for entry in args.sequence_values::<Table>() {
        let entry = entry?;
        let name: String = entry.get("name")?;
        let ty: String = entry.get("type")?;
        let required: bool = entry.get::<Option<bool>>("required")?.unwrap_or(false);
        let doc: Option<String> = entry.get::<Option<String>>("doc")?;
        let default = match entry.get::<Option<LuaValue>>("default")? {
            Some(LuaValue::Nil) | None => None,
            Some(v) => Some(lua.from_value::<serde_json::Value>(v)?),
        };
        out.push(RawCommandArg {
            name,
            ty,
            required,
            default,
            doc,
        });
    }
    Ok(out)
}

fn install_command_invoke(
    lua: &Lua,
    dispatch: CommandDispatchEnv,
    command: &Table,
) -> mlua::Result<()> {
    command.set(
        "invoke",
        lua.create_function(move |lua_inner, (name, args): (String, Option<Table>)| {
            let json_args = match args {
                Some(t) => lua_inner.from_value::<serde_json::Value>(LuaValue::Table(t))?,
                None => serde_json::Value::Null,
            };
            let outcome = dispatch_command(&dispatch, &name, json_args);
            Ok(matches!(outcome, InvokeOutcome::Ok))
        })?,
    )?;
    Ok(())
}

fn install_command_list(
    lua: &Lua,
    dispatch: CommandDispatchEnv,
    command: &Table,
) -> mlua::Result<()> {
    command.set(
        "list",
        lua.create_function(move |lua_inner, ()| {
            let summaries = dispatch.commands.borrow().summaries();
            let json: Vec<serde_json::Value> = summaries
                .into_iter()
                .map(|s| {
                    serde_json::json!({
                        "name": s.name,
                        "title": s.title,
                        "description": s.description,
                        "plugin_id": s.plugin_id,
                        "context": s.context,
                        "destructive": s.destructive,
                        "capabilities": s.capabilities,
                    })
                })
                .collect();
            lua_inner.to_value(&json)
        })?,
    )?;
    Ok(())
}

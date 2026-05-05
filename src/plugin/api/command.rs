//! User commands.
//!
//! `leviathan.command.{create, invoke, list}` captures typed command
//! registrations into the per-plugin `BuildState`; the host moves them
//! into the shared command registry once init completes. `invoke` and
//! `list` borrow the live registry through the shared
//! [`crate::plugin::commands::CommandDispatchEnv`] handle so every
//! entry point routes through the same dispatcher.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, Table, Value as LuaValue};

use crate::plugin::commands::{
    dispatch_command_as, CommandDispatchEnv, CommandInvocationCaller, InvokeOutcome, RawCommand,
    RawCommandArg,
};
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};
use crate::plugin::{capabilities::CapabilityGuard, resources::PluginId};

use super::BuildState;

#[derive(Default)]
pub struct UserCommands {
    /// Lua registry-key index keyed by command name. Kept around for
    /// `host.invoke_user_command`, which runs plugin command callbacks
    /// directly in tests and helpers.
    pub commands: HashMap<String, RegistryKey>,
}

pub struct InstallCtx {
    pub commands: Rc<RefCell<UserCommands>>,
    pub build: Rc<RefCell<BuildState>>,
    pub ledger: ResourceLedger,
    pub dispatch: CommandDispatchEnv,
    pub guard: Rc<CapabilityGuard>,
    pub plugin_id: PluginId,
}

pub fn install(lua: &Lua, leviathan: &Table, ctx: InstallCtx) -> mlua::Result<()> {
    let command_tbl = lua.create_table()?;
    install_command_create(
        lua,
        ctx.commands,
        Rc::clone(&ctx.build),
        ctx.ledger,
        &command_tbl,
    )?;
    install_command_invoke(
        lua,
        ctx.dispatch.clone(),
        ctx.guard,
        ctx.plugin_id,
        &command_tbl,
    )?;
    install_command_list(lua, ctx.dispatch, &command_tbl)?;

    leviathan.set("command", command_tbl)?;
    Ok(())
}

fn install_command_create(
    lua: &Lua,
    commands: Rc<RefCell<UserCommands>>,
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
            let direct_key = lua_inner.create_registry_value(callback.clone())?;
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
            commands
                .borrow_mut()
                .commands
                .insert(name.clone(), direct_key);
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
    guard: Rc<CapabilityGuard>,
    plugin_id: PluginId,
    command: &Table,
) -> mlua::Result<()> {
    command.set(
        "invoke",
        lua.create_function(move |lua_inner, (name, args): (String, Option<Table>)| {
            let json_args = match args {
                Some(t) => lua_inner.from_value::<serde_json::Value>(LuaValue::Table(t))?,
                None => serde_json::Value::Null,
            };
            let caller = CommandInvocationCaller {
                plugin_id: plugin_id.as_str().to_string(),
                capability_guard: Some(Rc::clone(&guard)),
            };
            let outcome = dispatch_command_as(&dispatch, Some(caller), &name, json_args);
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
                    let args: Vec<serde_json::Value> = s
                        .args
                        .into_iter()
                        .map(|a| {
                            let mut arg = serde_json::Map::new();
                            arg.insert("name".to_string(), serde_json::Value::String(a.name));
                            arg.insert("type".to_string(), serde_json::Value::String(a.ty));
                            arg.insert("required".to_string(), serde_json::Value::Bool(a.required));
                            if let Some(default) = a.default {
                                arg.insert("default".to_string(), default);
                            }
                            arg.insert("doc".to_string(), serde_json::Value::String(a.doc));
                            serde_json::Value::Object(arg)
                        })
                        .collect();
                    serde_json::json!({
                        "name": s.name,
                        "title": s.title,
                        "description": s.description,
                        "plugin_id": s.plugin_id,
                        "context": s.context,
                        "destructive": s.destructive,
                        "capabilities": s.capabilities,
                        "plugin_invocation_capabilities": s.plugin_invocation_capabilities,
                        "enabled": s.enabled,
                        "disabled_reason": s.disabled_reason,
                        "keymap_eligible": s.keymap_eligible,
                        "palette_visible": s.palette_visible,
                        "hooks": {
                            "before": s.hooks.before,
                            "after": s.hooks.after,
                            "veto": s.hooks.veto,
                            "replace": s.hooks.replace,
                        },
                        "args": args,
                    })
                })
                .collect();
            lua_inner.to_value(&json)
        })?,
    )?;
    Ok(())
}

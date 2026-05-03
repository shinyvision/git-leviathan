//! `leviathan.health.register(fn)` — plugin authors register a single
//! health-check callback. The host invokes it during `:checkhealth` /
//! `PluginHost::run_health_checks()` with a fresh `HealthContext`
//! userdata; the callback calls `ctx:ok / :info / :warn / :error` to
//! contribute items to the aggregated report.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, RegistryKey, Table, UserData, UserDataMethods};

use crate::plugin::resources::{PluginResourceKind, ResourceLedger};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Ok,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct HealthItem {
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct PluginHealth {
    pub plugin_id: String,
    pub items: Vec<HealthItem>,
}

#[derive(Debug, Clone, Default)]
pub struct HealthReport {
    pub plugins: Vec<PluginHealth>,
}

impl HealthReport {
    pub fn for_plugin(&self, id: &str) -> Option<&PluginHealth> {
        self.plugins.iter().find(|p| p.plugin_id == id)
    }
}

/// Userdata exposed to the Lua callback. `Rc<RefCell<Vec<HealthItem>>>`
/// is shared with the host's collection sink so emitted items flow back
/// into the aggregated report without an extra round-trip through Lua.
pub struct HealthContext {
    pub items: Rc<RefCell<Vec<HealthItem>>>,
}

impl UserData for HealthContext {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("ok", |_, this, msg: String| {
            this.items.borrow_mut().push(HealthItem {
                severity: Severity::Ok,
                message: msg,
            });
            Ok(())
        });
        methods.add_method("info", |_, this, msg: String| {
            this.items.borrow_mut().push(HealthItem {
                severity: Severity::Info,
                message: msg,
            });
            Ok(())
        });
        methods.add_method("warn", |_, this, msg: String| {
            this.items.borrow_mut().push(HealthItem {
                severity: Severity::Warn,
                message: msg,
            });
            Ok(())
        });
        methods.add_method("error", |_, this, msg: String| {
            this.items.borrow_mut().push(HealthItem {
                severity: Severity::Error,
                message: msg,
            });
            Ok(())
        });
    }
}

/// Per-plugin sink populated at install time, drained by the host's
/// `run_health_checks`. Each entry is a Lua function the host invokes
/// with a fresh `HealthContext`.
pub struct HealthCheckRegistration {
    pub callback: RegistryKey,
}

pub fn install(
    lua: &Lua,
    sink: Rc<RefCell<Vec<HealthCheckRegistration>>>,
    ledger: ResourceLedger,
    leviathan: &Table,
) -> mlua::Result<()> {
    let health = lua.create_table()?;
    health.set(
        "register",
        lua.create_function(move |lua_inner, f: Function| {
            let key = lua_inner.create_registry_value(f)?;
            let source = ResourceLedger::source_location(lua_inner);
            let resource_id = ledger.record(
                PluginResourceKind::HealthCheck,
                "health.register",
                source.clone(),
            );
            ledger.record(
                PluginResourceKind::LuaRegistryKey,
                format!("health:{resource_id}"),
                source,
            );
            sink.borrow_mut()
                .push(HealthCheckRegistration { callback: key });
            Ok(())
        })?,
    )?;
    leviathan.set("health", health)?;
    Ok(())
}

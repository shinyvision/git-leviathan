//! `leviathan.timer.after(ms, fn)` and `leviathan.timer.every(ms, fn)`.
//! Returns a handle with a `:cancel()` method.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Table, UserData, UserDataMethods};

use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::resources::{GenerationId, PluginId, PluginResourceKind, ResourceLedger};
use crate::plugin::timers::{PluginTimerCallbacks, TimerId, TimerKind, TimerRegistry};

pub struct LuaTimerHandle {
    pub timer_id: TimerId,
    pub registry: TimerRegistry,
    pub callbacks: Rc<RefCell<PluginTimerCallbacks>>,
}

impl UserData for LuaTimerHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("cancel", |_, this, ()| {
            this.registry.cancel(this.timer_id);
            this.callbacks.borrow_mut().remove(this.timer_id);
            Ok(())
        });
        methods.add_method("id", |_, this, ()| Ok(this.timer_id.get()));
    }
}

pub fn install(
    lua: &Lua,
    leviathan: &Table,
    guard: Rc<CapabilityGuard>,
    ledger: ResourceLedger,
    registry: TimerRegistry,
    callbacks: Rc<RefCell<PluginTimerCallbacks>>,
    plugin_id: PluginId,
    generation_id: GenerationId,
) -> mlua::Result<()> {
    let timer_tbl = lua.create_table()?;

    let plugin_id_after = plugin_id.clone();
    let registry_after = registry.clone();
    let ledger_after = ledger.clone();
    let callbacks_after = Rc::clone(&callbacks);
    let guard_after = Rc::clone(&guard);
    timer_tbl.set(
        "after",
        lua.create_function(move |lua_inner, (ms, f): (u64, Function)| {
            guard_after
                .check_named("timer:create")
                .map_err(mlua::Error::external)?;
            let timer_id = registry_after.allocate();
            let source = ResourceLedger::source_location(lua_inner);
            let resource_id = ledger_after.record(
                PluginResourceKind::Timer,
                format!("timer.after:{ms}ms"),
                source.clone(),
            );
            ledger_after.record(
                PluginResourceKind::LuaRegistryKey,
                format!("timer:{}", timer_id.get()),
                source,
            );
            let key = lua_inner.create_registry_value(f)?;
            callbacks_after.borrow_mut().insert(timer_id, key);
            registry_after.register(
                plugin_id_after.clone(),
                generation_id,
                timer_id,
                resource_id,
                TimerKind::After,
                ms,
            );
            Ok(LuaTimerHandle {
                timer_id,
                registry: registry_after.clone(),
                callbacks: Rc::clone(&callbacks_after),
            })
        })?,
    )?;

    let plugin_id_every = plugin_id;
    let registry_every = registry.clone();
    let ledger_every = ledger;
    let callbacks_every = Rc::clone(&callbacks);
    let guard_every = guard;
    timer_tbl.set(
        "every",
        lua.create_function(move |lua_inner, (ms, f): (u64, Function)| {
            guard_every
                .check_named("timer:create")
                .map_err(mlua::Error::external)?;
            if ms == 0 {
                return Err(mlua::Error::external(
                    "timer.every: interval must be > 0".to_string(),
                ));
            }
            let timer_id = registry_every.allocate();
            let source = ResourceLedger::source_location(lua_inner);
            let resource_id = ledger_every.record(
                PluginResourceKind::Timer,
                format!("timer.every:{ms}ms"),
                source.clone(),
            );
            ledger_every.record(
                PluginResourceKind::LuaRegistryKey,
                format!("timer:{}", timer_id.get()),
                source,
            );
            let key = lua_inner.create_registry_value(f)?;
            callbacks_every.borrow_mut().insert(timer_id, key);
            registry_every.register(
                plugin_id_every.clone(),
                generation_id,
                timer_id,
                resource_id,
                TimerKind::Every,
                ms,
            );
            Ok(LuaTimerHandle {
                timer_id,
                registry: registry_every.clone(),
                callbacks: Rc::clone(&callbacks_every),
            })
        })?,
    )?;

    leviathan.set("timer", timer_tbl)?;
    Ok(())
}

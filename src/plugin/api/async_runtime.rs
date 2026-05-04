//! Async deferred-call queues. `leviathan.api.schedule(fn)` and
//! `defer_fn(ms, fn)` enqueue callbacks the host drains each tick.
//!
//! Plus a coroutine bucket: user commands wrapped in coroutines that
//! yielded mid-execution land here so subsequent ticks resume them.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{Function, Lua, RegistryKey, Table};

use crate::plugin::resources::{PluginResourceKind, ResourceId, ResourceLedger};

pub struct DeferredCallback {
    pub key: RegistryKey,
    pub resource_id: ResourceId,
}

#[derive(Default)]
pub struct DeferredQueue {
    pub immediate: Vec<DeferredCallback>,
    pub delayed: Vec<(Instant, DeferredCallback)>,
    pub coroutines: Vec<DeferredCallback>,
}

impl DeferredQueue {
    pub fn has_pending(&self) -> bool {
        !self.immediate.is_empty() || !self.delayed.is_empty() || !self.coroutines.is_empty()
    }

    pub fn drain_immediate(&mut self) -> Vec<DeferredCallback> {
        std::mem::take(&mut self.immediate)
    }

    pub fn drain_due(&mut self, now: Instant) -> Vec<DeferredCallback> {
        let mut due = Vec::new();
        let mut rest = Vec::new();
        for (t, k) in std::mem::take(&mut self.delayed) {
            if t <= now {
                due.push(k);
            } else {
                rest.push((t, k));
            }
        }
        self.delayed = rest;
        due
    }
}

pub fn install(
    lua: &Lua,
    queue: Rc<RefCell<DeferredQueue>>,
    ledger: ResourceLedger,
    api: &Table,
) -> mlua::Result<()> {
    let q = Rc::clone(&queue);
    let ledger_for_schedule = ledger.clone();
    api.set(
        "schedule",
        lua.create_function(move |lua_inner, f: Function| {
            let key = lua_inner.create_registry_value(f)?;
            let source = ResourceLedger::source_location(lua_inner);
            let resource_id = ledger_for_schedule.record(
                PluginResourceKind::AsyncJob,
                "api.schedule",
                source.clone(),
            );
            ledger_for_schedule.record(
                PluginResourceKind::LuaRegistryKey,
                format!("async_job:{resource_id}"),
                source,
            );
            q.borrow_mut()
                .immediate
                .push(DeferredCallback { key, resource_id });
            Ok(())
        })?,
    )?;

    let q = Rc::clone(&queue);
    let ledger_for_defer = ledger;
    api.set(
        "defer_fn",
        lua.create_function(move |lua_inner, (ms, f): (u64, Function)| {
            let key = lua_inner.create_registry_value(f)?;
            let when = Instant::now() + Duration::from_millis(ms);
            let source = ResourceLedger::source_location(lua_inner);
            let resource_id = ledger_for_defer.record(
                PluginResourceKind::Timer,
                format!("api.defer_fn:{ms}ms"),
                source.clone(),
            );
            ledger_for_defer.record(
                PluginResourceKind::LuaRegistryKey,
                format!("timer:{resource_id}"),
                source,
            );
            q.borrow_mut()
                .delayed
                .push((when, DeferredCallback { key, resource_id }));
            Ok(())
        })?,
    )?;
    Ok(())
}

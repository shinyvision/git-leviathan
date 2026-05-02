//! Async deferred-call queues. `leviathan.api.schedule(fn)` and
//! `defer_fn(ms, fn)` enqueue callbacks the host drains each tick.
//!
//! Plus a coroutine bucket: user commands wrapped in coroutines that
//! yielded mid-execution land here so subsequent ticks resume them.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{Function, Lua, RegistryKey, Table};

#[derive(Default)]
pub struct DeferredQueue {
    pub immediate: Vec<RegistryKey>,
    pub delayed: Vec<(Instant, RegistryKey)>,
    pub coroutines: Vec<RegistryKey>,
}

impl DeferredQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn drain_immediate(&mut self) -> Vec<RegistryKey> {
        std::mem::take(&mut self.immediate)
    }

    pub fn drain_due(&mut self, now: Instant) -> Vec<RegistryKey> {
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
    api: &Table,
) -> mlua::Result<()> {
    let q = Rc::clone(&queue);
    api.set(
        "schedule",
        lua.create_function(move |lua_inner, f: Function| {
            let key = lua_inner.create_registry_value(f)?;
            q.borrow_mut().immediate.push(key);
            Ok(())
        })?,
    )?;

    let q = Rc::clone(&queue);
    api.set(
        "defer_fn",
        lua.create_function(move |lua_inner, (ms, f): (u64, Function)| {
            let key = lua_inner.create_registry_value(f)?;
            let when = Instant::now() + Duration::from_millis(ms);
            q.borrow_mut().delayed.push((when, key));
            Ok(())
        })?,
    )?;
    Ok(())
}

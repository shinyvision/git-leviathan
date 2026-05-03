//! `leviathan.async.spawn(body[, on_complete])` — spawn a host-managed
//! worker thread.
//!
//! The Lua body runs on a fresh `mlua::Lua` state on the worker thread
//! (Lua states are not Send, so the plugin's main state never crosses
//! threads). The body's return value, which must be JSON-serialisable,
//! travels back through a typed channel; the host's `tick()` drains it
//! and (re-hydrating into the plugin's main Lua state) invokes the
//! `on_complete` callback.
//!
//! Cancellation is cooperative: `JobHandle:cancel()` flips an
//! `AtomicBool`; the body checks it via `ctx:cancelled()`. The host
//! also flips the flag during plugin unload / reload so old-generation
//! jobs exit promptly.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::channel;

use mlua::{Function, Lua, RegistryKey, Table, UserData, UserDataMethods};

use crate::plugin::api::async_runtime::{DeferredCallback, DeferredQueue};
use crate::plugin::async_jobs::{
    AsyncJobRegistration, AsyncJobRegistry, CancellationToken, JobId, JobOutcome,
};
use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::resources::{GenerationId, PluginId, PluginResourceKind, ResourceLedger};

/// Per-plugin map of `JobId -> on-complete RegistryKey`. The host
/// looks up the key when it drains a finished job during `tick()`.
#[derive(Default)]
pub struct JobCallbacks {
    inner: HashMap<JobId, RegistryKey>,
}

impl JobCallbacks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, job_id: JobId, key: RegistryKey) {
        self.inner.insert(job_id, key);
    }

    pub fn remove(&mut self, job_id: JobId) -> Option<RegistryKey> {
        self.inner.remove(&job_id)
    }
}

/// Lua userdata exposed to plugin code as the spawn handle.
pub struct LuaJobHandle {
    pub job_id: JobId,
    pub registry: AsyncJobRegistry,
}

impl UserData for LuaJobHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("cancel", |_, this, ()| {
            this.registry.cancel(this.job_id);
            Ok(())
        });
        methods.add_method("id", |_, this, ()| Ok(this.job_id.get()));
    }
}

/// Lua userdata handed to the worker body as its single argument.
pub struct JobContext {
    pub cancel: CancellationToken,
}

impl UserData for JobContext {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("cancelled", |_, this, ()| Ok(this.cancel.is_cancelled()));
    }
}

pub struct AsyncInstallContext {
    pub guard: Rc<CapabilityGuard>,
    pub ledger: ResourceLedger,
    pub registry: AsyncJobRegistry,
    pub job_callbacks: Rc<RefCell<JobCallbacks>>,
    pub deferred: Rc<RefCell<DeferredQueue>>,
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
}

pub fn install(lua: &Lua, leviathan: &Table, ctx: AsyncInstallContext) -> mlua::Result<()> {
    let async_tbl = lua.create_table()?;
    let AsyncInstallContext {
        guard,
        ledger,
        registry,
        job_callbacks,
        deferred,
        plugin_id,
        generation_id,
    } = ctx;

    let registry_for_spawn = registry.clone();
    let ledger_for_spawn = ledger.clone();
    let plugin_id_for_spawn = plugin_id.clone();
    let job_callbacks_for_spawn = Rc::clone(&job_callbacks);
    async_tbl.set(
        "spawn",
        lua.create_function(
            move |lua_inner, (body, on_complete): (Function, Option<Function>)| {
                guard
                    .check_named("async:spawn")
                    .map_err(mlua::Error::external)?;

                let body_src = body.dump(false);
                if body_src.is_empty() {
                    return Err(mlua::Error::external(
                        "async.spawn: body must be a real Lua function".to_string(),
                    ));
                }

                let cancel = CancellationToken::new();
                let job_id = registry_for_spawn.allocate_job_id();
                let source = ResourceLedger::source_location(lua_inner);
                let resource_id = ledger_for_spawn.record(
                    PluginResourceKind::AsyncJob,
                    format!("async.spawn:{}", job_id.get()),
                    source,
                );

                if let Some(callback) = on_complete {
                    let key = lua_inner.create_registry_value(callback)?;
                    job_callbacks_for_spawn.borrow_mut().insert(job_id, key);
                }

                let (tx, rx) = channel::<JobOutcome>();
                let cancel_for_worker = cancel.clone();
                let plugin_id_str = plugin_id_for_spawn.as_str().to_string();
                let join = std::thread::spawn(move || {
                    let outcome = run_body(&body_src, &cancel_for_worker, &plugin_id_str);
                    let _ = tx.send(outcome);
                });

                registry_for_spawn.register(AsyncJobRegistration {
                    plugin_id: plugin_id_for_spawn.clone(),
                    generation_id,
                    job_id,
                    resource_id,
                    cancel,
                    join,
                    rx,
                });

                Ok(LuaJobHandle {
                    job_id,
                    registry: registry_for_spawn.clone(),
                })
            },
        )?,
    )?;

    leviathan.set("async", async_tbl)?;

    // Top-level alias for the existing api.schedule. Mounted here so
    // the descriptor coverage test sees the new function.
    let queue_for_alias = Rc::clone(&deferred);
    let ledger_for_alias = ledger;
    leviathan.set(
        "schedule",
        lua.create_function(move |lua_inner, f: Function| {
            let key = lua_inner.create_registry_value(f)?;
            let source = ResourceLedger::source_location(lua_inner);
            let resource_id = ledger_for_alias.record(
                PluginResourceKind::AsyncJob,
                "leviathan.schedule",
                source.clone(),
            );
            ledger_for_alias.record(
                PluginResourceKind::LuaRegistryKey,
                format!("schedule:{resource_id}"),
                source,
            );
            queue_for_alias
                .borrow_mut()
                .immediate
                .push(DeferredCallback { key, resource_id });
            Ok(())
        })?,
    )?;

    Ok(())
}

/// Worker-side execution. Lua states are not `Send`, so a fresh state
/// runs the dumped bytecode on the worker thread.
fn run_body(body_src: &[u8], cancel: &CancellationToken, plugin_id: &str) -> JobOutcome {
    if cancel.is_cancelled() {
        return JobOutcome::Cancelled;
    }
    let lua = Lua::new();
    let chunk_name = format!("plugins/{plugin_id}/async_spawn");
    let body: Function = match lua.load(body_src).set_name(chunk_name).into_function() {
        Ok(f) => f,
        Err(e) => return JobOutcome::Failed(format!("async body load failed: {e}")),
    };
    let ctx = JobContext {
        cancel: cancel.clone(),
    };
    match body.call::<mlua::Value>(ctx) {
        Ok(value) => {
            if cancel.is_cancelled() {
                return JobOutcome::Cancelled;
            }
            match lua_value_to_json(value) {
                Ok(v) => JobOutcome::Ok(v),
                Err(e) => JobOutcome::Failed(format!("async result not JSON-able: {e}")),
            }
        }
        Err(e) => {
            if cancel.is_cancelled() {
                JobOutcome::Cancelled
            } else {
                JobOutcome::Failed(format!("async body error: {e}"))
            }
        }
    }
}

/// Cross-thread `mlua::Value -> serde_json::Value` projector. Only
/// handles JSON-equivalent shapes (nil/bool/int/float/string/table).
/// Functions, threads, userdata, and other opaque types collapse to
/// `null`.
fn lua_value_to_json(value: mlua::Value) -> Result<serde_json::Value, String> {
    match value {
        mlua::Value::Nil => Ok(serde_json::Value::Null),
        mlua::Value::Boolean(b) => Ok(serde_json::Value::Bool(b)),
        mlua::Value::Integer(i) => Ok(serde_json::Value::Number(i.into())),
        mlua::Value::Number(n) => Ok(serde_json::Number::from_f64(n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null)),
        mlua::Value::String(text) => Ok(serde_json::Value::String(
            text.to_str().map(|cow| cow.to_string()).unwrap_or_default(),
        )),
        mlua::Value::Table(t) => {
            let len = t.raw_len();
            let mut is_array = len > 0;
            if is_array {
                for i in 1..=len {
                    match t.raw_get::<mlua::Value>(i) {
                        Ok(mlua::Value::Nil) => {
                            is_array = false;
                            break;
                        }
                        Err(_) => {
                            is_array = false;
                            break;
                        }
                        _ => {}
                    }
                }
            }
            if is_array {
                let mut arr = Vec::with_capacity(len);
                for i in 1..=len {
                    let v: mlua::Value = t.raw_get(i).map_err(|e| format!("table get {i}: {e}"))?;
                    arr.push(lua_value_to_json(v)?);
                }
                Ok(serde_json::Value::Array(arr))
            } else {
                let mut map = serde_json::Map::new();
                for pair in t.pairs::<mlua::Value, mlua::Value>() {
                    let (k, v) = pair.map_err(|e| format!("pairs: {e}"))?;
                    let key = match k {
                        mlua::Value::String(text) => {
                            text.to_str().map(|cow| cow.to_string()).unwrap_or_default()
                        }
                        mlua::Value::Integer(i) => i.to_string(),
                        mlua::Value::Number(n) => n.to_string(),
                        mlua::Value::Boolean(b) => b.to_string(),
                        _ => continue,
                    };
                    map.insert(key, lua_value_to_json(v)?);
                }
                Ok(serde_json::Value::Object(map))
            }
        }
        _ => Ok(serde_json::Value::Null),
    }
}

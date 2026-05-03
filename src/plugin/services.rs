//! Inter-plugin service registry with cross-VM JSON marshaling.
//!
//! Publishers register a table of Lua functions under "name@version".
//! Consumers `get` a proxy table whose methods bounce through this
//! registry: args serialize to JSON, the publisher's Lua VM is looked
//! up by name, the publisher's function runs, the return value
//! serializes back to JSON for the consumer's VM to deserialize.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, Value as LuaValue};

use git_leviathan_plugin_api::manifest::ServiceDecl;

use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::performance::{BudgetTracker, CallbackKind, Outcome as PerfOutcome};
use crate::plugin::resources::{GenerationId, PluginId};

const SERVICE_TRACE_LIMIT: usize = 128;

pub struct ServiceHandle {
    pub plugin_id: String,
    pub decl: ServiceDecl,
    pub methods: HashMap<String, RegistryKey>,
    pub lua: Rc<Lua>,
}

#[derive(Debug, Clone)]
pub struct ServiceCallTrace {
    pub caller_plugin_id: String,
    pub provider_plugin_id: String,
    pub service_key: String,
    pub method: String,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u128,
    pub timestamp_unix_ms: u128,
}

#[derive(Debug, Clone)]
pub struct ServiceDependencyStatus {
    pub consumer_plugin_id: String,
    pub service_key: String,
    pub required: bool,
    pub provider_plugin_id: Option<String>,
    pub satisfied: bool,
}

#[derive(Default)]
pub struct ServiceRegistry {
    handles: HashMap<String, ServiceHandle>,
    traces: RefCell<VecDeque<ServiceCallTrace>>,
    /// Phase 18 budget tracker (optional). When set, every service
    /// call is timed against the `ServiceCall` budget and routes
    /// through the per-callback circuit breaker. Constructed empty;
    /// the host installs the live tracker after `PluginHost::new`.
    budget_tracker: RefCell<Option<BudgetTracker>>,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the host's Phase 18 budget tracker. Calls before this
    /// runs simply skip the budget pass (used to keep a few existing
    /// unit tests that build a bare `ServiceRegistry` without a host
    /// from needing to reconstruct one).
    pub fn set_budget_tracker(&self, tracker: BudgetTracker) {
        *self.budget_tracker.borrow_mut() = Some(tracker);
    }

    pub fn key(decl: &ServiceDecl) -> String {
        decl.key()
    }

    pub fn register(
        &mut self,
        plugin_id: &str,
        decl: ServiceDecl,
        methods: HashMap<String, RegistryKey>,
        lua: Rc<Lua>,
    ) -> Result<(), String> {
        let k = Self::key(&decl);
        if self.handles.contains_key(&k) {
            return Err(format!("service '{k}' already registered"));
        }
        self.handles.insert(
            k,
            ServiceHandle {
                plugin_id: plugin_id.to_string(),
                decl,
                methods,
                lua,
            },
        );
        Ok(())
    }

    pub fn drain_for_plugin(&mut self, plugin_id: &str) -> Vec<(String, ServiceHandle)> {
        let mut drained = Vec::new();
        let keys: Vec<String> = self
            .handles
            .iter()
            .filter_map(|(key, handle)| (handle.plugin_id == plugin_id).then(|| key.clone()))
            .collect();
        for key in keys {
            if let Some(handle) = self.handles.remove(&key) {
                drained.push((key, handle));
            }
        }
        drained
    }

    pub fn restore_handles(&mut self, handles: Vec<(String, ServiceHandle)>) {
        for (key, handle) in handles {
            self.handles.insert(key, handle);
        }
    }

    pub fn unregister(&mut self, service_key: &str, plugin_id: &str) {
        if self
            .handles
            .get(service_key)
            .map(|handle| handle.plugin_id.as_str() == plugin_id)
            .unwrap_or(false)
        {
            self.handles.remove(service_key);
        }
    }

    pub fn handles_iter(&self) -> impl Iterator<Item = &ServiceHandle> {
        self.handles.values()
    }

    pub fn provider_plugin_id(&self, service_key: &str) -> Option<&str> {
        self.handles
            .get(service_key)
            .map(|handle| handle.plugin_id.as_str())
    }

    pub fn method_names(&self, service_key: &str) -> Option<Vec<String>> {
        self.handles.get(service_key).map(|handle| {
            let mut names: Vec<String> = handle.methods.keys().cloned().collect();
            names.sort();
            names
        })
    }

    pub fn traces(&self) -> Vec<ServiceCallTrace> {
        self.traces.borrow().iter().cloned().collect()
    }

    pub fn invoke(
        &self,
        caller_plugin_id: &str,
        caller_generation_id: GenerationId,
        caller_guard: Rc<CapabilityGuard>,
        service_key: &str,
        method: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let started = Instant::now();
        let timestamp_unix_ms = now_unix_ms();
        let provider_plugin_id = self
            .provider_plugin_id(service_key)
            .unwrap_or("<missing>")
            .to_string();

        // Phase 18: route through the budget tracker keyed against
        // the caller (callers are how plugins reach into a provider;
        // tripping per caller-method keeps a single misbehaving
        // consumer from disabling a healthy provider). The tracker
        // skip path returns a synthetic error string that surfaces in
        // the trace — same shape as a real failure so devtools rows
        // stay homogeneous.
        let tracker_opt = self.budget_tracker.borrow().clone();
        let callback_id = format!("service:{service_key}:{method}");
        let result = if let Some(tracker) = tracker_opt {
            let pid = PluginId::from(caller_plugin_id);
            match tracker.track_call::<serde_json::Value, String>(
                CallbackKind::ServiceCall,
                &pid,
                caller_generation_id,
                &callback_id,
                || self.invoke_inner(caller_guard, service_key, method, args),
            ) {
                PerfOutcome::Ok(v) => Ok(v),
                PerfOutcome::Err(e) => Err(e),
                PerfOutcome::Skipped => Err("service call skipped: circuit breaker tripped".into()),
            }
        } else {
            self.invoke_inner(caller_guard, service_key, method, args)
        };

        let error = result.as_ref().err().cloned();
        self.record_trace(ServiceCallTrace {
            caller_plugin_id: caller_plugin_id.to_string(),
            provider_plugin_id,
            service_key: service_key.to_string(),
            method: method.to_string(),
            success: result.is_ok(),
            error,
            duration_ms: started.elapsed().as_millis(),
            timestamp_unix_ms,
        });
        result
    }

    fn invoke_inner(
        &self,
        caller_guard: Rc<CapabilityGuard>,
        service_key: &str,
        method: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let h = self
            .handles
            .get(service_key)
            .ok_or_else(|| format!("service '{service_key}' not registered"))?;
        let key = h
            .methods
            .get(method)
            .ok_or_else(|| format!("service '{service_key}' has no method '{method}'"))?;
        let lua: &Lua = &h.lua;
        let f: Function = lua
            .registry_value(key)
            .map_err(|e| format!("registry: {e}"))?;
        let _caller_scope = CapabilityGuard::push_service_caller(caller_guard);
        let result_lua: LuaValue = match &args {
            serde_json::Value::Array(arr) => {
                let mut vargs = mlua::MultiValue::new();
                for v in arr {
                    let lv: LuaValue = lua.to_value(v).map_err(|e| format!("arg ser: {e}"))?;
                    vargs.push_back(lv);
                }
                f.call(vargs).map_err(|e| format!("call: {e}"))?
            }
            _ => {
                let lua_arg: LuaValue = lua.to_value(&args).map_err(|e| format!("arg ser: {e}"))?;
                f.call(lua_arg).map_err(|e| format!("call: {e}"))?
            }
        };
        lua.from_value(result_lua)
            .map_err(|e| format!("ret de: {e}"))
    }

    fn record_trace(&self, trace: ServiceCallTrace) {
        let mut traces = self.traces.borrow_mut();
        if traces.len() == SERVICE_TRACE_LIMIT {
            traces.pop_front();
        }
        traces.push_back(trace);
    }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

pub fn dependency_statuses(
    consumer_plugin_id: &str,
    provides: &[ServiceDecl],
    consumes: &[ServiceDecl],
    registry: &ServiceRegistry,
) -> Vec<ServiceDependencyStatus> {
    consumes
        .iter()
        .map(|decl| {
            let service_key = ServiceRegistry::key(decl);
            let provider_plugin_id = registry
                .provider_plugin_id(&service_key)
                .map(str::to_string)
                .or_else(|| {
                    provides
                        .iter()
                        .any(|provided| {
                            provided.name == decl.name && provided.version == decl.version
                        })
                        .then(|| consumer_plugin_id.to_string())
                });
            let satisfied = provider_plugin_id.is_some();
            ServiceDependencyStatus {
                consumer_plugin_id: consumer_plugin_id.to_string(),
                service_key,
                required: decl.required,
                satisfied,
                provider_plugin_id,
            }
        })
        .collect()
}

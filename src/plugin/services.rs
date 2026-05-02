//! Inter-plugin service registry with cross-VM JSON marshaling.
//!
//! Publishers register a table of Lua functions under "name@version".
//! Consumers `get` a proxy table whose methods bounce through this
//! registry: args serialize to JSON, the publisher's Lua VM is looked
//! up by name, the publisher's function runs, the return value
//! serializes back to JSON for the consumer's VM to deserialize.

use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, Value as LuaValue};

use git_leviathan_plugin_api::manifest::ServiceDecl;

pub struct ServiceHandle {
    pub plugin_id: String,
    pub decl: ServiceDecl,
    pub methods: HashMap<String, RegistryKey>,
    pub lua: Rc<Lua>,
}

#[derive(Default)]
pub struct ServiceRegistry {
    handles: HashMap<String, ServiceHandle>,
}

impl ServiceRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn key(decl: &ServiceDecl) -> String {
        format!("{}@{}", decl.name, decl.version)
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
        self.handles.insert(k, ServiceHandle { plugin_id: plugin_id.to_string(), decl, methods, lua });
        Ok(())
    }

    pub fn unregister_for_plugin(&mut self, plugin_id: &str) {
        self.handles.retain(|_, h| h.plugin_id != plugin_id);
    }

    pub fn handles_iter(&self) -> impl Iterator<Item = &ServiceHandle> {
        self.handles.values()
    }

    pub fn invoke(&self, service_key: &str, method: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
        let h = self.handles.get(service_key)
            .ok_or_else(|| format!("service '{service_key}' not registered"))?;
        let key = h.methods.get(method)
            .ok_or_else(|| format!("service '{service_key}' has no method '{method}'"))?;
        let lua: &Lua = &h.lua;
        let f: Function = lua.registry_value(key).map_err(|e| format!("registry: {e}"))?;
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
        lua.from_value(result_lua).map_err(|e| format!("ret de: {e}"))
    }
}

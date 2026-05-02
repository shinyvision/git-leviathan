//! leviathan.services.{register, get}.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Lua, LuaSerdeExt, MultiValue, Table, Value as LuaValue};

use crate::plugin::services::ServiceRegistry;
use git_leviathan_plugin_api::manifest::ServiceDecl;

pub struct ServicesContext {
    pub registry: Rc<RefCell<ServiceRegistry>>,
    pub plugin_id: String,
    pub provides: Vec<ServiceDecl>,
    pub consumes: Vec<ServiceDecl>,
    pub plugin_lua: Rc<Lua>,
}

pub fn install(lua: &Lua, ctx: ServicesContext, leviathan: &Table) -> mlua::Result<()> {
    let services = lua.create_table()?;

    let provides = ctx.provides.clone();
    let registry_for_register = Rc::clone(&ctx.registry);
    let plugin_id_for_register = ctx.plugin_id.clone();
    let lua_for_register = Rc::clone(&ctx.plugin_lua);

    services.set("register", lua.create_function(move |lua_inner, (name_at_ver, methods): (String, Table)| {
        let decl = parse_decl(&name_at_ver).map_err(mlua::Error::external)?;
        if !provides.iter().any(|d| d.name == decl.name && d.version == decl.version) {
            return Err(mlua::Error::external(format!(
                "service '{name_at_ver}' not declared in provides_services")));
        }
        let mut method_keys: HashMap<String, mlua::RegistryKey> = HashMap::new();
        for pair in methods.pairs::<String, mlua::Function>() {
            let (k, f) = pair?;
            method_keys.insert(k, lua_inner.create_registry_value(f)?);
        }
        registry_for_register.borrow_mut()
            .register(&plugin_id_for_register, decl, method_keys, Rc::clone(&lua_for_register))
            .map_err(mlua::Error::external)
    })?)?;

    let consumes = ctx.consumes.clone();
    let registry_for_get = Rc::clone(&ctx.registry);

    services.set("get", lua.create_function(move |lua_inner, name_at_ver: String| -> mlua::Result<Table> {
        let decl = parse_decl(&name_at_ver).map_err(mlua::Error::external)?;
        if !consumes.iter().any(|d| d.name == decl.name && d.version == decl.version) {
            return Err(mlua::Error::external(format!(
                "service '{name_at_ver}' not declared in consumes_services")));
        }
        let key = ServiceRegistry::key(&decl);
        let proxy = lua_inner.create_table()?;

        let method_names: Vec<String> = {
            let reg = registry_for_get.borrow();
            let h = reg.handles_iter()
                .find(|h| ServiceRegistry::key(&h.decl) == key)
                .ok_or_else(|| mlua::Error::external(format!("service '{name_at_ver}' not registered")))?;
            h.methods.keys().cloned().collect()
        };

        for method_name in method_names {
            let registry_for_method = Rc::clone(&registry_for_get);
            let key_for_method = key.clone();
            let mname = method_name.clone();
            proxy.set(method_name, lua_inner.create_function(move |consumer_lua, args: MultiValue| {
                let mut arr: Vec<serde_json::Value> = Vec::new();
                for v in args.into_iter() {
                    let j: serde_json::Value = consumer_lua.from_value(v)
                        .map_err(|e| mlua::Error::external(format!("arg ser: {e}")))?;
                    arr.push(j);
                }
                let result = registry_for_method.borrow()
                    .invoke(&key_for_method, &mname, serde_json::Value::Array(arr))
                    .map_err(mlua::Error::external)?;
                let v: LuaValue = consumer_lua.to_value(&result)?;
                Ok(v)
            })?)?;
        }
        Ok(proxy)
    })?)?;

    leviathan.set("services", services)?;
    Ok(())
}

fn parse_decl(s: &str) -> Result<ServiceDecl, String> {
    let (n, v) = s.split_once('@').ok_or_else(|| "service must be 'name@version'".to_string())?;
    Ok(ServiceDecl { name: n.to_string(), version: v.parse::<u32>().map_err(|e| format!("{e}"))? })
}

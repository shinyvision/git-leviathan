//! leviathan.services.{register, get}.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Lua, LuaSerdeExt, MultiValue, Table, Value as LuaValue};

use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::resources::{GenerationId, PluginResourceKind, ResourceLedger};
use crate::plugin::services::ServiceRegistry;
use git_leviathan_plugin_api::manifest::ServiceDecl;

pub struct ServicesContext {
    pub registry: Rc<RefCell<ServiceRegistry>>,
    pub lookup_registry: Option<Rc<RefCell<ServiceRegistry>>>,
    pub plugin_id: String,
    pub generation_id: GenerationId,
    pub provides: Vec<ServiceDecl>,
    pub consumes: Vec<ServiceDecl>,
    pub plugin_lua: Rc<Lua>,
    pub capability_guard: Rc<CapabilityGuard>,
}

pub fn install(
    lua: &Lua,
    ctx: ServicesContext,
    ledger: ResourceLedger,
    leviathan: &Table,
) -> mlua::Result<()> {
    let services = lua.create_table()?;

    let provides = ctx.provides.clone();
    let registry_for_register = Rc::clone(&ctx.registry);
    let plugin_id_for_register = ctx.plugin_id.clone();
    let lua_for_register = Rc::clone(&ctx.plugin_lua);

    services.set(
        "register",
        lua.create_function(move |lua_inner, args: MultiValue| {
            let (decl, methods) = parse_register_args(args)?;
            let service_key = ServiceRegistry::key(&decl);
            if !provides
                .iter()
                .any(|d| d.name == decl.name && d.version == decl.version)
            {
                return Err(mlua::Error::external(format!(
                    "service '{service_key}' not declared in provides_services"
                )));
            }
            let mut method_keys: HashMap<String, mlua::RegistryKey> = HashMap::new();
            let mut method_names: Vec<String> = Vec::new();
            for pair in methods.pairs::<String, mlua::Function>() {
                let (k, f) = pair?;
                method_names.push(k.clone());
                method_keys.insert(k, lua_inner.create_registry_value(f)?);
            }
            let source = ResourceLedger::source_location(lua_inner);
            registry_for_register
                .borrow_mut()
                .register(
                    &plugin_id_for_register,
                    decl.clone(),
                    method_keys,
                    Rc::clone(&lua_for_register),
                )
                .map_err(mlua::Error::external)?;
            ledger.record(
                PluginResourceKind::ServiceRegistration,
                service_key.clone(),
                source.clone(),
            );
            for method_name in method_names {
                ledger.record(
                    PluginResourceKind::LuaRegistryKey,
                    format!("service:{service_key}:{method_name}"),
                    source.clone(),
                );
            }
            Ok(())
        })?,
    )?;

    let consumes = ctx.consumes.clone();
    let registry_for_get = Rc::clone(&ctx.registry);
    let lookup_registry_for_get = ctx.lookup_registry.clone();
    let caller_plugin_id = ctx.plugin_id.clone();
    let caller_generation_id = ctx.generation_id;
    let caller_guard = Rc::clone(&ctx.capability_guard);

    services.set(
        "get",
        lua.create_function(
            move |lua_inner, args: MultiValue| -> mlua::Result<LuaValue> {
                let decl = parse_get_args(args)?;
                let service_key = ServiceRegistry::key(&decl);
                let Some(consume_decl) = consumes
                    .iter()
                    .find(|d| d.name == decl.name && d.version == decl.version)
                else {
                    return Err(mlua::Error::external(format!(
                        "service '{service_key}' not declared in consumes_services"
                    )));
                };
                let proxy = lua_inner.create_table()?;

                let Some((invoke_registry, method_names)) = find_service_methods(
                    &registry_for_get,
                    lookup_registry_for_get.as_ref(),
                    &service_key,
                ) else {
                    if consume_decl.required {
                        return Err(mlua::Error::external(format!(
                            "service '{service_key}' not registered"
                        )));
                    }
                    return Ok(LuaValue::Nil);
                };

                for method_name in method_names {
                    let registry_for_method = Rc::clone(&invoke_registry);
                    let key_for_method = service_key.clone();
                    let mname = method_name.clone();
                    let caller_plugin_id = caller_plugin_id.clone();
                    let caller_guard = Rc::clone(&caller_guard);
                    proxy.set(
                        method_name,
                        lua_inner.create_function(move |consumer_lua, args: MultiValue| {
                            let mut arr: Vec<serde_json::Value> = Vec::new();
                            for v in args.into_iter() {
                                let j: serde_json::Value = consumer_lua
                                    .from_value(v)
                                    .map_err(|e| mlua::Error::external(format!("arg ser: {e}")))?;
                                arr.push(j);
                            }
                            let result = registry_for_method
                                .borrow()
                                .invoke(
                                    &caller_plugin_id,
                                    caller_generation_id,
                                    Rc::clone(&caller_guard),
                                    &key_for_method,
                                    &mname,
                                    serde_json::Value::Array(arr),
                                )
                                .map_err(mlua::Error::external)?;
                            let v: LuaValue = consumer_lua.to_value(&result)?;
                            Ok(v)
                        })?,
                    )?;
                }
                Ok(LuaValue::Table(proxy))
            },
        )?,
    )?;

    leviathan.set("services", services)?;
    Ok(())
}

fn parse_register_args(args: MultiValue) -> mlua::Result<(ServiceDecl, Table)> {
    let values: Vec<LuaValue> = args.into_iter().collect();
    match values.as_slice() {
        [LuaValue::String(name_at_ver), LuaValue::Table(methods)] => {
            let decl = parse_decl(&name_at_ver.to_str()?).map_err(mlua::Error::external)?;
            Ok((decl, methods.clone()))
        }
        [LuaValue::String(name), LuaValue::Integer(version), LuaValue::Table(methods)] => {
            let decl =
                decl_from_name_version(&name.to_str()?, *version).map_err(mlua::Error::external)?;
            Ok((decl, methods.clone()))
        }
        _ => Err(mlua::Error::external(
            "services.register expects (name@version, methods) or (name, version, methods)",
        )),
    }
}

fn parse_get_args(args: MultiValue) -> mlua::Result<ServiceDecl> {
    let values: Vec<LuaValue> = args.into_iter().collect();
    match values.as_slice() {
        [LuaValue::String(name_at_ver)] => {
            parse_decl(&name_at_ver.to_str()?).map_err(mlua::Error::external)
        }
        [LuaValue::String(name), LuaValue::Integer(version)] => {
            decl_from_name_version(&name.to_str()?, *version).map_err(mlua::Error::external)
        }
        _ => Err(mlua::Error::external(
            "services.get expects (name@version) or (name, version)",
        )),
    }
}

fn parse_decl(s: &str) -> Result<ServiceDecl, String> {
    let (n, v) = s
        .split_once('@')
        .ok_or_else(|| "service must be 'name@version'".to_string())?;
    Ok(ServiceDecl {
        name: n.to_string(),
        version: v.parse::<u32>().map_err(|e| format!("{e}"))?,
        required: true,
    })
}

fn decl_from_name_version(name: &str, version: i64) -> Result<ServiceDecl, String> {
    let version = u32::try_from(version)
        .map_err(|_| format!("service version must be a non-negative u32: {version}"))?;
    Ok(ServiceDecl {
        name: name.to_string(),
        version,
        required: true,
    })
}

fn find_service_methods(
    primary: &Rc<RefCell<ServiceRegistry>>,
    fallback: Option<&Rc<RefCell<ServiceRegistry>>>,
    service_key: &str,
) -> Option<(Rc<RefCell<ServiceRegistry>>, Vec<String>)> {
    if let Some(methods) = primary.borrow().method_names(service_key) {
        return Some((Rc::clone(primary), methods));
    }
    let fallback = fallback?;
    if Rc::ptr_eq(primary, fallback) {
        return None;
    }
    fallback
        .borrow()
        .method_names(service_key)
        .map(|methods| (Rc::clone(fallback), methods))
}

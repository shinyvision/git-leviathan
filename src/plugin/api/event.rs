//! `leviathan.api.create_autocmd` (legacy) and `leviathan.autocmd.*`
//! (Phase 7) — plugin-side typed-event subscription.
//!
//! Phase 7 introduced typed event descriptors (see
//! [`git_leviathan_plugin_api::descriptor::api::API_EVENTS`]) and the
//! `leviathan.autocmd` namespace which mirrors Neovim's autocmd shape:
//!
//! ```lua
//! local group = leviathan.autocmd.group("my_plugin", { clear = true })
//! leviathan.autocmd.create("CommitSelected", {
//!     group = group,
//!     once = false,
//!     pattern = "feature/*",
//!     debounce_ms = 50,
//!     priority = 10,
//!     callback = function(ev)
//!         -- ev.event, ev.alias, ev.payload
//!     end,
//! })
//! ```
//!
//! The legacy shim `leviathan.api.create_autocmd({ "FetchStart" }, …)`
//! still works and routes through the same dispatch funnel — Phase 7
//! is purely additive on the v1 surface.
//!
//! ## ELM discipline
//!
//! Callbacks mutate Lua-side plugin state. After every fire the host
//! re-invokes each affected plugin's dynamic widget functions so the
//! main bar reflects the new state. The view stays a pure function of
//! (Rust state + plugin Lua state).

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Table, Value as LuaValue};

use crate::plugin::events::AutocmdOptions;
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};

use super::{BuildState, RawAutocmd, RawAutocmdClear, RawAutocmdGroup};

pub fn install(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    api: &Table,
    leviathan: &Table,
) -> mlua::Result<()> {
    install_legacy_create_autocmd(lua, Rc::clone(&build), ledger.clone(), api)?;

    let autocmd_tbl = lua.create_table()?;
    install_autocmd_group(lua, Rc::clone(&build), &autocmd_tbl)?;
    install_autocmd_create(lua, Rc::clone(&build), ledger.clone(), &autocmd_tbl)?;
    install_autocmd_clear(lua, Rc::clone(&build), &autocmd_tbl)?;

    leviathan.set("autocmd", autocmd_tbl)?;
    Ok(())
}

fn install_legacy_create_autocmd(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    api: &Table,
) -> mlua::Result<()> {
    api.set(
        "create_autocmd",
        lua.create_function(move |lua_inner, (events, opts): (Table, Table)| {
            let callback: Function = opts.get("callback")?;
            let source = ResourceLedger::source_location(lua_inner);
            // Decode options once; every event in the events array
            // shares the same options struct (cloning the few small
            // fields is cheap; the registry key is per-event).
            let opts_snapshot = decode_options(lua_inner, &opts, &build)?;
            for ev in events.sequence_values::<String>() {
                let ev = ev?;
                let key = lua_inner.create_registry_value(callback.clone())?;
                ledger.record(PluginResourceKind::Autocmd, ev.clone(), source.clone());
                ledger.record(
                    PluginResourceKind::LuaRegistryKey,
                    format!("autocmd:{ev}"),
                    source.clone(),
                );
                let sequence = {
                    let mut state = build.borrow_mut();
                    state.next_autocmd_sequence += 1;
                    state.next_autocmd_sequence
                };
                build.borrow_mut().autocmds.push(RawAutocmd {
                    event: ev,
                    callback: key,
                    local_group: opts_snapshot.local_group,
                    once: opts_snapshot.once,
                    pattern: opts_snapshot.pattern.clone(),
                    debounce_ms: opts_snapshot.debounce_ms,
                    priority: opts_snapshot.priority,
                    sequence,
                    source_location: source.clone(),
                });
            }
            Ok(())
        })?,
    )?;
    Ok(())
}

fn install_autocmd_group(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    autocmd: &Table,
) -> mlua::Result<()> {
    autocmd.set(
        "group",
        lua.create_function(move |lua_inner, (name, opts): (String, Option<Table>)| {
            let clear = opts
                .as_ref()
                .and_then(|t| t.get::<Option<bool>>("clear").ok().flatten())
                .unwrap_or(false);
            let source = ResourceLedger::source_location(lua_inner);
            let mut state = build.borrow_mut();
            state.next_autocmd_sequence += 1;
            let sequence = state.next_autocmd_sequence;
            // De-duplicate by name within a single init: re-calling
            // `group("x")` returns the same local id so the plugin's
            // module-level `M.group = leviathan.autocmd.group(...)`
            // pattern works without repeated allocations.
            if let Some(existing) = state
                .autocmd_groups
                .iter()
                .rev()
                .find(|g| g.name == name)
                .map(|g| g.local_id)
            {
                if clear {
                    state.autocmd_clears.push(RawAutocmdClear {
                        local_id: existing,
                        sequence,
                        source_location: source,
                    });
                }
                return Ok(existing);
            }
            state.next_local_group_id += 1;
            let local_id = state.next_local_group_id;
            state.autocmd_groups.push(RawAutocmdGroup {
                local_id,
                name,
                clear,
                sequence,
                source_location: source,
            });
            Ok(local_id)
        })?,
    )?;
    Ok(())
}

fn install_autocmd_create(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    autocmd: &Table,
) -> mlua::Result<()> {
    autocmd.set(
        "create",
        lua.create_function(move |lua_inner, (events, opts): (LuaValue, Table)| {
            let callback: Function = opts.get("callback")?;
            let source = ResourceLedger::source_location(lua_inner);
            let opts_snapshot = decode_options(lua_inner, &opts, &build)?;
            let event_names: Vec<String> = match events {
                LuaValue::String(s) => vec![s.to_str()?.to_string()],
                LuaValue::Table(t) => {
                    let mut v = Vec::new();
                    for value in t.sequence_values::<String>() {
                        v.push(value?);
                    }
                    v
                }
                other => {
                    return Err(mlua::Error::external(format!(
                        "leviathan.autocmd.create: event must be string or string[], got {}",
                        other.type_name()
                    )));
                }
            };
            for ev in event_names {
                let key = lua_inner.create_registry_value(callback.clone())?;
                ledger.record(PluginResourceKind::Autocmd, ev.clone(), source.clone());
                ledger.record(
                    PluginResourceKind::LuaRegistryKey,
                    format!("autocmd:{ev}"),
                    source.clone(),
                );
                let sequence = {
                    let mut state = build.borrow_mut();
                    state.next_autocmd_sequence += 1;
                    state.next_autocmd_sequence
                };
                build.borrow_mut().autocmds.push(RawAutocmd {
                    event: ev,
                    callback: key,
                    local_group: opts_snapshot.local_group,
                    once: opts_snapshot.once,
                    pattern: opts_snapshot.pattern.clone(),
                    debounce_ms: opts_snapshot.debounce_ms,
                    priority: opts_snapshot.priority,
                    sequence,
                    source_location: source.clone(),
                });
            }
            Ok(())
        })?,
    )?;
    Ok(())
}

fn install_autocmd_clear(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    autocmd: &Table,
) -> mlua::Result<()> {
    autocmd.set(
        "clear",
        lua.create_function(move |lua_inner, group: u64| {
            let source = ResourceLedger::source_location(lua_inner);
            let mut state = build.borrow_mut();
            state.next_autocmd_sequence += 1;
            let sequence = state.next_autocmd_sequence;
            state.autocmd_clears.push(RawAutocmdClear {
                local_id: group,
                sequence,
                source_location: source,
            });
            Ok(())
        })?,
    )?;
    Ok(())
}

/// Decoded snapshot of the option table with defaults filled in.
/// The struct is private so the autocmd descriptor stays the single
/// source of truth for option names.
struct DecodedOptions {
    local_group: Option<u64>,
    once: bool,
    pattern: Option<String>,
    debounce_ms: u64,
    priority: i32,
}

fn decode_options(
    _lua: &Lua,
    opts: &Table,
    build: &Rc<RefCell<BuildState>>,
) -> mlua::Result<DecodedOptions> {
    let group: Option<u64> = opts.get("group").ok().flatten();
    let once: bool = opts.get::<Option<bool>>("once")?.unwrap_or(false);
    let pattern: Option<String> = match opts.get::<Option<String>>("pattern") {
        Ok(v) => v.filter(|s| !s.is_empty()),
        Err(_) => None,
    };
    let debounce_ms: u64 = opts
        .get::<Option<i64>>("debounce_ms")?
        .map(|v| v.max(0) as u64)
        .unwrap_or(0);
    let priority: i32 = opts.get::<Option<i32>>("priority")?.unwrap_or(0);
    if let Some(g) = group {
        let known = build
            .borrow()
            .autocmd_groups
            .iter()
            .any(|gd| gd.local_id == g);
        if !known {
            return Err(mlua::Error::external(format!(
                "leviathan.autocmd: group handle {g} was not allocated by leviathan.autocmd.group in this plugin",
            )));
        }
    }
    Ok(DecodedOptions {
        local_group: group,
        once,
        pattern,
        debounce_ms,
        priority,
    })
}

/// Build an [`AutocmdOptions`] from a [`RawAutocmd`] given a lookup
/// from local-group id to host-wide GroupId. Used by the host /
/// staging install paths when committing autocmds into the
/// `EventBus`.
pub fn build_options(
    raw: &RawAutocmd,
    local_to_host: &std::collections::HashMap<u64, crate::plugin::events::GroupId>,
) -> AutocmdOptions {
    AutocmdOptions {
        group: raw
            .local_group
            .and_then(|local| local_to_host.get(&local).copied()),
        once: raw.once,
        pattern: raw.pattern.clone(),
        debounce_ms: raw.debounce_ms,
        priority: raw.priority,
        source_location: raw.source_location.clone(),
    }
}

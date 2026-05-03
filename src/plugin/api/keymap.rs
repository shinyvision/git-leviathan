//! keymaps `leviathan.keymap.{set,del,list}` Lua surface.
//!
//! `set` and `del` capture into the per-plugin [`BuildState`]; the
//! host drains those after init.lua and installs them into the live
//! [`crate::plugin::keymap::KeymapRegistry`] under
//! `(plugin_id, generation_id)`. `list` reads the resolved registry
//! through the cheap-cloned shared handle so plugin authors see
//! exactly what the host saw, complete with conflict winners /
//! losers.
//!
//! The `set` shim never tries to dispatch a binding — that's the
//! host's job. Plugins request, the host owns the effect.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Lua, LuaSerdeExt, Table, Value as LuaValue};

use crate::plugin::keymap::{KeymapRegistry, RawKeymap, RawKeymapDel};
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};

use super::BuildState;

/// Cheap-clone shared handle to the registry. Held by the host and
/// by every Lua `list` shim.
pub type SharedKeymapRegistry = Rc<RefCell<KeymapRegistry>>;

pub fn install(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    leviathan: &Table,
    keymaps: SharedKeymapRegistry,
) -> mlua::Result<()> {
    let keymap_tbl = lua.create_table()?;
    install_set(lua, Rc::clone(&build), ledger.clone(), &keymap_tbl)?;
    install_del(lua, Rc::clone(&build), &keymap_tbl)?;
    install_list(lua, keymaps, &keymap_tbl)?;
    leviathan.set("keymap", keymap_tbl)?;
    Ok(())
}

fn install_set(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    keymap_tbl: &Table,
) -> mlua::Result<()> {
    keymap_tbl.set(
        "set",
        lua.create_function(
            move |lua_inner,
                  (context, key, command, opts): (
                String,
                String,
                String,
                Option<Table>,
            )| {
                let (description, args) = match opts {
                    Some(t) => {
                        let description: String =
                            t.get::<Option<String>>("description")?.unwrap_or_default();
                        let args = match t.get::<Option<LuaValue>>("args")? {
                            Some(LuaValue::Nil) | None => serde_json::Value::Null,
                            Some(v) => lua_inner.from_value::<serde_json::Value>(v)?,
                        };
                        (description, args)
                    }
                    None => (String::new(), serde_json::Value::Null),
                };
                let source = ResourceLedger::source_location(lua_inner);
                let handle = format!("{context}:{key}");
                ledger.remove_by_kind_handle(PluginResourceKind::Keymap, &handle);
                ledger.record(
                    PluginResourceKind::Keymap,
                    handle,
                    source.clone(),
                );
                let mut b = build.borrow_mut();
                let sequence = b.next_keymap_sequence;
                b.next_keymap_sequence += 1;
                b.keymaps.push(RawKeymap {
                    context,
                    key,
                    command,
                    args,
                    description,
                    sequence,
                    source_location: source,
                });
                Ok(())
            },
        )?,
    )?;
    Ok(())
}

fn install_del(lua: &Lua, build: Rc<RefCell<BuildState>>, keymap_tbl: &Table) -> mlua::Result<()> {
    keymap_tbl.set(
        "del",
        lua.create_function(move |_lua_inner, (context, key): (String, String)| {
            let mut b = build.borrow_mut();
            let sequence = b.next_keymap_sequence;
            b.next_keymap_sequence += 1;
            b.keymap_dels.push(RawKeymapDel {
                context,
                key,
                sequence,
            });
            Ok(())
        })?,
    )?;
    Ok(())
}

fn install_list(lua: &Lua, keymaps: SharedKeymapRegistry, keymap_tbl: &Table) -> mlua::Result<()> {
    keymap_tbl.set(
        "list",
        lua.create_function(move |lua_inner, ()| {
            let summaries = keymaps.borrow().summaries();
            let json: Vec<serde_json::Value> = summaries
                .into_iter()
                .map(|s| {
                    serde_json::json!({
                        "context": s.context,
                        "key": s.key,
                        "command": s.command,
                        "plugin_id": s.plugin_id,
                        "source": s.source,
                        "status": s.status,
                        "description": s.description,
                        "conflict_with": s.conflict_with.map(|c| serde_json::json!({
                            "plugin_id": c.plugin_id,
                            "source": c.source,
                        })),
                    })
                })
                .collect();
            lua_inner.to_value(&json)
        })?,
    )?;
    Ok(())
}

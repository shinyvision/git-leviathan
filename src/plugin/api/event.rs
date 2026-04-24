//! `leviathan.api.create_autocmd` — plugin-side event subscription.
//!
//! Mirrors Neovim's autocmd shape: plugins declare "when event X fires,
//! call this function". The host defines the event catalogue (emitted
//! from the app's `FetchPolicy`, etc.) and fires them as state changes.
//!
//! ## ELM discipline
//!
//! Callbacks are expected to mutate Lua-side plugin state (a closed-over
//! upvalue, a module table, etc.) — *not* the iced UI directly. After
//! every fire, the host re-invokes each affected plugin's dynamic widget
//! functions so the main bar reflects the new state. This keeps the view
//! a pure function of (Rust state + plugin Lua state), even when plugins
//! react to events.
//!
//! Usage:
//!
//! ```lua
//! local fetching = false
//! leviathan.api.create_autocmd({ "FetchStart" }, {
//!   callback = function() fetching = true end,
//! })
//! leviathan.api.create_autocmd({ "FetchEnd" }, {
//!   callback = function() fetching = false end,
//! })
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Table};

use super::{BuildState, RawAutocmd};

pub fn install(lua: &Lua, build: Rc<RefCell<BuildState>>, leviathan: &Table) -> mlua::Result<()> {
    let api = match leviathan.get::<Table>("api") {
        Ok(t) => t,
        Err(_) => {
            let t = lua.create_table()?;
            leviathan.set("api", t.clone())?;
            t
        }
    };

    let build_for_autocmd = Rc::clone(&build);
    api.set(
        "create_autocmd",
        lua.create_function(move |lua_inner, (events, opts): (Table, Table)| {
            let callback: Function = opts.get("callback")?;
            // One registry entry per (event, callback) pair — mlua
            // RegistryKey is not Clone, and creating N keys for the same
            // function is cheap (all point into the same Lua registry).
            for ev in events.sequence_values::<String>() {
                let ev = ev?;
                let key = lua_inner.create_registry_value(callback.clone())?;
                build_for_autocmd
                    .borrow_mut()
                    .autocmds
                    .push(RawAutocmd { event: ev, callback: key });
            }
            Ok(())
        })?,
    )?;

    Ok(())
}

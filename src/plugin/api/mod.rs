//! Lua-facing API surface exposed under the `leviathan.*` global.
//!
//! One submodule per namespace (SOLID single-responsibility):
//! - `ui` — main-bar buttons, screen registration
//! - `fs` — filesystem operations
//! - `event` — `leviathan.api.create_autocmd` event subscription
//!
//! `install_all` mounts them on a fresh `leviathan` table. Callable state
//! that must survive `init.lua` exec (button/screen handlers, autocmd
//! callbacks) is captured in a shared `BuildState`; the host drains it
//! after exec.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Lua, RegistryKey};

pub mod event;
pub mod fs;
pub mod repository;
pub mod ui;

pub struct ScreenDef {
    pub init: RegistryKey,
    pub view: RegistryKey,
    pub update: RegistryKey,
}

/// Raw slot spec as captured from Lua during `init.lua` execution.
///
/// The `widget` is either a plugin-supplied widget tree (same DSL as
/// plugin screens) serialised to `serde_json::Value`, or a Lua function
/// the host re-invokes to produce one. See [`WidgetSource`]. The slot is
/// fully widget-type-agnostic: the host doesn't know whether a slot is an
/// `icon+text` button, a bordered pill, a plain label, or a layout of
/// nested containers — that's all the plugin's decision.
///
/// `on_click` is stashed as a registry key so the slot's handler can be
/// invoked later (fired from clickable widgets inside `widget` that
/// dispatch through the main-bar-slot scope).
pub struct RawSlotSpec {
    pub id: String,
    pub section: String,
    pub priority: i32,
    pub widget: WidgetSource,
    pub on_click: Option<RegistryKey>,
}

/// Where a slot's widget tree comes from.
///
/// - `Static` — the plugin declared a literal table once at init. The
///   host keeps it verbatim and builds from it each render.
/// - `Dynamic` — the plugin declared a function. The host re-invokes it
///   whenever plugin-observable state might have changed (autocmd
///   callbacks) and caches the resulting tree; see
///   [`PluginHost::fire_event`](crate::plugin::PluginHost).
pub enum WidgetSource {
    Static(serde_json::Value),
    Dynamic(RegistryKey),
}

/// One hook operation as issued by a plugin's init.lua. Kept in source
/// order so that within a single plugin, a remove issued after an add
/// targets the already-added slot (not the other way around).
pub enum RawSlotOp {
    Add(RawSlotSpec),
    Remove(String),
    Replace(String, RawSlotSpec),
}

/// One `leviathan.api.create_autocmd` subscription captured during init.
/// One per (event, callback) pair — a single create_autocmd call with an
/// array of events emits one `RawAutocmd` per event so dispatch is a
/// plain lookup by event name.
pub struct RawAutocmd {
    pub event: String,
    pub callback: RegistryKey,
}

#[derive(Default)]
pub struct BuildState {
    pub screens: HashMap<String, ScreenDef>,
    /// Ordered hook operations from `leviathan.ui.main_bar.{add,remove,replace}`.
    pub slot_ops: Vec<RawSlotOp>,
    /// Autocmd subscriptions from `leviathan.api.create_autocmd`.
    pub autocmds: Vec<RawAutocmd>,
}

pub fn install_all(lua: &Lua, build: Rc<RefCell<BuildState>>) -> mlua::Result<()> {
    let leviathan = lua.create_table()?;

    ui::install(lua, Rc::clone(&build), &leviathan)?;
    event::install(lua, Rc::clone(&build), &leviathan)?;
    fs::install(lua, &leviathan)?;

    // Start with an empty repository snapshot so plugin code that touches
    // `leviathan.repository` at `init.lua` time (before the first sync)
    // never trips on `nil`. The host overwrites this on every sync.
    leviathan.set("repository", repository::build_table(lua, "", "", &[])?)?;

    leviathan.set(
        "log",
        lua.create_function(|_, msg: String| {
            eprintln!("git_leviathan plugin: {msg}");
            Ok(())
        })?,
    )?;

    lua.globals().set("leviathan", leviathan)?;
    Ok(())
}

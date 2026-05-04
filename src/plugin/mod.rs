//! Lua plugin system.
//!
//! Plugins live in `./plugins/<name>/` alongside the binary's CWD. Each carries
//! `plugin.toml` (metadata) + `init.lua` (entry point). On startup, `PluginHost`
//! loads every plugin, runs its `init.lua` in a dedicated `mlua::Lua` state, and
//! exposes the `leviathan.*` API so the plugin can contribute main-bar buttons,
//! screens, and (eventually) overlays/keybinds.
//!
//! All Lua calls happen synchronously on the main thread from `App::update`.
//! The Lua state is never passed across tasks; plugin side-effects come back
//! as typed `PluginMessage` variants.

pub mod activation;
pub mod api;
pub mod async_jobs;
pub mod audit;
pub mod bridge;
pub mod capabilities;
pub mod capability_grants;
pub mod commands;
pub mod core_commands;
pub mod dependency;
pub mod devtools;
pub mod devtools_commands;
pub mod diagnostic;
pub mod dock;
pub mod events;
pub mod extensions;
pub mod generation;
pub mod git_ops;
pub mod host;
pub mod keymap;
pub mod lua_loader;
pub mod message;
pub mod navigation;
pub mod performance;
pub mod persist;
pub mod reload;
pub mod resources;
pub mod runtime_path;
pub mod secrets;
pub mod services;
pub mod settings;
pub mod slots;
pub mod staged_reload;
pub mod storage;
pub mod tab_snapshot;
pub mod timers;
pub mod ui;
pub mod watchers;

pub use host::PluginHost;
pub use message::PluginMessage;

#[cfg(test)]
pub mod tests;

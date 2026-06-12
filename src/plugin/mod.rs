//! Lua plugin system.
//!
//! User plugins live under the app config root in `plugins/<name>/`. The config
//! root owns a bootstrap `init.lua`; startup runs that file and lets Lua decide
//! which plugin packages to load through `leviathan.plugins.*`. Each plugin
//! package carries `plugin.toml` (metadata) + `init.lua` (entry point), and the
//! host runs each plugin entry point in a dedicated `mlua::Lua` state with the
//! `leviathan.*` API installed.
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
pub mod commit_data;
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
pub mod terminal;
pub mod timers;
pub mod ui;
pub mod util;
pub mod watchers;

pub use host::PluginHost;
pub use message::PluginMessage;

#[cfg(test)]
pub mod tests;

//! Lua plugin system (prototype).
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

pub mod api;
pub mod bridge;
pub mod host;
pub mod message;
pub mod slots;
pub mod tab_snapshot;
pub mod ui;

pub use host::PluginHost;
pub use message::PluginMessage;

#[cfg(test)]
pub mod tests;

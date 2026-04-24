//! Main-bar — data-driven slot registry.
//!
//! Every item on the main bar (Pull, Push, Branch, Stash, Pop, Search, repo
//! info, plugin buttons, …) is a [`MainBarSlot`] registered in a
//! [`MainBarRegistry`]. Built-in slots are contributed by
//! [`builtins::register_all`]; plugins contribute via adapters in
//! `crate::plugin` that emit the same slot type.
//!
//! At render time [`view::main_bar_view`] walks the registry, groups slots
//! by [`Section`], sorts each section by priority, and composes the final
//! toolbar — preserving the exact spacing/padding of the pre-refactor
//! monolithic `main_bar_view`.
//!
//! ## Addressing
//!
//! Slots are identified by string `id` (e.g. `"builtin.push"`). Plugins
//! modify the bar by adding new ids or calling [`MainBarRegistry::remove`]
//! / [`MainBarRegistry::replace`] on existing ids.

pub mod builtins;
mod registry;
mod slot;
pub mod style;
mod view;

pub use registry::MainBarRegistry;
pub use slot::{MainBarSlot, Section, SlotCtx};
pub use view::main_bar_view;

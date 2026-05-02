//! Plugin-extensible chrome region: the tab bar.
//!
//! Same shape as `widgets::chrome::main_bar` but no built-ins yet —
//! `tab_bar_view` keeps its hardcoded "+" / version-label chrome and
//! plugins inject leading/trailing widgets via this registry.

pub mod builtins;
mod registry;

pub use registry::{iter_section, Section, TabBarCtx, TabBarRegistry, TabBarSlot};

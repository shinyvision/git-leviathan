//! Main-bar — slot-registry-driven.

pub mod builtins;
mod registry;
mod slot;
pub mod style;
mod view;

pub use registry::MainBarRegistry;
pub use slot::{MainBarSlot, SlotCtx};
pub use view::main_bar_view;

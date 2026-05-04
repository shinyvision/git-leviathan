pub mod address;
pub mod builtins;
pub mod registry;

pub use address::{Container, SlotAddress, BUILTIN_PLUGIN_ID};
pub use registry::{IsSlot, SlotRegistry};

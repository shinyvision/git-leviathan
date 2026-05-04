//! Plugin loading, callback dispatch, and resource ownership.

mod core;
pub(crate) mod region_mount;
mod slots;
mod types;

pub(crate) use slots::{prepare_op, validate_raw_slot_op};
pub use types::{PluginHost, PluginLoadError};

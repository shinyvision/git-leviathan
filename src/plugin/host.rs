//! Plugin loading, callback dispatch, and resource ownership.

mod core;
mod slots;
mod types;

pub(crate) use slots::prepare_op;
pub use types::{PluginHost, PluginLoadError};

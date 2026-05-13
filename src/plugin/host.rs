//! Plugin loading, callback dispatch, and resource ownership.

use crate::services::RepoRef;

mod core;
pub(crate) mod region_mount;
mod slots;
mod types;

pub(crate) use slots::{prepare_op, validate_raw_slot_op};
pub use types::{PluginHost, PluginLoadError};

#[derive(Clone, Copy)]
pub(crate) struct RepositorySyncState<'a> {
    pub(crate) repo_name: &'a str,
    pub(crate) workdir_path: &'a str,
    pub(crate) current_branch_name: &'a str,
    pub(crate) head_hash: &'a str,
    pub(crate) default_remote_name: &'a str,
    pub(crate) remote_names: &'a [String],
    pub(crate) refs: &'a [RepoRef],
}

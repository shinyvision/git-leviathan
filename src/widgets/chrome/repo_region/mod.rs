//! Plugin-extensible content region: the repository view.

mod registry;

#[allow(unused_imports)]
pub use registry::{
    container, is_empty, iter, Pane, RepoPaneCtx, RepoPaneSlot, RepoRegionRegistry, Section,
};

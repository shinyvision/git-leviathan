//! Plugin-extensible content region: the repository view.

mod registry;

pub use registry::{
    render_bottom, render_top, Pane, RepoPaneCtx, RepoPaneSlot, RepoRegionRegistry,
};

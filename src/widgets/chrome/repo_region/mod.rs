//! Plugin-extensible content region: the repository view.

mod chrome;
mod registry;

pub use chrome::{ChromePane, RepoChromeRegistry, RepoChromeSlot};
pub use registry::{
    render_bottom, render_top, Pane, RepoPaneCtx, RepoPaneSlot, RepoRegionRegistry,
};

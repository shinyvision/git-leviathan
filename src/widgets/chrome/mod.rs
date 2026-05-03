//! Top and bottom chrome bars: menu / tabs / main-bar / status.

pub mod main_bar;
pub mod repo_region;
mod repo_tab_bar;
pub mod tab_bar_slots;

pub use main_bar::{main_bar_view, MainBarRegistry, SlotCtx};
pub use repo_tab_bar::tab_bar_view;

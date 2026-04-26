//! Top and bottom chrome bars: menu / tabs / main-bar / status.

pub mod main_bar;
mod repo_tab_bar;

pub use main_bar::{main_bar_view, MainBarRegistry, SlotCtx};
pub use repo_tab_bar::tab_bar_view;

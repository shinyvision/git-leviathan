//! Generic tab-bar widget. Domain-agnostic — items, callbacks, and slot
//! content are supplied by the caller. The repository-specific consumer lives
//! in `widgets::chrome::repo_tab_bar`.

mod state;
mod tracker;
mod widget;

pub use widget::{TabBar, TabItem};

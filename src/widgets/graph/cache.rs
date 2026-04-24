use std::cell::{Cell, RefCell};

use iced::{widget::canvas, Renderer};

use crate::core::RepoVersion;

pub const TILE_ROWS: usize = 16;

#[derive(Debug, Default)]
pub struct FullGraphState {
    pub(super) tiles: RefCell<Vec<canvas::Cache<Renderer>>>,
    pub(super) selection_cache: canvas::Cache<Renderer>,
    pub(super) last_graph_revision: Cell<RepoVersion>,
    pub(super) last_row_count: Cell<usize>,
    pub(super) last_selected_indices: RefCell<Vec<usize>>,
}

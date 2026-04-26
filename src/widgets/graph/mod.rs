//! Commit-graph widget: `canvas::Program` impl, painting primitives,
//! tile cache, and lane-painter helpers.

pub mod cache;
pub mod lane_painter;
pub mod program;
pub mod rendering;

pub use program::full_graph_widget;

pub fn graph_lanes_width(num_lanes: usize) -> f32 {
    num_lanes as f32 * crate::theme::LANE_WIDTH
}

pub fn graph_column_width(num_lanes: usize) -> f32 {
    graph_lanes_width(num_lanes) + crate::theme::GRAPH_COL_GUTTER
}

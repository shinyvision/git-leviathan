//! Commit-graph widget: `canvas::Program` impl, painting primitives,
//! tile cache, and lane-painter helpers.

pub mod cache;
pub mod lane_painter;
pub mod program;
pub mod rendering;

pub use program::full_graph_widget;

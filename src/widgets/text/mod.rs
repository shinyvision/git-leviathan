//! Generic row-based text canvas, split into layers:
//! - `layout`: row data model + monospace metrics + canvas id
//! - `selection`: selection state + text extraction
//! - `hit_test`: pixel → (row, col) position resolution
//! - `canvas`: the `canvas::Program` impl + widget factories

pub mod canvas;
pub mod hit_test;
pub mod layout;
pub mod selection;

pub use canvas::{content_canvas, gutter_canvas, shift_wheel_lock, CanvasCallbacks};
pub use layout::{
    char_cells, mono_char_width, text_cells, CanvasId, CanvasRow, TextCanvasData, VisibleRowRange,
    CONTENT_PAD_X, DEFAULT_CONTENT_LINE_HEIGHT,
};
pub use selection::{selection_to_text, SelectionMode, TextPosition, TextSelection};

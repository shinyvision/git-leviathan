//! Pixel→(row, col) resolution for the text canvas.
use iced::Point;

use super::layout::{TextCanvasData, CONTENT_PAD_X};
use super::selection::TextPosition;

impl TextCanvasData {
    /// Binary-search `row_offsets` for the row containing `y` (content space).
    pub(super) fn row_at_y(&self, y: f32) -> Option<usize> {
        if self.rows.is_empty() {
            return None;
        }
        let y = y.max(0.0);
        let offs = &self.row_offsets;
        let mut lo = 0usize;
        let mut hi = self.rows.len();
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if offs[mid] <= y {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(lo.min(self.rows.len().saturating_sub(1)))
    }

    /// Hit-test a content-space point (origin top-left of the content
    /// canvas). Clamps column to the row's char_count.
    pub fn hit_test(&self, point: Point) -> Option<TextPosition> {
        let row = self.row_at_y(point.y)?;
        let col = pixel_to_col(point.x, self.char_width, self.rows[row].char_count());
        Some(TextPosition { row, col })
    }
}

/// Convert a content-x pixel to a column index, clamped to `char_count`.
pub fn pixel_to_col(x: f32, char_width: f32, char_count: usize) -> usize {
    let rel_x = (x - CONTENT_PAD_X).max(0.0);
    let raw_col = (rel_x / char_width).round() as usize;
    raw_col.min(char_count)
}

/// Convert a column index to a content-x pixel (left edge of that column).
pub fn col_to_pixel(col: usize, char_width: f32) -> f32 {
    CONTENT_PAD_X + col as f32 * char_width
}

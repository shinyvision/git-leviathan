//! Pixel→(row, col) resolution for the text canvas.
use iced::Point;

use super::layout::{char_cells, cells_before, TextCanvasData, CONTENT_PAD_X};
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
        let text = self.rows[row].raw_text();
        let col = pixel_to_col(point.x, self.char_width, &text);
        Some(TextPosition { row, col })
    }
}

/// Convert a content-x pixel to a character column, accounting for
/// double-width glyphs. Clamped to the character count of `text`.
pub fn pixel_to_col(x: f32, char_width: f32, text: &str) -> usize {
    if char_width <= 0.0 {
        return 0;
    }
    // Target position expressed in display cells (each cell is `char_width`).
    let target_cells = ((x - CONTENT_PAD_X).max(0.0) / char_width).round() as usize;
    let mut acc = 0usize;
    for (idx, ch) in text.chars().enumerate() {
        let w = char_cells(ch);
        // Land on this char while the cursor is within its left half; past the
        // midpoint it belongs to the next column.
        if target_cells < acc + w.div_ceil(2) {
            return idx;
        }
        acc += w;
    }
    text.chars().count()
}

/// Convert a character column to a content-x pixel (left edge of that
/// column), accounting for double-width glyphs before it.
pub fn col_to_pixel(col: usize, char_width: f32, text: &str) -> f32 {
    CONTENT_PAD_X + cells_before(text, col) as f32 * char_width
}

#[cfg(test)]
mod tests {
    use super::*;

    const CW: f32 = 8.0;

    #[test]
    fn ascii_is_a_no_op_versus_uniform_width() {
        // For pure ASCII, col N sits at PAD + N*char_width and the pixel maps
        // back to col N — identical to the old uniform model (no regression).
        let text = "hello world";
        for col in 0..=text.chars().count() {
            let x = col_to_pixel(col, CW, text);
            assert_eq!(x, CONTENT_PAD_X + col as f32 * CW);
            assert_eq!(pixel_to_col(x, CW, text), col);
        }
    }

    #[test]
    fn wide_glyphs_advance_two_cells_and_round_trip() {
        // "日本X": 日 and 本 are double-width, X is single.
        let text = "日本X";
        // Left edge of each column in cells: [0, 2, 4, 5].
        assert_eq!(col_to_pixel(0, CW, text), CONTENT_PAD_X);
        assert_eq!(col_to_pixel(1, CW, text), CONTENT_PAD_X + 2.0 * CW);
        assert_eq!(col_to_pixel(2, CW, text), CONTENT_PAD_X + 4.0 * CW);
        assert_eq!(col_to_pixel(3, CW, text), CONTENT_PAD_X + 5.0 * CW);

        // Clicking within a wide glyph's left half lands on it; past the
        // midpoint advances to the next column.
        assert_eq!(pixel_to_col(col_to_pixel(0, CW, text), CW, text), 0);
        assert_eq!(pixel_to_col(col_to_pixel(1, CW, text), CW, text), 1);
        assert_eq!(pixel_to_col(col_to_pixel(2, CW, text), CW, text), 2);
        // A click one cell into the first wide glyph still resolves to col 0.
        assert_eq!(pixel_to_col(CONTENT_PAD_X + CW * 0.4, CW, text), 0);
    }

    #[test]
    fn pixel_to_col_clamps_past_end() {
        let text = "ab";
        assert_eq!(pixel_to_col(CONTENT_PAD_X + 100.0 * CW, CW, text), 2);
    }
}

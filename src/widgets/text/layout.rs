//! Row layout and monospace metrics.
use std::{any::Any, ops::Range, sync::Arc};

use iced::widget::canvas::Frame;

use crate::theme;

pub const DEFAULT_CONTENT_LINE_HEIGHT: f32 = 20.0;
pub const CONTENT_PAD_X: f32 = 10.0;

/// Opaque canvas identifier. Consumers assign their own ids (e.g. via
/// constants) so events can be routed back to the right buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CanvasId(pub u32);

/// A row in the canvas. Implementations decide their own gutter + content
/// rendering. The generic widget composes virtualization, scroll, and
/// selection on top.
pub trait CanvasRow: std::fmt::Debug + Send + Sync + Any {
    fn as_any(&self) -> &dyn Any;

    fn height(&self) -> f32;

    fn selectable(&self) -> bool {
        false
    }

    /// Length (in characters) of the selectable text content for this row.
    /// Zero for non-selectable rows.
    fn char_count(&self) -> usize {
        0
    }

    /// Raw text for clipboard export + word-boundary detection. Empty for
    /// non-selectable rows.
    fn raw_text(&self) -> String {
        String::new()
    }

    /// Draw gutter cells for this row. `top_y` is the row's top in
    /// widget-local coords (scroll already subtracted by the caller).
    fn draw_gutter(&self, frame: &mut Frame, top_y: f32, gutter_width: f32);

    /// Draw content cells: line background, highlights, glyphs, etc.
    /// Selection highlight is overlaid by the widget in a later pass.
    fn draw_content(&self, frame: &mut Frame, top_y: f32, width: f32, char_width: f32);

    /// Detect a click on a gutter hot spot. `local_x` is relative to the
    /// gutter origin (0..gutter_width); `local_y` is relative to the row's
    /// top. Returning `Some(meta)` signals a handled click and the canvas
    /// will emit a gutter-click event carrying the meta value.
    fn gutter_click(&self, _local_x: f32, _local_y: f32, _gutter_width: f32) -> Option<u64> {
        None
    }

    /// Whether the cursor at this point should show a pointer hint (used
    /// for clickable gutter elements).
    fn gutter_hover_interactive(&self, _local_x: f32, _local_y: f32, _gutter_width: f32) -> bool {
        false
    }
}

/// Precomputed layout for a set of rows. Immutable once built; cheap to
/// share via `Arc`.
#[derive(Debug)]
pub struct TextCanvasData {
    pub(in crate::widgets::text) rows: Vec<Arc<dyn CanvasRow>>,
    pub(in crate::widgets::text) row_offsets: Arc<[f32]>,
    pub(in crate::widgets::text) total_height: f32,
    pub(in crate::widgets::text) content_width: f32,
    pub(in crate::widgets::text) char_width: f32,
    pub(in crate::widgets::text) gutter_width: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleRowRange {
    pub visible: Range<usize>,
    pub with_overscan: Range<usize>,
}

impl TextCanvasData {
    pub fn from_rows(
        rows: Vec<Arc<dyn CanvasRow>>,
        content_width: f32,
        char_width: f32,
        gutter_width: f32,
    ) -> Self {
        let mut row_offsets = Vec::with_capacity(rows.len() + 1);
        let mut acc = 0.0f32;
        for r in &rows {
            row_offsets.push(acc);
            acc += r.height();
        }
        row_offsets.push(acc);
        Self {
            rows,
            row_offsets: Arc::from(row_offsets),
            total_height: acc,
            content_width,
            char_width,
            gutter_width,
        }
    }

    pub fn first_row_at_or_below(&self, scroll_y: f32) -> usize {
        self.row_offsets
            .partition_point(|&y| y < scroll_y)
            .min(self.rows.len())
    }

    pub fn visible_row_range(
        &self,
        scroll_y: f32,
        viewport_height: f32,
        overscan_rows: usize,
    ) -> VisibleRowRange {
        let row_count = self.rows.len();
        if row_count == 0 || viewport_height <= 0.0 {
            return VisibleRowRange {
                visible: 0..0,
                with_overscan: 0..0,
            };
        }

        let top = scroll_y.max(0.0).min(self.total_height);
        let bottom = (top + viewport_height).min(self.total_height);
        if bottom <= top {
            return VisibleRowRange {
                visible: row_count..row_count,
                with_overscan: row_count..row_count,
            };
        }

        let start = self.row_offsets[1..].partition_point(|row_bottom| *row_bottom <= top);
        let end = self.row_offsets[..row_count].partition_point(|row_top| *row_top < bottom);
        let visible = start.min(row_count)..end.max(start).min(row_count);
        let with_overscan = visible.start.saturating_sub(overscan_rows)
            ..(visible.end + overscan_rows).min(row_count);

        VisibleRowRange {
            visible,
            with_overscan,
        }
    }

    pub fn row_top_y(&self, row: usize) -> f32 {
        self.row_offsets.get(row).copied().unwrap_or(0.0)
    }

    pub fn content_width(&self) -> f32 {
        self.content_width
    }

    pub fn total_height(&self) -> f32 {
        self.total_height
    }

    pub fn gutter_width(&self) -> f32 {
        self.gutter_width
    }

    pub fn rows(&self) -> &[Arc<dyn CanvasRow>] {
        &self.rows
    }

    pub fn with_content_width(&self, content_width: f32) -> Self {
        Self {
            rows: self.rows.clone(),
            row_offsets: self.row_offsets.clone(),
            total_height: self.total_height,
            content_width,
            char_width: self.char_width,
            gutter_width: self.gutter_width,
        }
    }
}

/// Display cell width of a single character in the monospace grid. Wide
/// glyphs (CJK, fullwidth forms, most emoji) occupy two cells under
/// `Shaping::Advanced`; everything else — including ASCII and zero-width
/// combining marks — is treated as one. Deliberately only *widens* genuinely
/// double-width chars so ASCII/narrow text keeps `cells == char index`
/// (a no-op for the overwhelmingly common case).
pub fn char_cells(ch: char) -> usize {
    use unicode_width::UnicodeWidthChar;
    if ch.width().unwrap_or(1) >= 2 {
        2
    } else {
        1
    }
}

/// Sum of display cells of the first `col` characters of `text` (i.e. the
/// cell offset of the char at index `col`). For pure-ASCII text this equals
/// `col` — checked up front so the common case skips the per-char width walk.
pub fn cells_before(text: &str, col: usize) -> usize {
    if text.is_ascii() {
        return col.min(text.len());
    }
    text.chars().take(col).map(char_cells).sum()
}

/// Total display cells of `text`. ASCII text (1 byte == 1 char == 1 cell) is
/// short-circuited so hot draw paths don't pay a per-char unicode-width lookup.
pub fn text_cells(text: &str) -> usize {
    if text.is_ascii() {
        return text.len();
    }
    text.chars().map(char_cells).sum()
}

/// One-glyph width for the mono content font, cached after first measurement.
pub fn mono_char_width() -> f32 {
    use iced::advanced::graphics::text::{self as graphics_text, cosmic_text};
    use std::sync::OnceLock;
    static CACHED: OnceLock<f32> = OnceLock::new();
    *CACHED.get_or_init(|| {
        let mut font_system = graphics_text::font_system().write().expect("font system");
        let metrics = cosmic_text::Metrics::new(theme::FONT_DIFF, theme::FONT_DIFF * 1.2);
        let mut buffer = cosmic_text::Buffer::new(font_system.raw(), metrics);
        buffer.set_wrap(font_system.raw(), cosmic_text::Wrap::None);
        buffer.set_size(font_system.raw(), None, None);
        let attrs = graphics_text::to_attributes(theme::MONO);
        buffer.set_text(
            font_system.raw(),
            "M",
            &attrs,
            graphics_text::to_shaping(iced::advanced::text::Shaping::Advanced, "M"),
            None,
        );
        let (size, _) = graphics_text::measure(&buffer);
        if size.width > 0.0 {
            size.width
        } else {
            theme::FONT_DIFF * 0.6
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct StubRow(f32);

    impl CanvasRow for StubRow {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn height(&self) -> f32 {
            self.0
        }

        fn draw_gutter(&self, _: &mut Frame, _: f32, _: f32) {}

        fn draw_content(&self, _: &mut Frame, _: f32, _: f32, _: f32) {}
    }

    fn data_from_heights(heights: &[f32]) -> TextCanvasData {
        let rows = heights
            .iter()
            .map(|height| Arc::new(StubRow(*height)) as Arc<dyn CanvasRow>)
            .collect();
        TextCanvasData::from_rows(rows, 100.0, 8.0, 40.0)
    }

    #[test]
    fn visible_row_range_includes_intersecting_rows_and_overscan() {
        let data = data_from_heights(&[10.0, 20.0, 30.0, 40.0, 50.0]);

        let range = data.visible_row_range(25.0, 50.0, 1);

        assert_eq!(range.visible, 1..4);
        assert_eq!(range.with_overscan, 0..5);
    }

    #[test]
    fn visible_row_range_respects_row_boundaries() {
        let data = data_from_heights(&[10.0, 20.0, 30.0, 40.0]);

        let range = data.visible_row_range(30.0, 30.0, 0);

        assert_eq!(range.visible, 2..3);
        assert_eq!(range.with_overscan, 2..3);
    }

    #[test]
    fn visible_row_range_clamps_at_end() {
        let data = data_from_heights(&[10.0, 20.0, 30.0]);

        let range = data.visible_row_range(55.0, 40.0, 2);

        assert_eq!(range.visible, 2..3);
        assert_eq!(range.with_overscan, 0..3);
    }
}

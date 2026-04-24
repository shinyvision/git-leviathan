//! Row layout and monospace metrics.
use std::sync::Arc;

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
pub trait CanvasRow: std::fmt::Debug + Send + Sync {
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
    pub rows: Vec<Arc<dyn CanvasRow>>,
    pub row_offsets: Vec<f32>,
    pub total_height: f32,
    pub content_width: f32,
    pub char_width: f32,
    pub gutter_width: f32,
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
            row_offsets,
            total_height: acc,
            content_width,
            char_width,
            gutter_width,
        }
    }
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

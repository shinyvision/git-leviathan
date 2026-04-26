//! Diff-view rendering on top of the generic `text_canvas`.
//!
//! Defines the `DiffRow` model (hunk headers, file headers, content lines,
//! Eofnl markers) and its gutter rendering (two line-number columns plus the
//! +/- sign). Everything about selection, virtualization, scroll, and hit
//! testing lives in `text_canvas` — this module just populates rows.
use std::sync::Arc;

use iced::{
    advanced::text::{LineHeight, Shaping},
    alignment,
    widget::canvas::{Frame, Text},
    Color, Element, Pixels, Point, Size,
};

use crate::{
    message::Message,
    screens::repository::{panel_messages::DiffPanelAction, RepositoryMessage},
    services::{SegmentKind, SyntaxHighlightedSpan},
    theme,
    widgets::text::{
        self, CanvasCallbacks, CanvasId, CanvasRow, TextCanvasData, TextSelection,
        CONTENT_PAD_X, DEFAULT_CONTENT_LINE_HEIGHT,
    },
};

const CONTENT_LINE_HEIGHT: f32 = DEFAULT_CONTENT_LINE_HEIGHT;
pub const HUNK_HEADER_HEIGHT: f32 = 28.0; // content + 8px top spacing
const GUTTER_WIDTH: f32 = 90.0;
pub const LINE_NUM_WIDTH: f32 = 40.0;
pub const SIGN_WIDTH: f32 = 16.0;

pub fn available_content_width(total_width: f32) -> f32 {
    (total_width - GUTTER_WIDTH).max(0.0)
}

pub fn search_scroll_breadcrumb_offset() -> f32 {
    3.0 * CONTENT_LINE_HEIGHT
}

pub const CANVAS_ID: CanvasId = CanvasId(1);

// Legacy re-exports for existing call sites.
pub use text::{
    CanvasId as DiffCanvasId, SelectionMode, TextCanvasData as DiffCanvasData,
    TextPosition as DiffPosition, TextSelection as DiffSelection,
};

pub fn diff_char_width() -> f32 {
    text::mono_char_width()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone)]
pub struct SegmentBg {
    pub start_col: usize,
    pub end_col: usize,
    pub kind: SegmentKind,
}

#[derive(Debug, Clone)]
pub enum DiffRow {
    HunkHeader(String),
    FileHeader(String),
    Content {
        old_lineno: Option<u32>,
        new_lineno: Option<u32>,
        kind: LineKind,
        spans: Vec<SyntaxHighlightedSpan>,
        segment_bgs: Vec<SegmentBg>,
        char_count: usize,
    },
    Eofnl(LineKind),
}

impl CanvasRow for DiffRow {
    fn height(&self) -> f32 {
        match self {
            DiffRow::HunkHeader(_) => HUNK_HEADER_HEIGHT,
            DiffRow::FileHeader(_) => CONTENT_LINE_HEIGHT,
            DiffRow::Content { .. } | DiffRow::Eofnl(_) => CONTENT_LINE_HEIGHT,
        }
    }

    fn selectable(&self) -> bool {
        matches!(self, DiffRow::Content { .. })
    }

    fn char_count(&self) -> usize {
        match self {
            DiffRow::Content { char_count, .. } => *char_count,
            _ => 0,
        }
    }

    fn raw_text(&self) -> String {
        match self {
            DiffRow::Content { spans, .. } => spans.iter().map(|s| s.text.as_str()).collect(),
            _ => String::new(),
        }
    }

    fn draw_gutter(&self, frame: &mut Frame, top_y: f32, _gutter_width: f32) {
        match self {
            DiffRow::Content {
                old_lineno,
                new_lineno,
                kind,
                ..
            } => {
                if let Some(n) = old_lineno {
                    draw_gutter_num(frame, 4.0, top_y, LINE_NUM_WIDTH, *n);
                }
                if let Some(n) = new_lineno {
                    draw_gutter_num(frame, LINE_NUM_WIDTH + 4.0, top_y, LINE_NUM_WIDTH, *n);
                }
                draw_sign(frame, 2.0 * LINE_NUM_WIDTH, top_y, *kind);
            }
            DiffRow::Eofnl(kind) => {
                draw_sign(frame, 2.0 * LINE_NUM_WIDTH, top_y, *kind);
            }
            _ => {}
        }
    }

    fn draw_content(&self, frame: &mut Frame, top_y: f32, width: f32, char_width: f32) {
        let h = self.height();
        match self {
            DiffRow::HunkHeader(content) => {
                let top_pad = 8.0;
                frame.fill_rectangle(
                    Point::new(0.0, top_y + top_pad),
                    Size::new(width, h - top_pad),
                    theme::BG_BASE,
                );
                draw_single_span(
                    frame,
                    50.0 - GUTTER_WIDTH + CONTENT_PAD_X,
                    top_y + top_pad + (h - top_pad) / 2.0,
                    content,
                    Color::WHITE,
                );
            }
            DiffRow::FileHeader(content) => {
                frame.fill_rectangle(
                    Point::new(0.0, top_y),
                    Size::new(width, h),
                    theme::BG_HEADER,
                );
                draw_single_span(frame, 10.0, top_y + h / 2.0, content, theme::TEXT_SECONDARY);
            }
            DiffRow::Content {
                kind,
                spans,
                segment_bgs,
                ..
            } => {
                let line_bg = line_bg_color(*kind);
                frame.fill_rectangle(Point::new(0.0, top_y), Size::new(width, h), line_bg);
                for seg in segment_bgs {
                    let bg = segment_bg_color(seg.kind, *kind);
                    if bg == line_bg {
                        continue;
                    }
                    let sx = CONTENT_PAD_X + seg.start_col as f32 * char_width;
                    let ex = CONTENT_PAD_X + seg.end_col as f32 * char_width;
                    frame.fill_rectangle(
                        Point::new(sx, top_y),
                        Size::new((ex - sx).max(0.0), h),
                        bg,
                    );
                }
                let mut x = CONTENT_PAD_X;
                let baseline_y = top_y + h / 2.0;
                for span in spans {
                    if span.text.is_empty() {
                        continue;
                    }
                    let color = span.style.color.unwrap_or(theme::TEXT_PRIMARY);
                    let char_count = span.text.chars().count();
                    frame.fill_text(Text {
                        content: span.text.clone(),
                        position: Point::new(x, baseline_y),
                        color,
                        size: Pixels(theme::FONT_DIFF),
                        line_height: LineHeight::Absolute(Pixels(h)),
                        font: theme::MONO,
                        align_x: iced::advanced::text::Alignment::Left,
                        align_y: alignment::Vertical::Center,
                        shaping: Shaping::Advanced,
                        max_width: f32::INFINITY,
                    });
                    x += char_count as f32 * char_width;
                }
            }
            DiffRow::Eofnl(kind) => {
                let line_bg = line_bg_color(*kind);
                frame.fill_rectangle(Point::new(0.0, top_y), Size::new(width, h), line_bg);
            }
        }
    }
}

fn draw_single_span(frame: &mut Frame, x: f32, y: f32, content: &str, color: Color) {
    frame.fill_text(Text {
        content: content.to_string(),
        position: Point::new(x, y),
        color,
        size: Pixels(theme::FONT_DIFF),
        line_height: LineHeight::Relative(1.0),
        font: theme::MONO,
        align_x: iced::advanced::text::Alignment::Left,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
        max_width: f32::INFINITY,
    });
}

fn draw_gutter_num(frame: &mut Frame, x: f32, y: f32, width: f32, num: u32) {
    frame.fill_text(Text {
        content: num.to_string(),
        position: Point::new(x + width - 4.0, y + CONTENT_LINE_HEIGHT / 2.0),
        color: theme::TEXT_DIM,
        size: Pixels(theme::FONT_XS),
        line_height: LineHeight::Relative(1.0),
        font: theme::MONO,
        align_x: iced::advanced::text::Alignment::Right,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
        max_width: f32::INFINITY,
    });
}

fn draw_sign(frame: &mut Frame, x: f32, y: f32, kind: LineKind) {
    let (txt, col) = match kind {
        LineKind::Context => (" ", theme::TEXT_DIM),
        LineKind::Addition => ("+", theme::ACCENT_GREEN),
        LineKind::Deletion => ("-", theme::ACCENT_RED),
    };
    frame.fill_text(Text {
        content: txt.to_string(),
        position: Point::new(x + SIGN_WIDTH / 2.0, y + CONTENT_LINE_HEIGHT / 2.0),
        color: col,
        size: Pixels(theme::FONT_DIFF),
        line_height: LineHeight::Relative(1.0),
        font: theme::MONO,
        align_x: iced::advanced::text::Alignment::Center,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Advanced,
        max_width: f32::INFINITY,
    });
}

fn line_bg_color(kind: LineKind) -> Color {
    match kind {
        LineKind::Context => theme::BG_PANEL,
        LineKind::Addition => theme::ADDITION_BG,
        LineKind::Deletion => theme::DELETION_BG,
    }
}

fn segment_bg_color(seg: SegmentKind, line: LineKind) -> Color {
    match (line, seg) {
        (LineKind::Addition, SegmentKind::AdditionHighlight) => theme::ADDITION_HIGHLIGHT_BG,
        (LineKind::Addition, _) => theme::ADDITION_BG,
        (LineKind::Deletion, SegmentKind::DeletionHighlight) => theme::DELETION_HIGHLIGHT_BG,
        (LineKind::Deletion, _) => theme::DELETION_BG,
        (LineKind::Context, _) => theme::BG_PANEL,
    }
}

/// Build a `TextCanvasData` from `DiffRow`s, ready to hand to the generic
/// canvas builders.
pub fn build_canvas_data(rows: Vec<DiffRow>, char_width: f32) -> Arc<TextCanvasData> {
    let content_width = compute_content_width(&rows, char_width);
    let rows_dyn: Vec<Arc<dyn CanvasRow>> = rows
        .into_iter()
        .map(|r| Arc::new(r) as Arc<dyn CanvasRow>)
        .collect();
    Arc::new(TextCanvasData::from_rows(
        rows_dyn,
        content_width,
        char_width,
        GUTTER_WIDTH,
    ))
}

fn compute_content_width(rows: &[DiffRow], char_w: f32) -> f32 {
    let mut max_chars = 0usize;
    for r in rows {
        if let DiffRow::Content { char_count, .. } = r {
            max_chars = max_chars.max(*char_count);
        }
    }
    CONTENT_PAD_X * 2.0 + max_chars as f32 * char_w + 40.0
}

/// Callbacks wired into the repository message enum. Same set is shared
/// between all `DiffPanel`-hosted canvases (diff view + conflict buffers);
/// `canvas_id` disambiguates them at the message layer.
pub fn diff_panel_callbacks() -> CanvasCallbacks<Message> {
    CanvasCallbacks {
        on_selection_begin: Arc::new(|canvas_id, pos, viewport_rect, data| {
            Message::repo(RepositoryMessage::DiffPanel(
                DiffPanelAction::DiffSelectionBegin {
                    canvas_id,
                    row: pos.row,
                    col: pos.col,
                    viewport_rect,
                    data,
                },
            ))
        }),
        on_selection_extend: Arc::new(|canvas_id, pos| {
            Message::repo(RepositoryMessage::DiffPanel(
                DiffPanelAction::DiffSelectionExtend {
                    canvas_id,
                    row: pos.row,
                    col: pos.col,
                },
            ))
        }),
        on_selection_end: Arc::new(|canvas_id| {
            Message::repo(RepositoryMessage::DiffPanel(
                DiffPanelAction::DiffSelectionEnd { canvas_id },
            ))
        }),
        on_gutter_click: Arc::new(|canvas_id, row, meta| {
            Message::repo(RepositoryMessage::DiffPanel(
                DiffPanelAction::DiffGutterClicked {
                    canvas_id,
                    row,
                    meta,
                },
            ))
        }),
    }
}

/// Convenience wrapper around the generic content canvas using diff
/// callbacks + the diff canvas id.
pub fn diff_content_canvas(
    data: Arc<TextCanvasData>,
    selection: Option<TextSelection>,
    viewport: iced::Size,
    bottom_pad: f32,
    scroll_y: f32,
) -> Element<'static, Message> {
    text::content_canvas(
        CANVAS_ID,
        data,
        selection,
        viewport,
        bottom_pad,
        scroll_y,
        diff_panel_callbacks(),
    )
}

pub fn diff_gutter_canvas(data: Arc<TextCanvasData>, scroll_y: f32) -> Element<'static, Message> {
    text::gutter_canvas(CANVAS_ID, data, scroll_y, diff_panel_callbacks())
}

/// Helper for clipboard selection export.
pub fn selection_text_for_rows(rows: &[DiffRow], selection: &TextSelection) -> String {
    // Walk rows directly (no need to build a canvas) so call sites that
    // only have a `Vec<DiffRow>` don't have to construct a full data.
    if selection.is_empty() {
        return String::new();
    }
    let (start, end) = selection.ordered();
    let mut pieces: Vec<String> = Vec::new();
    for (idx, row) in rows.iter().enumerate() {
        if idx < start.row || idx > end.row {
            continue;
        }
        if !CanvasRow::selectable(row) {
            continue;
        }
        let raw = CanvasRow::raw_text(row);
        let chars: Vec<char> = raw.chars().collect();
        let from = if idx == start.row { start.col } else { 0 };
        let to = if idx == end.row { end.col } else { chars.len() };
        let from = from.min(chars.len());
        let to = to.min(chars.len()).max(from);
        pieces.push(chars[from..to].iter().collect());
    }
    pieces.join("\n")
}


//! Conflict-resolution rendering on top of the generic `text_canvas`.
//!
//! Defines `ConflictRow` (a line in one of the three conflict buffers —
//! ours, theirs, output) and its gutter rendering: a colored side-mark bar
//! for hunk rows, an optional clickable checkbox to pick the hunk, and a
//! line-number column. Selection + scroll + virtualization live in
//! `text_canvas`; this module only supplies rows.
use std::sync::Arc;

use iced::{
    advanced::text::{LineHeight, Shaping},
    alignment,
    widget::canvas::{Frame, Text},
    Color, Element, Pixels, Point, Size,
};

use crate::{
    message::Message,
    services::{SyntaxHighlightedSpan, SyntaxStyle},
    theme,
    widgets::{
        diff_canvas::diff_panel_callbacks,
        palette::{self, PaletteRole},
        text::{
            self, CanvasId, CanvasRow, TextCanvasData, TextSelection, CONTENT_PAD_X,
            DEFAULT_CONTENT_LINE_HEIGHT,
        },
    },
};

pub const CONTENT_LINE_HEIGHT: f32 = DEFAULT_CONTENT_LINE_HEIGHT;

const GUTTER_WIDTH_SIDE: f32 = 86.0;
const GUTTER_WIDTH_OUTPUT: f32 = 56.0;

const CHECKBOX_X: f32 = 6.0;
const CHECKBOX_SIZE: f32 = 14.0;
const MARK_X: f32 = 26.0;
const MARK_WIDTH: f32 = 4.0;
const SIDE_LN_X: f32 = 32.0;
const SIDE_LN_WIDTH: f32 = 48.0;
const OUTPUT_LN_X: f32 = 4.0;
const OUTPUT_LN_WIDTH: f32 = 48.0;

pub const CANVAS_ID_OURS: CanvasId = CanvasId(2);
pub const CANVAS_ID_THEIRS: CanvasId = CanvasId(3);
pub const CANVAS_ID_OUTPUT: CanvasId = CanvasId(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictSide {
    Ours,
    Theirs,
}

pub fn side_color(side: ConflictSide) -> Color {
    palette::palette_color(match side {
        ConflictSide::Ours => PaletteRole::ConflictOurs,
        ConflictSide::Theirs => PaletteRole::ConflictTheirs,
    })
}

/// Which gutter layout to use. `Side` supplies a color for the mark bar;
/// `Output` has no side-tinting or checkbox column.
#[derive(Debug, Clone, Copy)]
pub enum GutterKind {
    Side(ConflictSide),
    Output,
}

/// One rendered row in a conflict buffer.
#[derive(Debug, Clone)]
pub struct ConflictRow {
    pub line_number: Option<u32>,
    pub spans: Vec<SyntaxHighlightedSpan>,
    pub char_count: usize,
    /// Which hunk this row belongs to (if any). Drives the mark bar +
    /// background tint.
    pub hunk_idx: Option<usize>,
    /// True iff the hunk this row belongs to is currently picked on this
    /// side — controls how strongly the row background is tinted.
    pub hunk_selected: bool,
    /// If set, a checkbox is drawn for this row; encodes the current
    /// checked state. `hunk_idx` carries the hunk index for the click
    /// event. Only meaningful on `GutterKind::Side` canvases.
    pub show_checkbox: bool,
    /// If true, row renders dim placeholder text (e.g. "(empty)").
    pub is_placeholder: bool,
    /// Which gutter variant to draw. Fixed per canvas but stored on the
    /// row so the trait impl is self-contained.
    pub gutter_kind: GutterKind,
}

impl ConflictRow {
    fn raw(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    fn bg_color(&self) -> Color {
        match self.gutter_kind {
            GutterKind::Side(side) => {
                if self.hunk_idx.is_none() {
                    theme::BG_PANEL
                } else if self.hunk_selected {
                    palette::mix(theme::BG_PANEL, side_color(side), 0.34)
                } else {
                    palette::mix(theme::BG_PANEL, side_color(side), 0.22)
                }
            }
            GutterKind::Output => theme::BG_PANEL,
        }
    }
}

impl CanvasRow for ConflictRow {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn height(&self) -> f32 {
        CONTENT_LINE_HEIGHT
    }

    fn selectable(&self) -> bool {
        !self.is_placeholder
    }

    fn char_count(&self) -> usize {
        if self.is_placeholder {
            0
        } else {
            self.char_count
        }
    }

    fn raw_text(&self) -> String {
        if self.is_placeholder {
            String::new()
        } else {
            self.raw()
        }
    }

    fn draw_gutter(&self, frame: &mut Frame, top_y: f32, _gutter_width: f32) {
        let h = CONTENT_LINE_HEIGHT;
        match self.gutter_kind {
            GutterKind::Side(side) => {
                if self.hunk_idx.is_some() {
                    frame.fill_rectangle(
                        Point::new(MARK_X, top_y),
                        Size::new(MARK_WIDTH, h),
                        side_color(side),
                    );
                }
                if self.show_checkbox {
                    let y_center = top_y + h / 2.0;
                    let rect_y = y_center - CHECKBOX_SIZE / 2.0;
                    draw_checkbox(frame, CHECKBOX_X, rect_y, CHECKBOX_SIZE, self.hunk_selected);
                }
                if let Some(n) = self.line_number {
                    draw_gutter_num(frame, SIDE_LN_X, top_y, SIDE_LN_WIDTH, n);
                }
            }
            GutterKind::Output => {
                if let Some(n) = self.line_number {
                    draw_gutter_num(frame, OUTPUT_LN_X, top_y, OUTPUT_LN_WIDTH, n);
                }
            }
        }
    }

    fn draw_content(&self, frame: &mut Frame, top_y: f32, width: f32, char_width: f32) {
        let h = CONTENT_LINE_HEIGHT;
        let bg = self.bg_color();
        frame.fill_rectangle(Point::new(0.0, top_y), Size::new(width, h), bg);

        let baseline_y = top_y + h / 2.0;
        let mut x = CONTENT_PAD_X;
        for span in &self.spans {
            if span.text.is_empty() {
                continue;
            }
            let color = if self.is_placeholder {
                theme::TEXT_DIM
            } else {
                span.style.color.unwrap_or(theme::TEXT_PRIMARY)
            };
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

    fn gutter_click(&self, local_x: f32, local_y: f32, _gutter_width: f32) -> Option<u64> {
        if !self.show_checkbox {
            return None;
        }
        let hunk_idx = self.hunk_idx?;
        if !matches!(self.gutter_kind, GutterKind::Side(_)) {
            return None;
        }
        let h = CONTENT_LINE_HEIGHT;
        let y_center = h / 2.0;
        let rect_y = y_center - CHECKBOX_SIZE / 2.0;
        if (CHECKBOX_X..=CHECKBOX_X + CHECKBOX_SIZE).contains(&local_x)
            && local_y >= rect_y
            && local_y <= rect_y + CHECKBOX_SIZE
        {
            Some(hunk_idx as u64)
        } else {
            None
        }
    }

    fn gutter_hover_interactive(&self, local_x: f32, local_y: f32, gutter_width: f32) -> bool {
        self.gutter_click(local_x, local_y, gutter_width).is_some()
    }
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

fn draw_checkbox(frame: &mut Frame, x: f32, y: f32, size: f32, checked: bool) {
    let border_color = if checked {
        theme::ACCENT_GREEN
    } else {
        theme::TEXT_DIM
    };
    let bg = if checked {
        theme::BG_SELECTED
    } else {
        theme::BG_BASE
    };
    frame.fill_rectangle(Point::new(x, y), Size::new(size, size), border_color);
    frame.fill_rectangle(
        Point::new(x + 1.0, y + 1.0),
        Size::new(size - 2.0, size - 2.0),
        bg,
    );
    if checked {
        frame.fill_text(Text {
            content: "✓".to_string(),
            position: Point::new(x + size / 2.0, y + size / 2.0 + 0.5),
            color: Color::WHITE,
            size: Pixels(size - 2.0),
            line_height: LineHeight::Relative(1.0),
            font: theme::MONO,
            align_x: iced::advanced::text::Alignment::Center,
            align_y: alignment::Vertical::Center,
            shaping: Shaping::Advanced,
            max_width: f32::INFINITY,
        });
    }
}

pub fn plain_spans(content: &str) -> Vec<SyntaxHighlightedSpan> {
    vec![SyntaxHighlightedSpan {
        text: content.to_string(),
        style: SyntaxStyle::default(),
    }]
}

pub fn canvas_id_for_side(side: ConflictSide) -> CanvasId {
    match side {
        ConflictSide::Ours => CANVAS_ID_OURS,
        ConflictSide::Theirs => CANVAS_ID_THEIRS,
    }
}

pub fn side_for_canvas_id(id: CanvasId) -> Option<ConflictSide> {
    if id == CANVAS_ID_OURS {
        Some(ConflictSide::Ours)
    } else if id == CANVAS_ID_THEIRS {
        Some(ConflictSide::Theirs)
    } else {
        None
    }
}

pub fn build_side_canvas_data(rows: Vec<ConflictRow>, char_width: f32) -> Arc<TextCanvasData> {
    build_canvas_data(rows, char_width, GUTTER_WIDTH_SIDE)
}

pub fn build_output_canvas_data(rows: Vec<ConflictRow>, char_width: f32) -> Arc<TextCanvasData> {
    build_canvas_data(rows, char_width, GUTTER_WIDTH_OUTPUT)
}

fn build_canvas_data(
    rows: Vec<ConflictRow>,
    char_width: f32,
    gutter_width: f32,
) -> Arc<TextCanvasData> {
    let span = crate::perf::Span::new("cpu.canvas_data_building")
        .field("kind", "conflict")
        .field("rows", rows.len());
    let content_width = compute_content_width(&rows, char_width);
    let rows_dyn: Vec<Arc<dyn CanvasRow>> = rows
        .into_iter()
        .map(|r| Arc::new(r) as Arc<dyn CanvasRow>)
        .collect();
    let data = Arc::new(TextCanvasData::from_rows(
        rows_dyn,
        content_width,
        char_width,
        gutter_width,
    ));
    span.finish_with("content_width", content_width);
    data
}

fn compute_content_width(rows: &[ConflictRow], char_w: f32) -> f32 {
    let max_chars = rows.iter().map(|r| r.char_count).max().unwrap_or(0);
    CONTENT_PAD_X * 2.0 + max_chars as f32 * char_w + 80.0
}

pub fn conflict_content_canvas(
    canvas_id: CanvasId,
    data: Arc<TextCanvasData>,
    selection: Option<TextSelection>,
    viewport: iced::Size,
    bottom_pad: f32,
    scroll_y: f32,
) -> Element<'static, Message> {
    text::content_canvas(
        canvas_id,
        data,
        selection,
        viewport,
        bottom_pad,
        scroll_y,
        diff_panel_callbacks(),
    )
}

pub fn conflict_gutter_canvas(
    canvas_id: CanvasId,
    data: Arc<TextCanvasData>,
    scroll_y: f32,
) -> Element<'static, Message> {
    text::gutter_canvas(canvas_id, data, scroll_y, diff_panel_callbacks())
}

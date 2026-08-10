//! Diff-view rendering on top of the generic `text_canvas`.
//!
//! Defines the `DiffRow` model (hunk headers, file headers, content lines,
//! Eofnl markers) and its gutter rendering (two line-number columns plus the
//! +/- sign). Everything about selection, virtualization, scroll, and hit
//! testing lives in `text_canvas` — this module just populates rows.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, RwLock},
};

use iced::{
    advanced::text::{LineHeight, Shaping},
    alignment,
    widget::canvas::{Frame, Text},
    Color, Element, Pixels, Point, Size,
};

use crate::{
    message::Message,
    screens::repository::{panel_messages::DiffPanelAction, RepositoryMessage},
    services::{HighlightedFile, SegmentKind, SyntaxHighlightedSpan},
    theme,
    widgets::text::{
        self, text_cells, CanvasCallbacks, CanvasId, CanvasRow, TextCanvasData,
        TextSelection, CONTENT_PAD_X, DEFAULT_CONTENT_LINE_HEIGHT,
    },
};

const CONTENT_LINE_HEIGHT: f32 = DEFAULT_CONTENT_LINE_HEIGHT;
pub const HUNK_HEADER_HEIGHT: f32 = 28.0; // content + 8px top spacing
const GUTTER_WIDTH: f32 = 90.0;
pub const LINE_NUM_WIDTH: f32 = 40.0;
pub const SIGN_WIDTH: f32 = 16.0;
const DIFF_HIGHLIGHT_MATERIALIZED_CACHE_CAPACITY: usize = 2_048;

pub fn available_content_width(total_width: f32) -> f32 {
    (total_width - GUTTER_WIDTH).max(0.0)
}

pub fn search_scroll_breadcrumb_offset() -> f32 {
    3.0 * CONTENT_LINE_HEIGHT
}

pub const CANVAS_ID: CanvasId = CanvasId(1);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiffHighlightSide {
    Old,
    New,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiffHighlightReference {
    pub side: DiffHighlightSide,
    pub line_number: u32,
}

pub trait DiffHighlightProvider: std::fmt::Debug + Send + Sync {
    fn spans(&self, reference: DiffHighlightReference) -> Option<Arc<[SyntaxHighlightedSpan]>>;
}

#[derive(Debug)]
pub struct EagerDiffHighlightProvider {
    old: Option<Arc<HighlightedFile>>,
    new: Option<Arc<HighlightedFile>>,
    materialized: Mutex<BoundedDiffHighlightCache>,
}

impl EagerDiffHighlightProvider {
    fn new_with_capacity(
        old: Option<Arc<HighlightedFile>>,
        new: Option<Arc<HighlightedFile>>,
        capacity: usize,
    ) -> Option<Self> {
        if old.is_none() && new.is_none() {
            None
        } else {
            Some(Self {
                old,
                new,
                materialized: Mutex::new(BoundedDiffHighlightCache::new(capacity)),
            })
        }
    }

    fn materialized_spans(
        &self,
        reference: DiffHighlightReference,
    ) -> Option<Arc<[SyntaxHighlightedSpan]>> {
        if let Ok(mut cache) = self.materialized.lock() {
            if let Some(spans) = cache.get(reference) {
                return Some(spans);
            }
        }

        let file = match reference.side {
            DiffHighlightSide::Old => self.old.as_ref(),
            DiffHighlightSide::New => self.new.as_ref(),
        }?;
        let spans = file.line(reference.line_number);
        if spans.is_empty() {
            return None;
        }

        let spans: Arc<[SyntaxHighlightedSpan]> = Arc::from(spans.to_vec().into_boxed_slice());
        if let Ok(mut cache) = self.materialized.lock() {
            cache.insert(reference, spans.clone());
        }
        Some(spans)
    }
}

impl DiffHighlightProvider for EagerDiffHighlightProvider {
    fn spans(&self, reference: DiffHighlightReference) -> Option<Arc<[SyntaxHighlightedSpan]>> {
        self.materialized_spans(reference)
    }
}

#[derive(Debug)]
pub struct CachedDiffHighlightProvider {
    cache: Mutex<BoundedDiffHighlightCache>,
    eager: RwLock<Option<EagerDiffHighlightProvider>>,
    materialized_capacity: usize,
}

impl CachedDiffHighlightProvider {
    pub fn new() -> Self {
        Self::with_capacity(DIFF_HIGHLIGHT_MATERIALIZED_CACHE_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            cache: Mutex::new(BoundedDiffHighlightCache::new(capacity)),
            eager: RwLock::new(None),
            materialized_capacity: capacity,
        }
    }

    pub fn insert(&self, reference: DiffHighlightReference, spans: Arc<[SyntaxHighlightedSpan]>) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(reference, spans);
        }
    }

    pub fn cached_spans(
        &self,
        reference: DiffHighlightReference,
    ) -> Option<Arc<[SyntaxHighlightedSpan]>> {
        self.cache
            .lock()
            .ok()
            .and_then(|mut cache| cache.get(reference))
    }

    pub fn contains(&self, reference: DiffHighlightReference) -> bool {
        self.cached_spans(reference).is_some() || self.spans(reference).is_some()
    }

    pub fn set_eager_files(
        &self,
        old: Option<Arc<HighlightedFile>>,
        new: Option<Arc<HighlightedFile>>,
    ) {
        if let Ok(mut eager) = self.eager.write() {
            *eager =
                EagerDiffHighlightProvider::new_with_capacity(old, new, self.materialized_capacity);
        }
        // The span cache is keyed only by (side, line_number); swapping in new
        // file contents would otherwise serve the previous file's spans (and
        // their text) for the same line numbers.
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    #[cfg(test)]
    pub fn cached_len(&self) -> usize {
        self.cache.lock().map_or(0, |cache| cache.len())
    }
}

impl Default for CachedDiffHighlightProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct CacheEntry {
    spans: Arc<[SyntaxHighlightedSpan]>,
    tick: u64,
}

#[derive(Debug)]
struct BoundedDiffHighlightCache {
    entries: BTreeMap<DiffHighlightReference, CacheEntry>,
    order: BTreeMap<u64, DiffHighlightReference>,
    next_tick: u64,
    capacity: usize,
}

impl BoundedDiffHighlightCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            order: BTreeMap::new(),
            next_tick: 0,
            capacity,
        }
    }

    fn bump_tick(&mut self) -> u64 {
        let tick = self.next_tick;
        self.next_tick += 1;
        tick
    }

    fn get(&mut self, reference: DiffHighlightReference) -> Option<Arc<[SyntaxHighlightedSpan]>> {
        let tick = self.bump_tick();
        let entry = self.entries.get_mut(&reference)?;
        self.order.remove(&entry.tick);
        entry.tick = tick;
        let spans = entry.spans.clone();
        self.order.insert(tick, reference);
        Some(spans)
    }

    fn insert(&mut self, reference: DiffHighlightReference, spans: Arc<[SyntaxHighlightedSpan]>) {
        if self.capacity == 0 {
            return;
        }

        let tick = self.bump_tick();
        if let Some(entry) = self.entries.get_mut(&reference) {
            self.order.remove(&entry.tick);
            entry.spans = spans;
            entry.tick = tick;
            self.order.insert(tick, reference);
            return;
        }

        while self.entries.len() >= self.capacity {
            let Some((&evicted_tick, &evicted)) = self.order.iter().next() else {
                self.entries.clear();
                break;
            };
            self.order.remove(&evicted_tick);
            self.entries.remove(&evicted);
        }

        if self.entries.len() < self.capacity {
            self.order.insert(tick, reference);
            self.entries.insert(reference, CacheEntry { spans, tick });
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

impl DiffHighlightProvider for CachedDiffHighlightProvider {
    fn spans(&self, reference: DiffHighlightReference) -> Option<Arc<[SyntaxHighlightedSpan]>> {
        if let Some(spans) = self.cached_spans(reference) {
            return Some(spans);
        }

        self.eager.read().ok().and_then(|eager| {
            eager
                .as_ref()
                .and_then(|provider| provider.spans(reference))
        })
    }
}

#[derive(Debug, Clone)]
pub enum DiffRowText {
    Plain(String),
    Highlighted {
        source: DiffHighlightReference,
        alternate_source: Option<DiffHighlightReference>,
        fallback: String,
        provider: Option<Arc<dyn DiffHighlightProvider>>,
    },
}

impl DiffRowText {
    pub fn fallback(&self) -> &str {
        match self {
            DiffRowText::Plain(text) => text,
            DiffRowText::Highlighted { fallback, .. } => fallback,
        }
    }

    fn resolved_spans(&self) -> Option<Arc<[SyntaxHighlightedSpan]>> {
        let DiffRowText::Highlighted {
            source,
            alternate_source,
            provider: Some(provider),
            ..
        } = self
        else {
            return None;
        };

        provider
            .spans(*source)
            .or_else(|| alternate_source.and_then(|reference| provider.spans(reference)))
    }
}

#[derive(Debug, Clone)]
pub enum DiffRow {
    HunkHeader(String),
    FileHeader(String),
    Content {
        old_lineno: Option<u32>,
        new_lineno: Option<u32>,
        kind: LineKind,
        text: DiffRowText,
        segment_bgs: Vec<SegmentBg>,
        char_count: usize,
    },
    Eofnl(LineKind),
}

impl CanvasRow for DiffRow {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

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
            DiffRow::Content { text, .. } => text.fallback().to_string(),
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
                    // The content canvas clips to its own bounds, so the text
                    // must start at a non-negative x or the leading `@@ -` of
                    // every hunk header gets sliced off.
                    CONTENT_PAD_X,
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
                text,
                segment_bgs,
                ..
            } => {
                let line_bg = line_bg_color(*kind);
                frame.fill_rectangle(Point::new(0.0, top_y), Size::new(width, h), line_bg);
                let line_text = text.fallback();
                // Map char columns to display cells for wide-glyph alignment.
                // ASCII lines (the overwhelmingly common case) need no mapping
                // — cell == char index — so we skip building anything. For
                // non-ASCII lines build the prefix ONCE here rather than
                // rescanning from the start per segment, which was quadratic on
                // the hot draw path.
                let cell_prefix = (!line_text.is_ascii()).then(|| cell_offset_prefix(line_text));
                let col_to_cells = |col: usize| -> usize {
                    match &cell_prefix {
                        None => col,
                        Some(prefix) => prefix
                            .get(col)
                            .copied()
                            .unwrap_or_else(|| prefix.last().copied().unwrap_or(0)),
                    }
                };
                for seg in segment_bgs {
                    let bg = segment_bg_color(seg.kind, *kind);
                    if bg == line_bg {
                        continue;
                    }
                    let sx = CONTENT_PAD_X + col_to_cells(seg.start_col) as f32 * char_width;
                    let ex = CONTENT_PAD_X + col_to_cells(seg.end_col) as f32 * char_width;
                    frame.fill_rectangle(
                        Point::new(sx, top_y),
                        Size::new((ex - sx).max(0.0), h),
                        bg,
                    );
                }
                let baseline_y = top_y + h / 2.0;
                if let Some(spans) = text.resolved_spans() {
                    draw_highlighted_spans(frame, &spans, baseline_y, h, char_width);
                } else {
                    draw_single_span(
                        frame,
                        CONTENT_PAD_X,
                        baseline_y,
                        text.fallback(),
                        theme::TEXT_PRIMARY,
                    );
                }
            }
            DiffRow::Eofnl(kind) => {
                let line_bg = line_bg_color(*kind);
                frame.fill_rectangle(Point::new(0.0, top_y), Size::new(width, h), line_bg);
            }
        }
    }
}

fn draw_highlighted_spans(
    frame: &mut Frame,
    spans: &[SyntaxHighlightedSpan],
    baseline_y: f32,
    line_height: f32,
    char_width: f32,
) {
    let mut x = CONTENT_PAD_X;
    for span in spans {
        if span.text.is_empty() {
            continue;
        }
        let color = span.style.color.unwrap_or(theme::TEXT_PRIMARY);
        frame.fill_text(Text {
            content: span.text.clone(),
            position: Point::new(x, baseline_y),
            color,
            size: Pixels(theme::FONT_DIFF),
            line_height: LineHeight::Absolute(Pixels(line_height)),
            font: theme::MONO,
            align_x: iced::advanced::text::Alignment::Left,
            align_y: alignment::Vertical::Center,
            shaping: Shaping::Advanced,
            max_width: f32::INFINITY,
        });
        // Advance by display cells so glyphs after a wide (CJK/emoji) char in
        // the span aren't overlapped (`text_cells` fast-paths ASCII).
        x += text_cells(&span.text) as f32 * char_width;
    }
}

/// Cumulative display-cell offset for each character boundary of `text`:
/// entry `i` is the number of cells occupied by the first `i` characters, so
/// `prefix[col]` is the cell offset of column `col`. Length is
/// `char_count + 1`.
fn cell_offset_prefix(text: &str) -> Vec<usize> {
    let mut prefix = Vec::with_capacity(text.len() + 1);
    let mut acc = 0usize;
    prefix.push(0);
    for ch in text.chars() {
        acc += crate::widgets::text::char_cells(ch);
        prefix.push(acc);
    }
    prefix
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
    let span = crate::perf::Span::new("cpu.canvas_data_building")
        .field("kind", "diff")
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
        GUTTER_WIDTH,
    ));
    span.finish_with("content_width", content_width);
    data
}

fn compute_content_width(rows: &[DiffRow], char_w: f32) -> f32 {
    let mut max_cells = 0usize;
    for r in rows {
        if let DiffRow::Content { text, .. } = r {
            // Use display cells so lines with wide (CJK/emoji) glyphs get a
            // horizontal scroll extent that actually reaches their end.
            max_cells = max_cells.max(text_cells(text.fallback()));
        }
    }
    CONTENT_PAD_X * 2.0 + max_cells as f32 * char_w + 40.0
}

/// Callbacks wired into the repository message enum. Same set is shared
/// between all `DiffPanel`-hosted canvases (diff view + conflict buffers);
/// `canvas_id` disambiguates them at the message layer.
pub fn diff_panel_callbacks() -> CanvasCallbacks<Message> {
    static CALLBACKS: std::sync::LazyLock<CanvasCallbacks<Message>> =
        std::sync::LazyLock::new(|| CanvasCallbacks {
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
        });
    CALLBACKS.clone()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{highlight_document, HighlightDocument, SyntaxStyle};

    fn reference(side: DiffHighlightSide, line_number: u32) -> DiffHighlightReference {
        DiffHighlightReference { side, line_number }
    }

    fn span(text: &str) -> SyntaxHighlightedSpan {
        SyntaxHighlightedSpan {
            text: text.to_string(),
            style: SyntaxStyle::default(),
        }
    }

    fn span_text(spans: &[SyntaxHighlightedSpan]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn cached_provider_returns_inserted_line_spans() {
        let provider = CachedDiffHighlightProvider::new();
        let reference = reference(DiffHighlightSide::New, 2);
        provider.insert(
            reference,
            Arc::from(vec![span("cached")].into_boxed_slice()),
        );

        let spans = provider.spans(reference).expect("cached spans");

        assert_eq!(span_text(&spans), "cached");
        assert_eq!(provider.cached_len(), 1);
    }

    #[test]
    fn cached_provider_prefers_lazy_cache_over_eager_file() {
        let provider = CachedDiffHighlightProvider::new();
        let reference = reference(DiffHighlightSide::New, 1);
        let document = HighlightDocument::new("eager", "txt");
        provider.set_eager_files(None, Some(highlight_document(&document)));
        provider.insert(
            reference,
            Arc::from(vec![span("cached")].into_boxed_slice()),
        );

        let spans = provider.spans(reference).expect("cached spans");

        assert_eq!(span_text(&spans), "cached");
    }

    #[test]
    fn cached_provider_evicts_least_recent_materialized_line() {
        let provider = CachedDiffHighlightProvider::with_capacity(2);
        let first = reference(DiffHighlightSide::New, 1);
        let second = reference(DiffHighlightSide::New, 2);
        let third = reference(DiffHighlightSide::New, 3);
        provider.insert(first, Arc::from(vec![span("first")].into_boxed_slice()));
        provider.insert(second, Arc::from(vec![span("second")].into_boxed_slice()));
        assert!(provider.cached_spans(first).is_some());

        provider.insert(third, Arc::from(vec![span("third")].into_boxed_slice()));

        assert!(provider.cached_spans(first).is_some());
        assert!(provider.cached_spans(second).is_none());
        assert!(provider.cached_spans(third).is_some());
        assert_eq!(provider.cached_len(), 2);
    }

    #[test]
    fn diff_row_text_resolves_alternate_provider_reference() {
        let provider = Arc::new(CachedDiffHighlightProvider::new());
        let preferred = reference(DiffHighlightSide::New, 10);
        let alternate = reference(DiffHighlightSide::Old, 9);
        provider.insert(
            alternate,
            Arc::from(vec![span("alternate")].into_boxed_slice()),
        );
        let text = DiffRowText::Highlighted {
            source: preferred,
            alternate_source: Some(alternate),
            fallback: "fallback".to_string(),
            provider: Some(provider),
        };

        let spans = text.resolved_spans().expect("alternate spans");

        assert_eq!(span_text(&spans), "alternate");
    }
}

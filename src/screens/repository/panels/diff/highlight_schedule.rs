use std::{cmp::Ordering, collections::BTreeSet, ops::Range, sync::Arc};

use crate::{
    services::{
        syntax_highlight::{HighlightLineResult, SyntaxGrammarStatus},
        HighlightDocument,
    },
    widgets::{
        diff_canvas::{
            CachedDiffHighlightProvider, DiffCanvasData, DiffHighlightReference, DiffHighlightSide,
            DiffRow, DiffRowText,
        },
        text::{VisibleRowRange, DEFAULT_CONTENT_LINE_HEIGHT},
    },
};

pub(in crate::screens::repository) const HIGHLIGHT_OVERSCAN_ROWS: usize = 4;
pub(in crate::screens::repository) const HIGHLIGHT_REQUEST_BUDGET: usize = 24;
pub(in crate::screens::repository) const INITIAL_HIGHLIGHT_VIEWPORT_HEIGHT: f32 =
    DEFAULT_CONTENT_LINE_HEIGHT * 36.0;
const MAX_HIGHLIGHT_OVERSCAN_ROWS: usize = 12;
const MIN_HIGHLIGHT_REQUEST_BUDGET: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::screens::repository) struct DiffLineHighlightRequest {
    pub generation: u64,
    pub document_id: Option<u64>,
    pub reference: DiffHighlightReference,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::screens::repository) struct DiffHighlightDocumentIds {
    old: Option<u64>,
    new: Option<u64>,
}

impl DiffHighlightDocumentIds {
    pub fn from_documents(
        old_document: Option<&HighlightDocument>,
        new_document: Option<&HighlightDocument>,
    ) -> Self {
        Self {
            old: old_document.map(HighlightDocument::highlight_generation_id),
            new: new_document.map(HighlightDocument::highlight_generation_id),
        }
    }

    fn id_for(self, side: DiffHighlightSide) -> Option<u64> {
        match side {
            DiffHighlightSide::Old => self.old,
            DiffHighlightSide::New => self.new,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::screens::repository) struct DiffHighlightScheduleStats {
    pub visible_rows: usize,
    pub candidate_rows: usize,
    pub overscan_rows: usize,
    pub request_budget: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::screens::repository) struct DiffHighlightMaterializeStats {
    pub requested: usize,
    pub highlighted: usize,
    pub provider_hits: usize,
    pub provider_misses: usize,
    pub stale_generation: usize,
    pub stale_documents: usize,
    pub missing_documents: usize,
    pub missing_grammars: usize,
    pub missing_lines: usize,
    pub syntax_cache_hits: usize,
    pub syntax_cache_misses: usize,
    pub tree_parse_hits: usize,
    pub tree_parse_misses: usize,
    pub parsed_lines: usize,
}

#[derive(Debug, Default)]
pub(in crate::screens::repository) struct DiffHighlightMaterializeResult {
    pub stats: DiffHighlightMaterializeStats,
}

impl std::ops::Deref for DiffHighlightMaterializeResult {
    type Target = DiffHighlightMaterializeStats;

    fn deref(&self) -> &Self::Target {
        &self.stats
    }
}

#[derive(Debug, Default)]
pub(in crate::screens::repository) struct DiffHighlightScheduler {
    generation: Option<u64>,
    requested: BTreeSet<DiffHighlightReference>,
    last_batch: Vec<DiffLineHighlightRequest>,
    last_scroll_y: Option<f32>,
    last_stats: DiffHighlightScheduleStats,
}

impl DiffHighlightScheduler {
    pub fn reset(&mut self) {
        self.generation = None;
        self.requested.clear();
        self.last_batch.clear();
        self.last_scroll_y = None;
        self.last_stats = DiffHighlightScheduleStats::default();
    }

    pub fn schedule(
        &mut self,
        generation: u64,
        document_ids: DiffHighlightDocumentIds,
        data: &DiffCanvasData,
        scroll_y: f32,
        viewport_height: f32,
    ) -> &[DiffLineHighlightRequest] {
        if self.generation != Some(generation) {
            self.generation = Some(generation);
            self.requested.clear();
            self.last_batch.clear();
            self.last_scroll_y = None;
        }

        let overscan_rows = highlight_overscan_rows(viewport_height);
        let rows = data.visible_row_range(scroll_y, viewport_height, overscan_rows);
        let request_budget = highlight_request_budget(rows.visible.len());
        let direction = scroll_direction(self.last_scroll_y, scroll_y);
        self.last_scroll_y = Some(scroll_y);
        self.last_stats = DiffHighlightScheduleStats {
            visible_rows: rows.visible.len(),
            candidate_rows: rows.with_overscan.len(),
            overscan_rows,
            request_budget,
        };
        self.last_batch = collect_visible_highlight_requests(
            data,
            &rows,
            &mut self.requested,
            request_budget,
            direction,
        )
        .into_iter()
        .map(|reference| DiffLineHighlightRequest {
            generation,
            document_id: document_ids.id_for(reference.side),
            reference,
        })
        .collect();
        &self.last_batch
    }

    pub fn last_stats(&self) -> DiffHighlightScheduleStats {
        self.last_stats
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollDirection {
    Up,
    Down,
    Stationary,
}

fn scroll_direction(previous: Option<f32>, current: f32) -> ScrollDirection {
    match previous.and_then(|previous| current.partial_cmp(&previous)) {
        Some(Ordering::Greater) => ScrollDirection::Down,
        Some(Ordering::Less) => ScrollDirection::Up,
        _ => ScrollDirection::Stationary,
    }
}

fn highlight_overscan_rows(viewport_height: f32) -> usize {
    if viewport_height <= 0.0 {
        return HIGHLIGHT_OVERSCAN_ROWS;
    }
    let viewport_rows = (viewport_height / DEFAULT_CONTENT_LINE_HEIGHT).ceil() as usize;
    (viewport_rows / 3).clamp(HIGHLIGHT_OVERSCAN_ROWS, MAX_HIGHLIGHT_OVERSCAN_ROWS)
}

fn highlight_request_budget(visible_rows: usize) -> usize {
    visible_rows.clamp(MIN_HIGHLIGHT_REQUEST_BUDGET, HIGHLIGHT_REQUEST_BUDGET)
}

fn collect_visible_highlight_requests(
    data: &DiffCanvasData,
    rows: &VisibleRowRange,
    requested: &mut BTreeSet<DiffHighlightReference>,
    budget: usize,
    direction: ScrollDirection,
) -> Vec<DiffHighlightReference> {
    let mut out = Vec::new();
    collect_range(data, rows.visible.clone(), requested, budget, &mut out);
    let before = rows.with_overscan.start..rows.visible.start;
    let after = rows.visible.end..rows.with_overscan.end;
    match direction {
        ScrollDirection::Down => {
            collect_range(data, after, requested, budget, &mut out);
            collect_range(data, before, requested, budget, &mut out);
        }
        _ => {
            collect_range(data, before, requested, budget, &mut out);
            collect_range(data, after, requested, budget, &mut out);
        }
    }
    out
}

fn collect_range(
    data: &DiffCanvasData,
    rows: Range<usize>,
    requested: &mut BTreeSet<DiffHighlightReference>,
    budget: usize,
    out: &mut Vec<DiffHighlightReference>,
) {
    if out.len() >= budget {
        return;
    }

    let row_count = data.rows().len();
    let start = rows.start.min(row_count);
    let end = rows.end.min(row_count).max(start);
    for row in &data.rows()[start..end] {
        let Some(diff_row) = row.as_any().downcast_ref::<DiffRow>() else {
            continue;
        };
        push_row_references(diff_row, requested, budget, out);
        if out.len() >= budget {
            return;
        }
    }
}

fn push_row_references(
    row: &DiffRow,
    requested: &mut BTreeSet<DiffHighlightReference>,
    budget: usize,
    out: &mut Vec<DiffHighlightReference>,
) {
    let DiffRow::Content { text, .. } = row else {
        return;
    };
    let DiffRowText::Highlighted {
        source,
        alternate_source,
        ..
    } = text
    else {
        return;
    };

    push_reference(*source, requested, budget, out);
    if let Some(alternate) = alternate_source {
        push_reference(*alternate, requested, budget, out);
    }
}

fn push_reference(
    reference: DiffHighlightReference,
    requested: &mut BTreeSet<DiffHighlightReference>,
    budget: usize,
    out: &mut Vec<DiffHighlightReference>,
) {
    if out.len() < budget && requested.insert(reference) {
        out.push(reference);
    }
}

pub(in crate::screens::repository) fn highlight_scheduled_requests_with_stats(
    requests: &[DiffLineHighlightRequest],
    generation: u64,
    document_ids: DiffHighlightDocumentIds,
    old_document: Option<&HighlightDocument>,
    new_document: Option<&HighlightDocument>,
    provider: &Arc<CachedDiffHighlightProvider>,
    budget: usize,
) -> DiffHighlightMaterializeResult {
    let mut stats = DiffHighlightMaterializeStats {
        requested: requests.len(),
        ..DiffHighlightMaterializeStats::default()
    };
    if budget == 0 {
        return DiffHighlightMaterializeResult { stats };
    }

    let service = crate::services::syntax_highlight::get_syntax_service();
    for request in requests {
        if stats.highlighted >= budget {
            break;
        }
        if request.generation != generation {
            stats.stale_generation += 1;
            continue;
        }
        if request.document_id != document_ids.id_for(request.reference.side) {
            stats.stale_documents += 1;
            continue;
        }
        if provider.contains(request.reference) {
            stats.provider_hits += 1;
            continue;
        }
        stats.provider_misses += 1;
        let document = match request.reference.side {
            DiffHighlightSide::Old => old_document,
            DiffHighlightSide::New => new_document,
        };
        let Some(document) = document else {
            stats.missing_documents += 1;
            continue;
        };
        if matches!(
            service.syntax_status_for_document(document),
            SyntaxGrammarStatus::Missing { .. }
        ) {
            stats.missing_grammars += 1;
        }
        let before = document.highlight_stats();
        let line =
            match service.highlight_line_for_document(document, request.reference.line_number) {
                HighlightLineResult::Ready(line) => line,
                HighlightLineResult::Missing => {
                    stats.missing_lines += 1;
                    continue;
                }
            };
        let after = document.highlight_stats();
        stats.syntax_cache_hits += after.cache_hits.saturating_sub(before.cache_hits);
        stats.syntax_cache_misses += after.cache_misses.saturating_sub(before.cache_misses);
        stats.tree_parse_hits += after.parse_hits.saturating_sub(before.parse_hits);
        stats.tree_parse_misses += after.parse_misses.saturating_sub(before.parse_misses);
        stats.parsed_lines += after.parsed_lines.saturating_sub(before.parsed_lines);
        if line.line_number != request.reference.line_number {
            stats.missing_lines += 1;
            continue;
        }
        provider.insert(request.reference, line.into_spans());
        stats.highlighted += 1;
    }
    DiffHighlightMaterializeResult { stats }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::diff_canvas::{self, DiffHighlightSide, LineKind, SegmentBg};

    fn content_row(
        source: DiffHighlightReference,
        alternate_source: Option<DiffHighlightReference>,
    ) -> DiffRow {
        DiffRow::Content {
            old_lineno: None,
            new_lineno: None,
            kind: LineKind::Context,
            text: DiffRowText::Highlighted {
                source,
                alternate_source,
                fallback: "line".to_string(),
                provider: None,
            },
            segment_bgs: Vec::<SegmentBg>::new(),
            char_count: 4,
        }
    }

    fn header_row() -> DiffRow {
        DiffRow::FileHeader("header".to_string())
    }

    fn reference(side: DiffHighlightSide, line_number: u32) -> DiffHighlightReference {
        DiffHighlightReference { side, line_number }
    }

    fn document_ids(
        old_document: Option<&HighlightDocument>,
        new_document: Option<&HighlightDocument>,
    ) -> DiffHighlightDocumentIds {
        DiffHighlightDocumentIds::from_documents(old_document, new_document)
    }

    #[test]
    fn scheduler_deduplicates_visible_and_overscan_requests() {
        let old_1 = reference(DiffHighlightSide::Old, 1);
        let new_1 = reference(DiffHighlightSide::New, 1);
        let new_2 = reference(DiffHighlightSide::New, 2);
        let data = diff_canvas::build_canvas_data(
            vec![
                content_row(old_1, None),
                content_row(new_1, Some(old_1)),
                content_row(new_1, None),
                content_row(new_2, None),
            ],
            8.0,
        );
        let rows = VisibleRowRange {
            visible: 1..3,
            with_overscan: 0..4,
        };
        let mut requested = BTreeSet::new();

        let requests = collect_visible_highlight_requests(
            &data,
            &rows,
            &mut requested,
            10,
            ScrollDirection::Stationary,
        );

        assert_eq!(requests, vec![new_1, old_1, new_2]);
    }

    #[test]
    fn scheduler_prioritizes_visible_rows_before_overscan() {
        let old_1 = reference(DiffHighlightSide::Old, 1);
        let old_2 = reference(DiffHighlightSide::Old, 2);
        let new_10 = reference(DiffHighlightSide::New, 10);
        let new_11 = reference(DiffHighlightSide::New, 11);
        let data = diff_canvas::build_canvas_data(
            vec![
                content_row(old_1, None),
                content_row(new_10, None),
                content_row(new_11, None),
                content_row(old_2, None),
            ],
            8.0,
        );
        let rows = VisibleRowRange {
            visible: 1..3,
            with_overscan: 0..4,
        };
        let mut requested = BTreeSet::new();

        let requests = collect_visible_highlight_requests(
            &data,
            &rows,
            &mut requested,
            3,
            ScrollDirection::Stationary,
        );

        assert_eq!(requests, vec![new_10, new_11, old_1]);
    }

    #[test]
    fn scheduler_resets_dedupe_when_generation_changes() {
        let new_1 = reference(DiffHighlightSide::New, 1);
        let data =
            diff_canvas::build_canvas_data(vec![header_row(), content_row(new_1, None)], 8.0);
        let mut scheduler = DiffHighlightScheduler::default();

        let first = scheduler
            .schedule(7, DiffHighlightDocumentIds::default(), &data, 20.0, 20.0)
            .to_vec();
        let repeated = scheduler
            .schedule(7, DiffHighlightDocumentIds::default(), &data, 20.0, 20.0)
            .to_vec();
        let next_generation = scheduler
            .schedule(8, DiffHighlightDocumentIds::default(), &data, 20.0, 20.0)
            .to_vec();

        assert_eq!(first.len(), 1);
        assert!(repeated.is_empty());
        assert_eq!(next_generation.len(), 1);
        assert_eq!(next_generation[0].generation, 8);
    }

    #[test]
    fn scheduler_prefetches_scroll_direction_after_visible_rows() {
        let old_1 = reference(DiffHighlightSide::Old, 1);
        let old_2 = reference(DiffHighlightSide::Old, 2);
        let new_10 = reference(DiffHighlightSide::New, 10);
        let new_11 = reference(DiffHighlightSide::New, 11);
        let data = diff_canvas::build_canvas_data(
            vec![
                content_row(old_1, None),
                content_row(new_10, None),
                content_row(new_11, None),
                content_row(old_2, None),
            ],
            8.0,
        );
        let rows = VisibleRowRange {
            visible: 1..3,
            with_overscan: 0..4,
        };
        let mut requested = BTreeSet::new();

        let requests = collect_visible_highlight_requests(
            &data,
            &rows,
            &mut requested,
            3,
            ScrollDirection::Down,
        );

        assert_eq!(requests, vec![new_10, new_11, old_2]);
    }

    #[test]
    fn generation_reset_reissues_visible_request_after_stale_batch() {
        let new_1 = reference(DiffHighlightSide::New, 1);
        let data = diff_canvas::build_canvas_data(vec![content_row(new_1, None)], 8.0);
        let document = HighlightDocument::new("one\n", "txt");
        let provider = Arc::new(CachedDiffHighlightProvider::new());
        let mut scheduler = DiffHighlightScheduler::default();

        let stale = scheduler
            .schedule(
                20,
                document_ids(None, Some(&document)),
                &data,
                0.0,
                DEFAULT_CONTENT_LINE_HEIGHT,
            )
            .to_vec();
        let ignored = highlight_scheduled_requests_with_stats(
            &stale,
            21,
            document_ids(None, Some(&document)),
            None,
            Some(&document),
            &provider,
            1,
        )
        .highlighted;
        let fresh = scheduler
            .schedule(
                21,
                document_ids(None, Some(&document)),
                &data,
                0.0,
                DEFAULT_CONTENT_LINE_HEIGHT,
            )
            .to_vec();
        let highlighted = highlight_scheduled_requests_with_stats(
            &fresh,
            21,
            document_ids(None, Some(&document)),
            None,
            Some(&document),
            &provider,
            1,
        )
        .highlighted;

        assert_eq!(ignored, 0);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].generation, 21);
        assert_eq!(highlighted, 1);
        assert!(provider.cached_spans(new_1).is_some());
    }

    #[test]
    fn stale_document_id_drops_file_change_results() {
        let target = reference(DiffHighlightSide::New, 1);
        let data = diff_canvas::build_canvas_data(vec![content_row(target, None)], 8.0);
        let old_document = HighlightDocument::from_path("old\n", "old.rs");
        let new_document = HighlightDocument::from_path("new\n", "new.rs");
        let old_ids = document_ids(None, Some(&old_document));
        let new_ids = document_ids(None, Some(&new_document));
        let provider = Arc::new(CachedDiffHighlightProvider::new());
        let mut scheduler = DiffHighlightScheduler::default();

        let stale = scheduler
            .schedule(42, old_ids, &data, 0.0, DEFAULT_CONTENT_LINE_HEIGHT)
            .to_vec();
        let stale_result = highlight_scheduled_requests_with_stats(
            &stale,
            42,
            new_ids,
            None,
            Some(&new_document),
            &provider,
            1,
        );
        scheduler.reset();
        let fresh = scheduler
            .schedule(42, new_ids, &data, 0.0, DEFAULT_CONTENT_LINE_HEIGHT)
            .to_vec();
        let fresh_result = highlight_scheduled_requests_with_stats(
            &fresh,
            42,
            new_ids,
            None,
            Some(&new_document),
            &provider,
            1,
        );

        assert_eq!(stale_result.highlighted, 0);
        assert_eq!(stale_result.stale_documents, 1);
        assert_eq!(fresh_result.highlighted, 1);
        assert!(provider.cached_spans(target).is_some());
    }

    #[test]
    fn highlighting_scheduled_requests_respects_budget_and_dedupes_cache() {
        let new_1 = reference(DiffHighlightSide::New, 1);
        let new_2 = reference(DiffHighlightSide::New, 2);
        let new_3 = reference(DiffHighlightSide::New, 3);
        let data = diff_canvas::build_canvas_data(
            vec![
                content_row(new_1, None),
                content_row(new_1, None),
                content_row(new_2, None),
                content_row(new_3, None),
            ],
            8.0,
        );
        let document = HighlightDocument::new("one\ntwo\nthree\n", "txt");
        let provider = Arc::new(CachedDiffHighlightProvider::new());
        let mut scheduler = DiffHighlightScheduler::default();

        let requests = scheduler
            .schedule(
                11,
                document_ids(None, Some(&document)),
                &data,
                0.0,
                DEFAULT_CONTENT_LINE_HEIGHT * 4.0,
            )
            .to_vec();
        let highlighted = highlight_scheduled_requests_with_stats(
            &requests,
            11,
            document_ids(None, Some(&document)),
            None,
            Some(&document),
            &provider,
            2,
        )
        .highlighted;

        assert_eq!(
            requests
                .iter()
                .map(|request| request.reference)
                .collect::<Vec<_>>(),
            vec![new_1, new_2, new_3]
        );
        assert_eq!(highlighted, 2);
        assert!(provider.cached_spans(new_1).is_some());
        assert!(provider.cached_spans(new_2).is_some());
        assert!(provider.cached_spans(new_3).is_none());
        assert_eq!(provider.cached_len(), 2);
    }

    #[test]
    fn highlighting_scheduled_requests_ignores_stale_generations() {
        let new_1 = reference(DiffHighlightSide::New, 1);
        let document = HighlightDocument::new("one\n", "txt");
        let provider = Arc::new(CachedDiffHighlightProvider::new());
        let requests = vec![DiffLineHighlightRequest {
            generation: 10,
            document_id: Some(document.highlight_generation_id()),
            reference: new_1,
        }];

        let highlighted = highlight_scheduled_requests_with_stats(
            &requests,
            11,
            document_ids(None, Some(&document)),
            None,
            Some(&document),
            &provider,
            10,
        )
        .highlighted;

        assert_eq!(highlighted, 0);
        assert!(provider.cached_spans(new_1).is_none());
    }

    #[test]
    fn unsupported_syntax_materializes_plain_spans_without_parse() {
        let target = reference(DiffHighlightSide::New, 1);
        let data = diff_canvas::build_canvas_data(vec![content_row(target, None)], 8.0);
        let document = HighlightDocument::new("plain marker line\n", "txt");
        let ids = document_ids(None, Some(&document));
        let provider = Arc::new(CachedDiffHighlightProvider::new());
        let mut scheduler = DiffHighlightScheduler::default();

        let requests = scheduler
            .schedule(70, ids, &data, 0.0, DEFAULT_CONTENT_LINE_HEIGHT)
            .to_vec();
        let result = highlight_scheduled_requests_with_stats(
            &requests,
            70,
            ids,
            None,
            Some(&document),
            &provider,
            1,
        );
        let spans = provider.cached_spans(target).unwrap();
        let text = spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();

        assert_eq!(result.highlighted, 1);
        assert_eq!(text, "plain marker line");
        assert_eq!(document.highlight_stats().parsed_trees, 0);
    }
}

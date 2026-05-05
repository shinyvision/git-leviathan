//! Data-transform layer. Converts service-level diff/conflict types into
//! canvas row types consumed by the diff + conflict canvas widgets. No
//! widget-tree construction here — pure function shapes.

use crate::{
    services::{
        ConflictBlock, ConflictResolutionResult, DiffContentSkipReason, DiffFallbacks,
        DiffLineType, DiffSide, HighlightedFile, SegmentKind, SyntaxHighlightedSpan, SyntaxStyle,
        WorkingTreeDiffLine,
    },
    widgets::{
        conflict_canvas::{
            self, ConflictRow, GutterKind, CANVAS_ID_OURS, CANVAS_ID_OUTPUT, CANVAS_ID_THEIRS,
        },
        diff_canvas::{
            diff_char_width, DiffCanvasId, DiffRow, LineKind as CanvasLineKind, SegmentBg,
        },
        text::TextCanvasData,
    },
};

use std::sync::Arc;

use super::super::conflict::{ConflictHunkSelection, ConflictSide};

/// Translate the diff model into canvas rows. Hunk / file headers and Eofnl
/// rows are marked non-selectable so a selection crossing them skips them on
/// copy. Segment bgs carry char-index ranges so the canvas can paint
/// intra-line highlights without building a per-segment widget tree.
pub(in crate::screens::repository) fn build_diff_rows_public(
    lines: &[WorkingTreeDiffLine],
    old_hl: Option<&HighlightedFile>,
    new_hl: Option<&HighlightedFile>,
    fallbacks: &DiffFallbacks,
) -> Vec<DiffRow> {
    build_diff_rows(lines, old_hl, new_hl, fallbacks)
}

pub(super) fn build_diff_rows(
    lines: &[WorkingTreeDiffLine],
    old_hl: Option<&HighlightedFile>,
    new_hl: Option<&HighlightedFile>,
    fallbacks: &DiffFallbacks,
) -> Vec<DiffRow> {
    let span = crate::perf::Span::new("cpu.diff_row_building").field("input_lines", lines.len());
    let notices = fallback_notices(fallbacks);
    let mut rows: Vec<DiffRow> = Vec::with_capacity(lines.len() + notices.len());
    for notice in notices {
        rows.push(DiffRow::FileHeader(notice));
    }
    for line in lines {
        match line.line_type {
            DiffLineType::HunkHeader => {
                rows.push(DiffRow::HunkHeader(line.content.clone()));
            }
            DiffLineType::FileHeader => {
                rows.push(DiffRow::FileHeader(line.content.clone()));
            }
            DiffLineType::Context | DiffLineType::Addition | DiffLineType::Deletion => {
                let kind = match line.line_type {
                    DiffLineType::Addition => CanvasLineKind::Addition,
                    DiffLineType::Deletion => CanvasLineKind::Deletion,
                    _ => CanvasLineKind::Context,
                };
                let spans =
                    highlighted_spans_for_diff_line(line, old_hl, new_hl).unwrap_or_else(|| {
                        vec![SyntaxHighlightedSpan {
                            text: line.content.clone(),
                            style: SyntaxStyle::default(),
                        }]
                    });
                let char_count: usize = spans.iter().map(|s| s.text.chars().count()).sum();
                let segment_bgs = build_segment_bgs(&line.segments);
                rows.push(DiffRow::Content {
                    old_lineno: line.old_lineno,
                    new_lineno: line.new_lineno,
                    kind,
                    spans,
                    segment_bgs,
                    char_count,
                });
            }
            DiffLineType::AddEofnl => rows.push(DiffRow::Eofnl(CanvasLineKind::Addition)),
            DiffLineType::DeleteEofnl => rows.push(DiffRow::Eofnl(CanvasLineKind::Deletion)),
            DiffLineType::ContextEofnl => rows.push(DiffRow::Eofnl(CanvasLineKind::Context)),
        }
    }
    span.finish_with("output_rows", rows.len());
    rows
}

fn fallback_notices(fallbacks: &DiffFallbacks) -> Vec<String> {
    let mut notices = Vec::new();
    if let Some(truncation) = fallbacks.truncation {
        notices.push(format!(
            "Large diff truncated: showing {} lines, {} lines omitted.",
            truncation.shown_lines, truncation.omitted_lines
        ));
    }
    if !fallbacks.highlight_skips.is_empty() {
        notices.push(format!(
            "Syntax highlighting skipped: {}.",
            highlight_skip_summary(fallbacks)
        ));
    }
    if fallbacks.has_binary_or_non_utf8() {
        notices
            .push("Binary or non-UTF-8 content shown without full-file highlighting.".to_string());
    }
    if let Some(skip) = fallbacks.intra_line_skip {
        notices.push(format!(
            "Intra-line highlighting skipped for {} changed line pairs over caps ({} chars per line, {} pairs per file).",
            skip.skipped_pairs, skip.max_line_chars, skip.max_pairs
        ));
    }
    notices
}

fn highlight_skip_summary(fallbacks: &DiffFallbacks) -> String {
    fallbacks
        .highlight_skips
        .iter()
        .map(|skip| {
            let side = match skip.side {
                DiffSide::Old => "old file",
                DiffSide::New => "new file",
            };
            let reason = match skip.reason {
                DiffContentSkipReason::Binary => "binary content".to_string(),
                DiffContentSkipReason::NonUtf8 => "non-UTF-8 content".to_string(),
                DiffContentSkipReason::TooManyBytes { bytes, max } => {
                    format!("{bytes} bytes exceeds {max}")
                }
                DiffContentSkipReason::TooManyLines { lines, max } => {
                    format!("{lines} lines exceeds {max}")
                }
            };
            format!("{side} {reason}")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub(super) fn build_conflict_side_rows(
    result: &ConflictResolutionResult,
    selections: &[ConflictHunkSelection],
    side: ConflictSide,
    highlighted: Option<&HighlightedFile>,
) -> Vec<ConflictRow> {
    let mut rows = Vec::new();
    let mut line_number: u32 = 1;

    for block in &result.blocks {
        match block {
            ConflictBlock::Context(lines) => {
                for line in lines {
                    let spans = resolve_spans(highlighted, Some(line_number), line);
                    let char_count = row_char_count(&spans);
                    rows.push(ConflictRow {
                        line_number: Some(line_number),
                        spans,
                        char_count,
                        hunk_idx: None,
                        hunk_selected: false,
                        show_checkbox: false,
                        is_placeholder: false,
                        gutter_kind: GutterKind::Side(side),
                    });
                    line_number += 1;
                }
            }
            ConflictBlock::Conflict(hunk) => {
                let source_lines = match side {
                    ConflictSide::Ours => &hunk.ours_lines,
                    ConflictSide::Theirs => &hunk.theirs_lines,
                };
                let is_selected = selections
                    .get(hunk.index)
                    .is_some_and(|selection| selection.has(side));
                let checkbox_line_idx = source_lines.len().saturating_sub(1) / 2;

                if source_lines.is_empty() {
                    rows.push(ConflictRow {
                        line_number: None,
                        spans: conflict_canvas::plain_spans("(empty)"),
                        char_count: "(empty)".chars().count(),
                        hunk_idx: Some(hunk.index),
                        hunk_selected: is_selected,
                        show_checkbox: true,
                        is_placeholder: true,
                        gutter_kind: GutterKind::Side(side),
                    });
                } else {
                    for (idx, line) in source_lines.iter().enumerate() {
                        let spans = resolve_spans(highlighted, Some(line_number), line);
                        let char_count = row_char_count(&spans);
                        rows.push(ConflictRow {
                            line_number: Some(line_number),
                            spans,
                            char_count,
                            hunk_idx: Some(hunk.index),
                            hunk_selected: is_selected,
                            show_checkbox: idx == checkbox_line_idx,
                            is_placeholder: false,
                            gutter_kind: GutterKind::Side(side),
                        });
                        line_number += 1;
                    }
                }
            }
        }
    }

    rows
}

pub(super) fn build_conflict_output_rows(
    result: &ConflictResolutionResult,
    selections: &[ConflictHunkSelection],
) -> Vec<ConflictRow> {
    let lines = super::super::conflict::conflict_resolution_output_lines(result, selections);
    lines
        .into_iter()
        .enumerate()
        .map(|(idx, line)| {
            let spans = conflict_canvas::plain_spans(&line);
            let char_count = row_char_count(&spans);
            ConflictRow {
                line_number: Some((idx + 1) as u32),
                spans,
                char_count,
                hunk_idx: None,
                hunk_selected: false,
                show_checkbox: false,
                is_placeholder: false,
                gutter_kind: GutterKind::Output,
            }
        })
        .collect()
}

pub(in crate::screens::repository) fn build_conflict_rows_for_canvas(
    canvas_id: DiffCanvasId,
    result: &ConflictResolutionResult,
    selections: &[ConflictHunkSelection],
    ours_hl: Option<&HighlightedFile>,
    theirs_hl: Option<&HighlightedFile>,
) -> Option<Arc<TextCanvasData>> {
    let span = crate::perf::Span::new("cpu.diff_row_building").field("kind", "conflict");
    let char_w = diff_char_width();
    let result = if canvas_id == CANVAS_ID_OURS {
        let rows = build_conflict_side_rows(result, selections, ConflictSide::Ours, ours_hl);
        Some(conflict_canvas::build_side_canvas_data(rows, char_w))
    } else if canvas_id == CANVAS_ID_THEIRS {
        let rows = build_conflict_side_rows(result, selections, ConflictSide::Theirs, theirs_hl);
        Some(conflict_canvas::build_side_canvas_data(rows, char_w))
    } else if canvas_id == CANVAS_ID_OUTPUT {
        let rows = build_conflict_output_rows(result, selections);
        Some(conflict_canvas::build_output_canvas_data(rows, char_w))
    } else {
        None
    };
    span.finish_with("canvas", canvas_id.0);
    result
}

pub(super) fn resolve_spans(
    highlighted: Option<&HighlightedFile>,
    line_number: Option<u32>,
    fallback: &str,
) -> Vec<SyntaxHighlightedSpan> {
    match highlighted.and_then(|hf| line_number.map(|ln| hf.line(ln))) {
        Some(spans) if !spans.is_empty() => spans.to_vec(),
        _ => conflict_canvas::plain_spans(fallback),
    }
}

pub(super) fn row_char_count(spans: &[SyntaxHighlightedSpan]) -> usize {
    spans.iter().map(|s| s.text.chars().count()).sum()
}

fn highlighted_spans_for_diff_line(
    line: &WorkingTreeDiffLine,
    old_highlighted: Option<&HighlightedFile>,
    new_highlighted: Option<&HighlightedFile>,
) -> Option<Vec<SyntaxHighlightedSpan>> {
    let spans = match line.line_type {
        DiffLineType::Deletion => extract_highlighted_line(old_highlighted, line.old_lineno)
            .or_else(|| extract_highlighted_line(new_highlighted, line.new_lineno)),
        DiffLineType::Addition => extract_highlighted_line(new_highlighted, line.new_lineno)
            .or_else(|| extract_highlighted_line(old_highlighted, line.old_lineno)),
        DiffLineType::Context => extract_highlighted_line(new_highlighted, line.new_lineno)
            .or_else(|| extract_highlighted_line(old_highlighted, line.old_lineno)),
        _ => None,
    }?;

    if spans.is_empty() {
        None
    } else {
        Some(spans.to_vec())
    }
}

fn extract_highlighted_line(
    highlighted: Option<&HighlightedFile>,
    line_number: Option<u32>,
) -> Option<&[SyntaxHighlightedSpan]> {
    highlighted.and_then(|file| line_number.map(|line| file.line(line)))
}

fn build_segment_bgs(segments: &[crate::services::DiffSegment]) -> Vec<SegmentBg> {
    // Only emit bgs for the "highlight" variants; the base line bg is painted
    // for the full line, so context/addition/deletion segments don't need
    // separate rectangles.
    let mut out = Vec::new();
    let mut col = 0usize;
    for seg in segments {
        let len = seg.text.chars().count();
        if matches!(
            seg.kind,
            SegmentKind::AdditionHighlight | SegmentKind::DeletionHighlight
        ) {
            out.push(SegmentBg {
                start_col: col,
                end_col: col + len,
                kind: seg.kind,
            });
        }
        col += len;
    }
    out
}

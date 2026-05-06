//! Data-transform layer. Converts service-level diff/conflict types into
//! canvas row types consumed by the diff + conflict canvas widgets. No
//! widget-tree construction here — pure function shapes.

use crate::{
    services::{
        ConflictBlock, ConflictResolutionResult, DiffContentSkipReason, DiffFallbacks,
        DiffLineType, DiffSide, HighlightedFile, SegmentKind, SyntaxHighlightedSpan,
        WorkingTreeDiffLine,
    },
    widgets::{
        conflict_canvas::{
            self, ConflictRow, GutterKind, CANVAS_ID_OURS, CANVAS_ID_OUTPUT, CANVAS_ID_THEIRS,
        },
        diff_canvas::{
            diff_char_width, DiffCanvasId, DiffHighlightProvider, DiffHighlightReference,
            DiffHighlightSide, DiffRow, DiffRowText, LineKind as CanvasLineKind, SegmentBg,
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
    fallbacks: &DiffFallbacks,
    highlight_provider: Option<Arc<dyn DiffHighlightProvider>>,
) -> Vec<DiffRow> {
    build_diff_rows_with_provider(lines, fallbacks, highlight_provider)
}

fn build_diff_rows_with_provider(
    lines: &[WorkingTreeDiffLine],
    fallbacks: &DiffFallbacks,
    highlight_provider: Option<Arc<dyn DiffHighlightProvider>>,
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
                let text = row_text_for_diff_line(line, highlight_provider.clone());
                let char_count = text.fallback().chars().count();
                let segment_bgs = build_segment_bgs(&line.segments);
                rows.push(DiffRow::Content {
                    old_lineno: line.old_lineno,
                    new_lineno: line.new_lineno,
                    kind,
                    text,
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
    let mut output_rows = 0usize;
    let result = if canvas_id == CANVAS_ID_OURS {
        let rows = build_conflict_side_rows(result, selections, ConflictSide::Ours, ours_hl);
        output_rows = rows.len();
        Some(conflict_canvas::build_side_canvas_data(rows, char_w))
    } else if canvas_id == CANVAS_ID_THEIRS {
        let rows = build_conflict_side_rows(result, selections, ConflictSide::Theirs, theirs_hl);
        output_rows = rows.len();
        Some(conflict_canvas::build_side_canvas_data(rows, char_w))
    } else if canvas_id == CANVAS_ID_OUTPUT {
        let rows = build_conflict_output_rows(result, selections);
        output_rows = rows.len();
        Some(conflict_canvas::build_output_canvas_data(rows, char_w))
    } else {
        None
    };
    span.field("output_rows", output_rows)
        .finish_with("canvas", canvas_id.0);
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

fn row_text_for_diff_line(
    line: &WorkingTreeDiffLine,
    provider: Option<Arc<dyn crate::widgets::diff_canvas::DiffHighlightProvider>>,
) -> DiffRowText {
    let Some((source, alternate_source)) = highlight_references_for_diff_line(line) else {
        return DiffRowText::Plain(line.content.clone());
    };

    DiffRowText::Highlighted {
        source,
        alternate_source,
        fallback: line.content.clone(),
        provider,
    }
}

fn highlight_references_for_diff_line(
    line: &WorkingTreeDiffLine,
) -> Option<(DiffHighlightReference, Option<DiffHighlightReference>)> {
    let preferred = match line.line_type {
        DiffLineType::Deletion => highlight_reference(DiffHighlightSide::Old, line.old_lineno),
        DiffLineType::Addition => highlight_reference(DiffHighlightSide::New, line.new_lineno),
        DiffLineType::Context => highlight_reference(DiffHighlightSide::New, line.new_lineno),
        _ => None,
    };
    let alternate = match line.line_type {
        DiffLineType::Deletion => highlight_reference(DiffHighlightSide::New, line.new_lineno),
        DiffLineType::Addition | DiffLineType::Context => {
            highlight_reference(DiffHighlightSide::Old, line.old_lineno)
        }
        _ => None,
    };

    preferred.or(alternate).map(|source| {
        let alternate_source = alternate.filter(|alternate| *alternate != source);
        (source, alternate_source)
    })
}

fn highlight_reference(
    side: DiffHighlightSide,
    line_number: Option<u32>,
) -> Option<DiffHighlightReference> {
    line_number.map(|line_number| DiffHighlightReference { side, line_number })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::text::CanvasRow;

    fn line(
        line_type: DiffLineType,
        content: &str,
        old_lineno: Option<u32>,
        new_lineno: Option<u32>,
    ) -> WorkingTreeDiffLine {
        WorkingTreeDiffLine {
            line_type,
            content: content.to_string(),
            old_lineno,
            new_lineno,
            segments: Vec::new(),
        }
    }

    fn content_text(row: &DiffRow) -> &DiffRowText {
        let DiffRow::Content { text, .. } = row else {
            panic!("expected content row");
        };
        text
    }

    fn source(text: &DiffRowText) -> Option<DiffHighlightReference> {
        match text {
            DiffRowText::Plain(_) => None,
            DiffRowText::Highlighted { source, .. } => Some(*source),
        }
    }

    fn alternate_source(text: &DiffRowText) -> Option<DiffHighlightReference> {
        match text {
            DiffRowText::Plain(_) => None,
            DiffRowText::Highlighted {
                alternate_source, ..
            } => *alternate_source,
        }
    }

    #[test]
    fn diff_rows_carry_preferred_highlight_sources() {
        let rows = build_diff_rows_public(
            &[
                line(DiffLineType::Addition, "added", None, Some(7)),
                line(DiffLineType::Deletion, "deleted", Some(3), None),
                line(DiffLineType::Context, "same", Some(11), Some(12)),
            ],
            &DiffFallbacks::default(),
            None,
        );

        assert_eq!(
            source(content_text(&rows[0])),
            Some(DiffHighlightReference {
                side: DiffHighlightSide::New,
                line_number: 7,
            })
        );
        assert_eq!(
            source(content_text(&rows[1])),
            Some(DiffHighlightReference {
                side: DiffHighlightSide::Old,
                line_number: 3,
            })
        );
        assert_eq!(
            source(content_text(&rows[2])),
            Some(DiffHighlightReference {
                side: DiffHighlightSide::New,
                line_number: 12,
            })
        );
        assert_eq!(
            alternate_source(content_text(&rows[2])),
            Some(DiffHighlightReference {
                side: DiffHighlightSide::Old,
                line_number: 11,
            })
        );
    }

    #[test]
    fn diff_rows_fall_back_to_available_side_when_preferred_line_is_missing() {
        let rows = build_diff_rows_public(
            &[line(DiffLineType::Addition, "odd add", Some(4), None)],
            &DiffFallbacks::default(),
            None,
        );

        assert_eq!(
            source(content_text(&rows[0])),
            Some(DiffHighlightReference {
                side: DiffHighlightSide::Old,
                line_number: 4,
            })
        );
        assert_eq!(alternate_source(content_text(&rows[0])), None);
    }

    #[test]
    fn diff_rows_fall_back_to_available_side_or_plain_text_when_line_numbers_are_missing() {
        let rows = build_diff_rows_public(
            &[
                line(DiffLineType::Deletion, "deleted", None, Some(8)),
                line(DiffLineType::Context, "context", Some(4), None),
                line(DiffLineType::Addition, "unmapped", None, None),
            ],
            &DiffFallbacks::default(),
            None,
        );

        assert_eq!(
            source(content_text(&rows[0])),
            Some(DiffHighlightReference {
                side: DiffHighlightSide::New,
                line_number: 8,
            })
        );
        assert_eq!(alternate_source(content_text(&rows[0])), None);
        assert_eq!(
            source(content_text(&rows[1])),
            Some(DiffHighlightReference {
                side: DiffHighlightSide::Old,
                line_number: 4,
            })
        );
        assert_eq!(alternate_source(content_text(&rows[1])), None);
        assert!(matches!(content_text(&rows[2]), DiffRowText::Plain(text) if text == "unmapped"));
    }

    #[test]
    fn diff_rows_use_fallback_text_for_raw_text_and_char_count() {
        let fallback = "écho";
        let rows = build_diff_rows_public(
            &[line(DiffLineType::Addition, fallback, None, Some(1))],
            &DiffFallbacks::default(),
            None,
        );

        let DiffRow::Content { char_count, .. } = &rows[0] else {
            panic!("expected content row");
        };
        assert_eq!(*char_count, fallback.chars().count());
        assert_eq!(rows[0].raw_text(), fallback);
        assert_eq!(content_text(&rows[0]).fallback(), fallback);
    }
}

//! Conflict-resolution state: three-way buffers (ours/theirs/output), hunk
//! selections, scroll sync math that keeps the two side buffers aligned on
//! their shared content axis, and the `save` action that writes the resolved
//! output back to disk.

use std::sync::Arc;

use iced::{widget::scrollable, Task};

use crate::{
    message::Message,
    services::{ConflictBlock, ConflictResolutionResult, HighlightedFile},
};

use super::super::super::state::{
    conflict_ours_scroll_id, conflict_output_scroll_id, conflict_theirs_scroll_id,
};
use super::{DiffPanel, DiffPanelAction};

pub const CONFLICT_RESOLUTION_LINE_HEIGHT: f32 = 20.0;

pub use crate::widgets::conflict_canvas::ConflictSide;

#[derive(Debug, Clone)]
pub struct ConflictFileResolutionState {
    pub file_path: String,
    pub result: Option<ConflictResolutionResult>,
    pub selections: Vec<ConflictHunkSelection>,
    pub selected_file_idx: usize,
    pub initial_scroll_done: bool,
    pub ignore_initial_ours_top_scroll: bool,
    pub ignore_initial_theirs_top_scroll: bool,
    pub ignore_next_ours_scroll: bool,
    pub ignore_next_theirs_scroll: bool,
    pub ours_highlighted: Option<Arc<HighlightedFile>>,
    pub theirs_highlighted: Option<Arc<HighlightedFile>>,
    pub render_generation: u64,
    pub ours_scroll_offset_y: f32,
    pub theirs_scroll_offset_y: f32,
    pub output_scroll_offset_y: f32,
    pub ours_scroll_offset_x: f32,
    pub theirs_scroll_offset_x: f32,
    pub output_scroll_offset_x: f32,
    pub ours_viewport_height: Option<f32>,
    pub theirs_viewport_height: Option<f32>,
    pub output_viewport_height: Option<f32>,
    pub ours_canvas: Option<Arc<crate::widgets::text::TextCanvasData>>,
    pub theirs_canvas: Option<Arc<crate::widgets::text::TextCanvasData>>,
    pub output_canvas: Option<Arc<crate::widgets::text::TextCanvasData>>,
}

impl ConflictFileResolutionState {
    pub(in crate::screens::repository) fn rebuild_canvases(&mut self) {
        use crate::widgets::conflict_canvas::{CANVAS_ID_OURS, CANVAS_ID_OUTPUT, CANVAS_ID_THEIRS};
        let Some(result) = self.result.as_ref() else {
            self.ours_canvas = None;
            self.theirs_canvas = None;
            self.output_canvas = None;
            return;
        };
        let ours_hl = self.ours_highlighted.as_deref();
        let theirs_hl = self.theirs_highlighted.as_deref();
        let ours = super::view::build_conflict_rows_for_canvas(
            CANVAS_ID_OURS,
            result,
            &self.selections,
            ours_hl,
            theirs_hl,
        );
        let theirs = super::view::build_conflict_rows_for_canvas(
            CANVAS_ID_THEIRS,
            result,
            &self.selections,
            ours_hl,
            theirs_hl,
        );
        let output = super::view::build_conflict_rows_for_canvas(
            CANVAS_ID_OUTPUT,
            result,
            &self.selections,
            ours_hl,
            theirs_hl,
        );
        self.ours_canvas = ours;
        self.theirs_canvas = theirs;
        self.output_canvas = output;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictScrollTarget {
    Ours,
    Theirs,
    Output,
}

#[derive(Debug, Clone, Default)]
pub struct ConflictHunkSelection {
    pub picked: Vec<ConflictSide>,
}

impl ConflictHunkSelection {
    pub(in crate::screens::repository) fn has(&self, side: ConflictSide) -> bool {
        self.picked.contains(&side)
    }

    pub(in crate::screens::repository) fn toggle(&mut self, side: ConflictSide) {
        if let Some(index) = self.picked.iter().position(|picked| *picked == side) {
            self.picked.remove(index);
        } else {
            self.picked.push(side);
        }
    }
}

pub(in crate::screens::repository) struct ConflictResolverViewModel<'a> {
    pub(in crate::screens::repository) file_path: &'a str,
    pub(in crate::screens::repository) result: Option<&'a ConflictResolutionResult>,
    pub(in crate::screens::repository) selections: &'a [ConflictHunkSelection],
    pub(in crate::screens::repository) ours_canvas:
        Option<Arc<crate::widgets::text::TextCanvasData>>,
    pub(in crate::screens::repository) theirs_canvas:
        Option<Arc<crate::widgets::text::TextCanvasData>>,
    pub(in crate::screens::repository) output_canvas:
        Option<Arc<crate::widgets::text::TextCanvasData>>,
    pub(in crate::screens::repository) ours_scroll_offset_y: f32,
    pub(in crate::screens::repository) theirs_scroll_offset_y: f32,
    pub(in crate::screens::repository) output_scroll_offset_y: f32,
    pub(in crate::screens::repository) ours_selection:
        Option<crate::widgets::diff_canvas::DiffSelection>,
    pub(in crate::screens::repository) theirs_selection:
        Option<crate::widgets::diff_canvas::DiffSelection>,
    pub(in crate::screens::repository) output_selection:
        Option<crate::widgets::diff_canvas::DiffSelection>,
    pub(in crate::screens::repository) shift_held: bool,
    pub(in crate::screens::repository) save_busy: bool,
    /// Single search overlay for the whole conflict view. Rendered at a
    /// stable tree position (top of the conflict body's Stack) so the
    /// text_input's internal state — including caret position — survives
    /// moves between buffers. Positioning onto the hovered buffer happens
    /// inside the overlay via `responsive` + padding.
    pub(in crate::screens::repository) search_overlay: Option<iced::Element<'a, Message>>,
}

impl DiffPanel {
    pub(in crate::screens::repository) fn is_conflict_fullscreen(&self) -> bool {
        matches!(self.center_view_mode, super::CenterViewMode::DiffView)
            && self.conflict_file_resolution.is_some()
    }

    pub(in crate::screens::repository) fn on_conflict_resolution_loaded(
        &mut self,
        generation: u64,
        result: Result<ConflictResolutionResult, crate::services::GitError>,
    ) -> Option<DiffPanelAction> {
        let Ok(conflict_result) = result else {
            return None;
        };
        if let Some(state) = &mut self.conflict_file_resolution {
            if state.file_path == conflict_result.file_path && state.render_generation == generation
            {
                let hunk_count = conflict_hunk_count(&conflict_result);
                let first_load = state.result.is_none();
                state.result = Some(conflict_result);
                if first_load {
                    state.initial_scroll_done = false;
                    state.ignore_initial_ours_top_scroll = false;
                    state.ignore_initial_theirs_top_scroll = false;
                    state.ignore_next_ours_scroll = false;
                    state.ignore_next_theirs_scroll = false;
                }
                if state.selections.len() != hunk_count {
                    state.selections = vec![ConflictHunkSelection::default(); hunk_count];
                }
                state.rebuild_canvases();
                let file_path = state.file_path.clone();
                let ours_content = state
                    .result
                    .as_ref()
                    .and_then(|r| conflict_side_highlight_content(r, ConflictSide::Ours));
                let theirs_content = state
                    .result
                    .as_ref()
                    .and_then(|r| conflict_side_highlight_content(r, ConflictSide::Theirs));
                return Some(DiffPanelAction::RunConflictHighlight {
                    generation,
                    file_path,
                    ours_content,
                    theirs_content,
                });
            }
        }
        None
    }

    pub(in crate::screens::repository) fn on_conflict_highlight_ready(
        &mut self,
        generation: u64,
        ours: Option<Arc<HighlightedFile>>,
        theirs: Option<Arc<HighlightedFile>>,
    ) {
        if let Some(state) = &mut self.conflict_file_resolution {
            if state.render_generation == generation {
                state.ours_highlighted = ours;
                state.theirs_highlighted = theirs;
                state.rebuild_canvases();
            }
        }
    }

    pub(in crate::screens::repository) fn toggle_hunk_side(
        &mut self,
        hunk_idx: usize,
        side: ConflictSide,
    ) {
        if let Some(state) = self.conflict_file_resolution.as_mut() {
            if let Some(selection) = state.selections.get_mut(hunk_idx) {
                selection.toggle(side);
                state.rebuild_canvases();
            }
        }
    }

    pub(in crate::screens::repository) fn toggle_all_conflict_side(&mut self, side: ConflictSide) {
        let Some(state) = self.conflict_file_resolution.as_mut() else {
            return;
        };
        if state.selections.is_empty() {
            return;
        }

        let all_selected = state.selections.iter().all(|selection| selection.has(side));
        for selection in &mut state.selections {
            if all_selected {
                selection.picked.retain(|picked| *picked != side);
            } else if !selection.has(side) {
                selection.picked.push(side);
            }
        }
        state.rebuild_canvases();
    }

    pub(in crate::screens::repository) fn save_conflict_resolution(
        &self,
    ) -> Option<DiffPanelAction> {
        let state = self.conflict_file_resolution.as_ref()?;
        let _ = state.result.as_ref()?;
        Some(DiffPanelAction::ConflictResolutionSaveRequested)
    }

    pub(in crate::screens::repository) fn handle_conflict_buffer_scrolled(
        &mut self,
        side: ConflictSide,
        viewport: scrollable::Viewport,
    ) -> Task<Message> {
        if let Some(state) = self.conflict_file_resolution.as_mut() {
            let viewport_height = viewport.bounds().height;
            match side {
                ConflictSide::Ours => state.ours_viewport_height = Some(viewport_height),
                ConflictSide::Theirs => state.theirs_viewport_height = Some(viewport_height),
            }
        }

        let should_initial_scroll = self
            .conflict_file_resolution
            .as_ref()
            .is_some_and(|state| !state.initial_scroll_done);

        if should_initial_scroll {
            if let Some(state) = self.conflict_file_resolution.as_mut() {
                state.initial_scroll_done = true;
                state.ignore_initial_ours_top_scroll = true;
                state.ignore_initial_theirs_top_scroll = true;
            }
            let viewport_height = viewport.bounds().height;
            return self.scroll_conflict_resolution_to_first_hunk(viewport_height);
        }

        if let Some(state) = self.conflict_file_resolution.as_mut() {
            let off = viewport.absolute_offset();
            match side {
                ConflictSide::Ours => {
                    state.ours_scroll_offset_y = off.y;
                    state.ours_scroll_offset_x = off.x;
                }
                ConflictSide::Theirs => {
                    state.theirs_scroll_offset_y = off.y;
                    state.theirs_scroll_offset_x = off.x;
                }
            }
            let ignore_top_scroll = match side {
                ConflictSide::Ours => &mut state.ignore_initial_ours_top_scroll,
                ConflictSide::Theirs => &mut state.ignore_initial_theirs_top_scroll,
            };
            if *ignore_top_scroll {
                if off.y <= 1.0 {
                    *ignore_top_scroll = false;
                    return Task::none();
                }
                *ignore_top_scroll = false;
            }

            let ignore_sync_echo = match side {
                ConflictSide::Ours => &mut state.ignore_next_ours_scroll,
                ConflictSide::Theirs => &mut state.ignore_next_theirs_scroll,
            };
            if *ignore_sync_echo {
                *ignore_sync_echo = false;
                return Task::none();
            }
        }

        self.sync_conflict_buffer_scroll(side, viewport)
    }

    pub(in crate::screens::repository) fn handle_conflict_output_scrolled(
        &mut self,
        viewport: scrollable::Viewport,
    ) -> Task<Message> {
        if let Some(state) = self.conflict_file_resolution.as_mut() {
            let off = viewport.absolute_offset();
            state.output_scroll_offset_y = off.y;
            state.output_scroll_offset_x = off.x;
            state.output_viewport_height = Some(viewport.bounds().height);
        }
        Task::none()
    }

    pub(in crate::screens::repository) fn scroll_conflict_resolution_to_first_hunk(
        &self,
        viewport_height: f32,
    ) -> Task<Message> {
        let Some(result) = self
            .conflict_file_resolution
            .as_ref()
            .and_then(|state| state.result.as_ref())
        else {
            return Task::none();
        };
        let Some(axis_center) = first_conflict_axis_center(result) else {
            return Task::none();
        };

        let segments = conflict_scroll_segments(result);
        let ours_center = conflict_axis_to_side_line(&segments, ConflictSide::Ours, axis_center);
        let theirs_center =
            conflict_axis_to_side_line(&segments, ConflictSide::Theirs, axis_center);
        let output_center = first_conflict_output_center(result).unwrap_or(axis_center);

        Task::batch(vec![
            scroll_conflict_side_to_line_center(
                ConflictSide::Ours,
                ours_center,
                viewport_height,
                0.0,
            ),
            scroll_conflict_side_to_line_center(
                ConflictSide::Theirs,
                theirs_center,
                viewport_height,
                0.0,
            ),
            scroll_conflict_output_to_line_center(output_center, viewport_height, 0.0),
        ])
    }

    pub(in crate::screens::repository) fn sync_conflict_buffer_scroll(
        &mut self,
        source_side: ConflictSide,
        viewport: scrollable::Viewport,
    ) -> Task<Message> {
        let Some(result) = self
            .conflict_file_resolution
            .as_ref()
            .and_then(|state| state.result.as_ref())
        else {
            return Task::none();
        };

        let target_side = match source_side {
            ConflictSide::Ours => ConflictSide::Theirs,
            ConflictSide::Theirs => ConflictSide::Ours,
        };
        let offset = viewport.absolute_offset();
        let viewport_height = viewport.bounds().height;
        let source_center_line =
            (offset.y + (viewport_height / 2.0)) / CONFLICT_RESOLUTION_LINE_HEIGHT;
        let segments = conflict_scroll_segments(result);
        let axis_line = conflict_side_line_to_axis(&segments, source_side, source_center_line);
        let target_center_line = conflict_axis_to_side_line(&segments, target_side, axis_line);

        if let Some(state) = self.conflict_file_resolution.as_mut() {
            match target_side {
                ConflictSide::Ours => state.ignore_next_ours_scroll = true,
                ConflictSide::Theirs => state.ignore_next_theirs_scroll = true,
            }
        }

        scroll_conflict_side_to_line_center(
            target_side,
            target_center_line,
            viewport_height,
            offset.x,
        )
    }
}

pub(in crate::screens::repository) fn conflict_side_lines(
    result: &ConflictResolutionResult,
    side: ConflictSide,
) -> Vec<String> {
    let mut out = Vec::new();
    for block in &result.blocks {
        match block {
            ConflictBlock::Context(ctx) => out.extend(ctx.iter().cloned()),
            ConflictBlock::Conflict(hunk) => match side {
                ConflictSide::Ours => out.extend(hunk.ours_lines.iter().cloned()),
                ConflictSide::Theirs => out.extend(hunk.theirs_lines.iter().cloned()),
            },
        }
    }
    out
}

pub(super) fn conflict_side_highlight_content(
    result: &ConflictResolutionResult,
    side: ConflictSide,
) -> Option<String> {
    let lines = conflict_side_lines(result, side);
    if lines.len() > crate::services::git::working_tree_diff::MAX_HIGHLIGHT_LINES {
        return None;
    }
    let byte_count = lines.iter().map(|line| line.len() + 1).sum::<usize>();
    if byte_count > crate::services::git::working_tree_diff::MAX_HIGHLIGHT_FILE_BYTES {
        return None;
    }
    Some(lines.join("\n"))
}

pub fn conflict_resolution_output_lines(
    result: &ConflictResolutionResult,
    selections: &[ConflictHunkSelection],
) -> Vec<String> {
    let mut output = Vec::new();

    for block in &result.blocks {
        match block {
            ConflictBlock::Context(lines) => output.extend(lines.iter().cloned()),
            ConflictBlock::Conflict(hunk) => {
                let picked = selections
                    .get(hunk.index)
                    .map(|selection| selection.picked.as_slice())
                    .unwrap_or(&[]);

                if picked.is_empty() {
                    output.extend(hunk.original_lines.iter().cloned());
                } else {
                    for side in picked {
                        match side {
                            ConflictSide::Ours => output.extend(hunk.ours_lines.iter().cloned()),
                            ConflictSide::Theirs => {
                                output.extend(hunk.theirs_lines.iter().cloned())
                            }
                        }
                    }
                }
            }
        }
    }

    output
}

pub(in crate::screens::repository) fn conflict_resolution_output_text(
    result: &ConflictResolutionResult,
    selections: &[ConflictHunkSelection],
) -> String {
    let mut text = conflict_resolution_output_lines(result, selections).join("\n");
    if result.has_trailing_newline {
        text.push('\n');
    }
    text
}

pub(in crate::screens::repository) fn conflict_hunk_count(
    result: &ConflictResolutionResult,
) -> usize {
    result
        .blocks
        .iter()
        .filter(|block| matches!(block, ConflictBlock::Conflict(_)))
        .count()
}

#[derive(Debug, Clone, Copy)]
pub(in crate::screens::repository) struct ConflictScrollSegment {
    axis_start: f32,
    axis_len: f32,
    ours_start: f32,
    ours_len: f32,
    theirs_start: f32,
    theirs_len: f32,
}

impl ConflictScrollSegment {
    fn side_start(self, side: ConflictSide) -> f32 {
        match side {
            ConflictSide::Ours => self.ours_start,
            ConflictSide::Theirs => self.theirs_start,
        }
    }

    fn side_len(self, side: ConflictSide) -> f32 {
        match side {
            ConflictSide::Ours => self.ours_len,
            ConflictSide::Theirs => self.theirs_len,
        }
    }
}

pub(in crate::screens::repository) fn conflict_scroll_segments(
    result: &ConflictResolutionResult,
) -> Vec<ConflictScrollSegment> {
    let mut segments = Vec::new();
    let mut axis_start = 0.0;
    let mut ours_start = 0.0;
    let mut theirs_start = 0.0;

    for block in &result.blocks {
        match block {
            ConflictBlock::Context(lines) => {
                let len = lines.len() as f32;
                if len > 0.0 {
                    segments.push(ConflictScrollSegment {
                        axis_start,
                        axis_len: len,
                        ours_start,
                        ours_len: len,
                        theirs_start,
                        theirs_len: len,
                    });
                    axis_start += len;
                    ours_start += len;
                    theirs_start += len;
                }
            }
            ConflictBlock::Conflict(hunk) => {
                let ours_len = displayed_conflict_hunk_len(&hunk.ours_lines) as f32;
                let theirs_len = displayed_conflict_hunk_len(&hunk.theirs_lines) as f32;
                let axis_len = ours_len.max(theirs_len);
                segments.push(ConflictScrollSegment {
                    axis_start,
                    axis_len,
                    ours_start,
                    ours_len,
                    theirs_start,
                    theirs_len,
                });
                axis_start += axis_len;
                ours_start += ours_len;
                theirs_start += theirs_len;
            }
        }
    }

    segments
}

pub(in crate::screens::repository) fn displayed_conflict_hunk_len(lines: &[String]) -> usize {
    lines.len().max(1)
}

pub(in crate::screens::repository) fn conflict_side_line_to_axis(
    segments: &[ConflictScrollSegment],
    side: ConflictSide,
    line: f32,
) -> f32 {
    for segment in segments {
        let side_start = segment.side_start(side);
        let side_len = segment.side_len(side);
        if line <= side_start + side_len {
            let within = (line - side_start).clamp(0.0, side_len);
            let ratio = if side_len <= f32::EPSILON {
                0.0
            } else {
                within / side_len
            };
            return segment.axis_start + ratio * segment.axis_len;
        }
    }

    segments
        .last()
        .map(|segment| segment.axis_start + segment.axis_len)
        .unwrap_or(0.0)
}

pub(in crate::screens::repository) fn conflict_axis_to_side_line(
    segments: &[ConflictScrollSegment],
    side: ConflictSide,
    axis_line: f32,
) -> f32 {
    for segment in segments {
        if axis_line <= segment.axis_start + segment.axis_len {
            let within = (axis_line - segment.axis_start).clamp(0.0, segment.axis_len);
            let ratio = if segment.axis_len <= f32::EPSILON {
                0.0
            } else {
                within / segment.axis_len
            };
            return segment.side_start(side) + ratio * segment.side_len(side);
        }
    }

    segments
        .last()
        .map(|segment| segment.side_start(side) + segment.side_len(side))
        .unwrap_or(0.0)
}

pub(in crate::screens::repository) fn first_conflict_axis_center(
    result: &ConflictResolutionResult,
) -> Option<f32> {
    let mut axis_start = 0.0;
    for block in &result.blocks {
        match block {
            ConflictBlock::Context(lines) => axis_start += lines.len() as f32,
            ConflictBlock::Conflict(hunk) => {
                let ours_len = displayed_conflict_hunk_len(&hunk.ours_lines) as f32;
                let theirs_len = displayed_conflict_hunk_len(&hunk.theirs_lines) as f32;
                return Some(axis_start + (ours_len.max(theirs_len) / 2.0));
            }
        }
    }
    None
}

pub(in crate::screens::repository) fn first_conflict_output_center(
    result: &ConflictResolutionResult,
) -> Option<f32> {
    let mut output_start = 0.0;
    for block in &result.blocks {
        match block {
            ConflictBlock::Context(lines) => output_start += lines.len() as f32,
            ConflictBlock::Conflict(hunk) => {
                return Some(output_start + (hunk.original_lines.len().max(1) as f32 / 2.0));
            }
        }
    }
    None
}

pub(in crate::screens::repository) fn conflict_line_center_offset(
    line_center: f32,
    viewport_height: f32,
) -> f32 {
    (line_center * CONFLICT_RESOLUTION_LINE_HEIGHT - viewport_height / 2.0).max(0.0)
}

pub(in crate::screens::repository) fn scroll_conflict_side_to_line_center(
    side: ConflictSide,
    line_center: f32,
    viewport_height: f32,
    x: f32,
) -> Task<Message> {
    iced::widget::operation::scroll_to(
        conflict_scroll_id_for_side(side),
        scrollable::AbsoluteOffset {
            x,
            y: conflict_line_center_offset(line_center, viewport_height),
        },
    )
}

pub(in crate::screens::repository) fn scroll_conflict_output_to_line_center(
    line_center: f32,
    viewport_height: f32,
    x: f32,
) -> Task<Message> {
    iced::widget::operation::scroll_to(
        conflict_output_scroll_id(),
        scrollable::AbsoluteOffset {
            x,
            y: conflict_line_center_offset(line_center, viewport_height),
        },
    )
}

pub(in crate::screens::repository) fn conflict_scroll_id_for_side(
    side: ConflictSide,
) -> iced::widget::Id {
    match side {
        ConflictSide::Ours => conflict_ours_scroll_id(),
        ConflictSide::Theirs => conflict_theirs_scroll_id(),
    }
}

/// Build a `TextCanvasData` for a specific conflict canvas. Used by
/// `DiffPanel::copy_conflict_selection_text` to resolve the selection text
/// of whichever buffer carried the active selection.
pub(in crate::screens::repository) fn build_conflict_rows_for_canvas(
    canvas_id: crate::widgets::diff_canvas::DiffCanvasId,
    result: &ConflictResolutionResult,
    selections: &[ConflictHunkSelection],
    ours_hl: Option<&HighlightedFile>,
    theirs_hl: Option<&HighlightedFile>,
) -> Option<Arc<crate::widgets::text::TextCanvasData>> {
    super::view::build_conflict_rows_for_canvas(canvas_id, result, selections, ours_hl, theirs_hl)
}

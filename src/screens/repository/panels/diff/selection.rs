//! Selection state machine shared across every diff canvas — char/word drag
//! modes, double-click promotion, auto-scroll capture, and copy-to-text.
//!
//! Lives as `impl DiffPanel` methods (plus a couple helpers) so the mode-
//! specific files (`dirty`, `commit`, `merged`, `conflict`) don't each carry
//! their own selection logic.

use crate::widgets::diff_canvas::{
    self as dc, DiffCanvasData, DiffCanvasId, DiffPosition, DiffSelection, SelectionMode,
};

use super::DiffPanel;

impl DiffPanel {
    pub(in crate::screens::repository) fn begin_selection_char(
        &mut self,
        canvas_id: DiffCanvasId,
        row: usize,
        col: usize,
    ) {
        let pos = DiffPosition { row, col };
        self.diff_selection = Some((
            canvas_id,
            DiffSelection {
                anchor: pos,
                head: pos,
                dragging: true,
                mode: SelectionMode::Char,
                anchor_word: None,
            },
        ));
    }

    pub(in crate::screens::repository) fn begin_selection_word(
        &mut self,
        canvas_id: DiffCanvasId,
        row: usize,
        col: usize,
        data: &DiffCanvasData,
    ) {
        let pos = DiffPosition { row, col };
        let (ws, we) = data.word_range_at(pos);
        self.diff_selection = Some((
            canvas_id,
            DiffSelection {
                anchor: ws,
                head: we,
                dragging: true,
                mode: SelectionMode::Word,
                anchor_word: Some((ws, we)),
            },
        ));
    }

    pub(in crate::screens::repository) fn extend_selection(
        &mut self,
        canvas_id: DiffCanvasId,
        row: usize,
        col: usize,
    ) {
        let Some((active_id, sel)) = self.diff_selection.as_mut() else {
            return;
        };
        if *active_id != canvas_id {
            return;
        }
        sel.head = DiffPosition { row, col };
    }

    pub(in crate::screens::repository) fn extend_selection_word(
        &mut self,
        canvas_id: DiffCanvasId,
        row: usize,
        col: usize,
        data: &DiffCanvasData,
    ) {
        let Some((active_id, sel)) = self.diff_selection.as_mut() else {
            return;
        };
        if *active_id != canvas_id {
            return;
        }
        let Some((aw_start, aw_end)) = sel.anchor_word else {
            sel.head = DiffPosition { row, col };
            return;
        };
        let (cw_start, cw_end) = data.word_range_at(DiffPosition { row, col });
        let cursor_after = (cw_start.row, cw_start.col) >= (aw_end.row, aw_end.col);
        if cursor_after {
            sel.anchor = aw_start;
            sel.head = cw_end;
        } else {
            sel.anchor = aw_end;
            sel.head = cw_start;
        }
    }

    pub(in crate::screens::repository) fn finalize_selection(&mut self, canvas_id: DiffCanvasId) {
        if let Some((active_id, sel)) = self.diff_selection.as_mut() {
            if *active_id != canvas_id {
                return;
            }
            sel.dragging = false;
            if sel.anchor == sel.head {
                self.diff_selection = None;
            }
        }
        self.diff_viewport_rect = None;
        self.diff_drag_canvas_data = None;
    }

    pub(in crate::screens::repository) fn is_diff_dragging(&self) -> bool {
        self.diff_selection
            .as_ref()
            .map(|(_, s)| s.dragging)
            .unwrap_or(false)
    }

    pub(in crate::screens::repository) fn selection_for(
        &self,
        canvas_id: DiffCanvasId,
    ) -> Option<DiffSelection> {
        self.diff_selection
            .and_then(|(id, sel)| if id == canvas_id { Some(sel) } else { None })
    }

    pub(in crate::screens::repository) fn copy_selection_text(&self) -> Option<String> {
        let (canvas_id, selection) = self.diff_selection?;
        if canvas_id != dc::CANVAS_ID {
            return self.copy_conflict_selection_text(canvas_id, selection);
        }
        let data = self
            .dirty_file_diff
            .as_ref()
            .and_then(|s| s.render_data.as_deref())
            .or_else(|| {
                self.commit_file_diff
                    .as_ref()
                    .and_then(|s| s.render_data.as_deref())
            })
            .or_else(|| {
                self.merged_file_diff
                    .as_ref()
                    .and_then(|s| s.render_data.as_deref())
            })?;
        Some(crate::widgets::text::selection_to_text(data, &selection))
    }

    fn copy_conflict_selection_text(
        &self,
        canvas_id: DiffCanvasId,
        selection: DiffSelection,
    ) -> Option<String> {
        let state = self.conflict_file_resolution.as_ref()?;
        let result = state.result.as_ref()?;
        let rows = super::conflict::build_conflict_rows_for_canvas(
            canvas_id,
            result,
            &state.selections,
            state.ours_highlighted.as_deref(),
            state.theirs_highlighted.as_deref(),
        )?;
        Some(crate::widgets::text::selection_to_text(&rows, &selection))
    }
}

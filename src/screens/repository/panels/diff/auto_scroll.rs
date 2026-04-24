//! Selection-drag auto-scroll tick. Takes the last window-level cursor
//! position and the viewport rect captured at drag begin, then scrolls the
//! active diff/conflict canvas at a quadratic rate past the edge band.
//! Re-extends the selection to the row/col under the cursor at the new
//! offset, so a held cursor past the edge keeps selecting as content scrolls.

use iced::{Point, Task};

use crate::{
    message::Message,
    widgets::{
        conflict_canvas::{CANVAS_ID_OURS, CANVAS_ID_OUTPUT, CANVAS_ID_THEIRS},
        diff_canvas::{DiffCanvasId, SelectionMode},
    },
};

use super::super::super::state;
use super::DiffPanel;

fn scroll_id_for_canvas(canvas_id: DiffCanvasId) -> iced::widget::Id {
    if canvas_id == CANVAS_ID_OURS {
        state::conflict_ours_scroll_id()
    } else if canvas_id == CANVAS_ID_THEIRS {
        state::conflict_theirs_scroll_id()
    } else if canvas_id == CANVAS_ID_OUTPUT {
        state::conflict_output_scroll_id()
    } else {
        state::diff_content_scroll_id()
    }
}

pub(in crate::screens::repository) fn tick(
    diff_panel: &mut DiffPanel,
    last_pointer_position: Option<Point>,
    delta_ms: f32,
) -> Option<Task<Message>> {
    if !diff_panel.is_diff_dragging() {
        return None;
    }
    // Gate on actual drag: skip while mouse button is down but the cursor
    // hasn't moved off the press position yet. Otherwise a bare click on an
    // already-scrolled canvas can trip the edge check and issue a scroll_to
    // that clamps scroll_y downward.
    let (canvas_id, sel) = diff_panel.diff_selection?;
    if sel.anchor == sel.head {
        return None;
    }
    let vp = diff_panel.diff_viewport_rect?;
    let cursor = last_pointer_position?;

    // Cursor in viewport-local coords. Viewport rect is fixed once captured,
    // so this stays meaningful even when the scrollable scrolls underneath
    // the cursor.
    let vx = cursor.x - vp.x;
    let vy = cursor.y - vp.y;

    const EDGE_BAND: f32 = 24.0;
    const MAX_PER_TICK: f32 = 400.0;
    // Quadratic response on distance past the edge band — 30px past
    // → ~1 px/tick, 100 → ~12, 300 → ~110, 600+ → 400 (cap).
    const REF: f32 = 30.0;

    fn axis_delta(pos: f32, view: f32) -> f32 {
        if view <= 0.0 {
            return 0.0;
        }
        let lo = EDGE_BAND;
        let hi = view - EDGE_BAND;
        let past = if pos < lo {
            pos - lo
        } else if pos > hi {
            pos - hi
        } else {
            0.0
        };
        let sign = past.signum();
        let scaled = (past.abs() / REF).powi(2) * REF;
        (sign * scaled).clamp(-MAX_PER_TICK, MAX_PER_TICK)
    }

    let dx = axis_delta(vx, vp.width);
    let dy = axis_delta(vy, vp.height);
    if dx == 0.0 && dy == 0.0 {
        return None;
    }

    let t = delta_ms / 16.0;
    // Current scroll offset of the scrollable we're driving — differs by
    // canvas. Diff canvas tracks in `diff_scroll_{x,y}`; conflict buffers
    // track in `ConflictFileResolutionState.{ours,theirs,output}_scroll_offset_y`.
    let (cur_x, cur_y) = {
        let cs = diff_panel.conflict_file_resolution.as_ref();
        if canvas_id == CANVAS_ID_OURS {
            (0.0, cs.map(|s| s.ours_scroll_offset_y).unwrap_or(0.0))
        } else if canvas_id == CANVAS_ID_THEIRS {
            (0.0, cs.map(|s| s.theirs_scroll_offset_y).unwrap_or(0.0))
        } else if canvas_id == CANVAS_ID_OUTPUT {
            (0.0, cs.map(|s| s.output_scroll_offset_y).unwrap_or(0.0))
        } else {
            (diff_panel.diff_scroll_x, diff_panel.diff_scroll_y)
        }
    };
    // Only clamp to 0 — let the scrollable clamp to its own max. Using a
    // stale content-size estimate to clamp here can pull a valid scroll_y
    // down (click at bottom → snaps to top).
    let new_y = (cur_y + dy * t).max(0.0);
    let new_x = (cur_x + dx * t).max(0.0);
    if (new_y - cur_y).abs() < 0.01 && (new_x - cur_x).abs() < 0.01 {
        return None;
    }

    // Extend selection to the row/col under the held cursor at the new scroll
    // position. Content-space point = viewport-local cursor + new scroll.
    if let Some(data) = diff_panel.diff_drag_canvas_data.clone() {
        let content_point = Point::new(vx + new_x, vy + new_y);
        if let Some(pos) = data.hit_test(content_point) {
            let mode = diff_panel.diff_selection.as_ref().map(|(_, s)| s.mode);
            if mode == Some(SelectionMode::Word) {
                diff_panel.extend_selection_word(canvas_id, pos.row, pos.col, &data);
            } else {
                diff_panel.extend_selection(canvas_id, pos.row, pos.col);
            }
        }
    }

    let scroll_id = scroll_id_for_canvas(canvas_id);
    Some(iced::widget::operation::scroll_to(
        scroll_id,
        iced::widget::scrollable::AbsoluteOffset { x: new_x, y: new_y },
    ))
}

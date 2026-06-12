//! Shared text-buffer search across every diff/conflict canvas. One widget
//! instance re-renders over whichever canvas the cursor is currently inside
//! — iced's `text_input` keeps focus + caret state because the widget keeps
//! the same position in the element tree.

use std::sync::Arc;

use iced::{Element, Task};

use crate::{
    message::Message,
    widgets::{
        diff_canvas::DiffCanvasId,
        search_widget::{
            self as sw, first_match_from_anchor, SearchWidget, SearchWidgetId, SearchWidgetMessage,
        },
        text::TextCanvasData,
    },
};

use super::super::super::state::{
    conflict_ours_scroll_id, conflict_output_scroll_id, conflict_theirs_scroll_id,
    diff_content_scroll_id,
};
use super::{DiffPanel, DiffPanelAction};

/// Stable id used for the shared search input across every buffer — same id
/// means iced's focus system carries focus through a buffer change.
pub(in crate::screens::repository) const SEARCH_WIDGET_ID: SearchWidgetId =
    SearchWidgetId("diff-shared");

pub(in crate::screens::repository) fn scroll_id_for_canvas(cid: DiffCanvasId) -> iced::widget::Id {
    use crate::widgets::conflict_canvas::{CANVAS_ID_OURS, CANVAS_ID_OUTPUT, CANVAS_ID_THEIRS};
    if cid == CANVAS_ID_OURS {
        conflict_ours_scroll_id()
    } else if cid == CANVAS_ID_THEIRS {
        conflict_theirs_scroll_id()
    } else if cid == CANVAS_ID_OUTPUT {
        conflict_output_scroll_id()
    } else {
        diff_content_scroll_id()
    }
}

impl DiffPanel {
    /// Open the shared text-search bar. Refocuses the existing widget if
    /// already open; otherwise creates a fresh one. The visible target buffer
    /// is derived from `hovered_canvas` at render time.
    pub(in crate::screens::repository) fn open_text_search(&mut self) {
        if let Some(widget) = self.text_search.as_mut() {
            widget.request_focus();
            return;
        }
        self.text_search = Some(SearchWidget::new(SEARCH_WIDGET_ID));
    }

    pub(in crate::screens::repository) fn close_text_search(&mut self) {
        self.text_search = None;
    }

    /// Which canvas the search bar should render over. Prefers where the
    /// cursor is now; falls back to the last-clicked canvas, then to a sane
    /// default for the active view. `None` only when no diff/conflict view
    /// is visible.
    pub(in crate::screens::repository) fn current_search_canvas(&self) -> Option<DiffCanvasId> {
        if !self.is_active() {
            return None;
        }
        if let Some(id) = self.hovered_canvas {
            return Some(id);
        }
        if let Some((id, _)) = self.diff_selection {
            return Some(id);
        }
        if self.conflict_file_resolution.is_some() {
            Some(crate::widgets::conflict_canvas::CANVAS_ID_OUTPUT)
        } else {
            Some(crate::widgets::diff_canvas::CANVAS_ID)
        }
    }

    /// Cursor entered a buffer. Updates the hover tracker and, if a search
    /// is already open, silently recomputes matches against the new buffer's
    /// content so the counter reflects the visible bar — intentionally does
    /// NOT scroll or re-select, so moving the mouse never jumps the viewport.
    pub(in crate::screens::repository) fn on_canvas_hover_entered(
        &mut self,
        canvas_id: DiffCanvasId,
    ) {
        if self.hovered_canvas == Some(canvas_id) {
            return;
        }
        self.hovered_canvas = Some(canvas_id);
        if self.text_search.is_some() {
            self.recompute_text_search_matches_for(canvas_id);
        }
    }

    /// Build a `TextCanvasData` snapshot for the canvas currently hosting a
    /// text search — used by the search driver to run `find_text_matches` and
    /// to compute scroll targets. Returns `None` if the buffer isn't loaded
    /// or the requested canvas isn't part of the active view.
    fn build_canvas_data_for(&self, canvas_id: DiffCanvasId) -> Option<Arc<TextCanvasData>> {
        use crate::widgets::diff_canvas as dc;
        if canvas_id == dc::CANVAS_ID {
            self.dirty_file_diff
                .as_ref()
                .and_then(|s| s.render_data.clone())
                .or_else(|| {
                    self.commit_file_diff
                        .as_ref()
                        .and_then(|s| s.render_data.clone())
                })
                .or_else(|| {
                    self.merged_file_diff
                        .as_ref()
                        .and_then(|s| s.render_data.clone())
                })
        } else {
            let state = self.conflict_file_resolution.as_ref()?;
            let result = state.result.as_ref()?;
            super::view::build_conflict_rows_for_canvas(
                canvas_id,
                result,
                &state.selections,
                state.ours_highlighted.as_deref(),
                state.theirs_highlighted.as_deref(),
            )
        }
    }

    /// Recompute matches against the buffer currently under the shared
    /// search bar. Does not emit a scroll task — caller chains
    /// `apply_current_text_match` when the new current match should be
    /// selected and scrolled to.
    fn recompute_text_search_matches_for(&mut self, canvas_id: DiffCanvasId) {
        let Some(widget) = self.text_search.as_mut() else {
            return;
        };
        let query = widget.query().to_string();
        let Some(data) = self.build_canvas_data_for(canvas_id) else {
            if let Some(w) = self.text_search.as_mut() {
                w.clear_matches();
            }
            return;
        };
        let matches = sw::find_text_matches(&data, &query);

        let anchor_row = self.text_search_anchor_row(canvas_id, &data);
        let initial = first_match_from_anchor(&matches, anchor_row);

        if let Some(w) = self.text_search.as_mut() {
            w.set_matches(matches, initial);
        }
    }

    /// Top-visible row on the target canvas — used as the anchor for "find
    /// next going forward from what I'm looking at". Falls back to 0 when
    /// the caller hasn't scrolled or the canvas has no line-height data.
    fn text_search_anchor_row(&self, canvas_id: DiffCanvasId, data: &TextCanvasData) -> usize {
        use crate::widgets::conflict_canvas::{CANVAS_ID_OURS, CANVAS_ID_OUTPUT, CANVAS_ID_THEIRS};
        let scroll_y = if canvas_id == CANVAS_ID_OURS {
            self.conflict_file_resolution
                .as_ref()
                .map(|s| s.ours_scroll_offset_y)
                .unwrap_or(0.0)
        } else if canvas_id == CANVAS_ID_THEIRS {
            self.conflict_file_resolution
                .as_ref()
                .map(|s| s.theirs_scroll_offset_y)
                .unwrap_or(0.0)
        } else if canvas_id == CANVAS_ID_OUTPUT {
            self.conflict_file_resolution
                .as_ref()
                .map(|s| s.output_scroll_offset_y)
                .unwrap_or(0.0)
        } else {
            self.diff_scroll_y
        };
        data.first_row_at_or_below(scroll_y)
    }

    /// Apply the current match as a text selection on the canvas the search
    /// bar is currently over and emit a scroll task that puts the match row
    /// near the top. No-op when there's no current match or no target canvas.
    fn apply_current_text_match(&mut self) -> Task<Message> {
        use crate::widgets::text::{SelectionMode, TextPosition, TextSelection};
        let Some(widget) = self.text_search.as_ref() else {
            return Task::none();
        };
        let Some(canvas_id) = self.current_search_canvas() else {
            return Task::none();
        };
        let Some(m) = widget.current_match() else {
            self.diff_selection = None;
            return Task::none();
        };
        let sel = TextSelection {
            anchor: TextPosition {
                row: m.row,
                col: m.col_start,
            },
            head: TextPosition {
                row: m.row,
                col: m.col_end,
            },
            dragging: false,
            mode: SelectionMode::Char,
            anchor_word: None,
        };
        self.diff_selection = Some((canvas_id, sel));
        self.scroll_canvas_to_match_row(canvas_id, m.row)
    }

    fn scroll_canvas_to_match_row(&self, canvas_id: DiffCanvasId, row: usize) -> Task<Message> {
        let Some(data) = self.build_canvas_data_for(canvas_id) else {
            return Task::none();
        };
        let y = data.row_top_y(row);
        let target_y =
            (y - crate::widgets::diff_canvas::search_scroll_breadcrumb_offset()).max(0.0);
        let current_x = match canvas_id {
            _ if canvas_id == crate::widgets::diff_canvas::CANVAS_ID => self.diff_scroll_x,
            _ => 0.0,
        };
        iced::widget::operation::scroll_to(
            scroll_id_for_canvas(canvas_id),
            iced::widget::scrollable::AbsoluteOffset {
                x: current_x,
                y: target_y,
            },
        )
    }

    /// Handle a `SearchWidgetMessage` targeting the active text search.
    /// `shift_held` is needed for `Submit`: plain Enter advances, Shift+Enter
    /// steps back. No-op when no search is open.
    pub(in crate::screens::repository) fn handle_text_search(
        &mut self,
        msg: SearchWidgetMessage,
        shift_held: bool,
    ) -> Task<Message> {
        match msg {
            SearchWidgetMessage::Close => {
                self.close_text_search();
                Task::none()
            }
            SearchWidgetMessage::BarClicked => {
                if let Some(w) = self.text_search.as_mut() {
                    w.request_focus();
                }
                Task::none()
            }
            SearchWidgetMessage::InputChanged(query) => {
                if let Some(w) = self.text_search.as_mut() {
                    w.set_query(query);
                }
                let Some(canvas_id) = self.current_search_canvas() else {
                    return Task::none();
                };
                self.recompute_text_search_matches_for(canvas_id);
                self.apply_current_text_match()
            }
            SearchWidgetMessage::Submit => {
                // First submit after a hover change has empty matches for the
                // new buffer (hover only silently updates). Compute them now so
                // Enter jumps to the first hit instead of silently doing
                // nothing.
                if self
                    .text_search
                    .as_ref()
                    .is_some_and(|w| w.matches().is_empty())
                {
                    if let Some(canvas_id) = self.current_search_canvas() {
                        self.recompute_text_search_matches_for(canvas_id);
                    }
                }
                let stepped = if shift_held {
                    self.text_search.as_mut().and_then(|w| w.go_previous())
                } else {
                    self.text_search.as_mut().and_then(|w| w.go_next())
                };
                if stepped.is_some() {
                    self.apply_current_text_match()
                } else {
                    Task::none()
                }
            }
            SearchWidgetMessage::Next => {
                if self
                    .text_search
                    .as_mut()
                    .and_then(|w| w.go_next())
                    .is_some()
                {
                    self.apply_current_text_match()
                } else {
                    Task::none()
                }
            }
            SearchWidgetMessage::Previous => {
                if self
                    .text_search
                    .as_mut()
                    .and_then(|w| w.go_previous())
                    .is_some()
                {
                    self.apply_current_text_match()
                } else {
                    Task::none()
                }
            }
        }
    }

    /// After a file load or highlight refresh, re-run the matcher against the
    /// new buffer contents so match indices line up with the rendered rows.
    pub(in crate::screens::repository) fn refresh_text_search_after_buffer_change(&mut self) {
        if self.text_search.is_none() {
            return;
        }
        let Some(canvas_id) = self.current_search_canvas() else {
            return;
        };
        self.recompute_text_search_matches_for(canvas_id);
    }

    /// Build the search-bar element for the single-buffer diff view. Lives
    /// inside the diff body's `Stack` overlay — no positioning logic needed
    /// since there's only one buffer.
    pub(in crate::screens::repository) fn diff_view_search_bar<'a>(
        &'a self,
    ) -> Option<Element<'a, Message>> {
        let widget = self.text_search.as_ref()?;
        Some(sw::search_bar_view(
            SEARCH_WIDGET_ID,
            widget.query(),
            widget.current_match_index(),
            widget.matches().len(),
            |msg| {
                Message::repo(super::super::super::RepositoryMessage::DiffPanel(
                    DiffPanelAction::TextSearch(msg),
                ))
            },
        ))
    }

    /// Build a single search overlay for the conflict view. The returned
    /// element carries a `responsive` child that positions the bar above
    /// the hovered buffer by computing pixel padding from the container
    /// size + known buffer ratios (top-half = 2/3 height, split 50/50
    /// horizontally; bottom-half = 1/3 height).
    ///
    /// Using one overlay at a stable tree position is deliberate: iced
    /// preserves `text_input` widget state (caret position, selection)
    /// across re-renders only when the widget keeps the same tree path.
    /// Earlier designs rendered a bar *inside* each buffer's Stack, which
    /// reset the caret every time the hover target changed.
    pub(in crate::screens::repository) fn conflict_view_search_overlay<'a>(
        &'a self,
    ) -> Option<Element<'a, Message>> {
        let widget = self.text_search.as_ref()?;
        let query = widget.query().to_string();
        let current_idx = widget.current_match_index();
        let total = widget.matches().len();
        let hovered = self.current_search_canvas();

        Some(
            iced::widget::responsive(move |size| {
                use crate::widgets::conflict_canvas::{CANVAS_ID_OURS, CANVAS_ID_OUTPUT};
                use iced::{alignment, Length, Padding};

                let pad = if hovered == Some(CANVAS_ID_OURS) {
                    Padding {
                        top: 5.0,
                        right: size.width / 2.0 + 5.0,
                        bottom: 0.0,
                        left: 0.0,
                    }
                } else if hovered == Some(CANVAS_ID_OUTPUT) {
                    Padding {
                        top: size.height * 2.0 / 3.0 + 5.0,
                        right: 5.0,
                        bottom: 0.0,
                        left: 0.0,
                    }
                } else {
                    Padding {
                        top: 5.0,
                        right: 5.0,
                        bottom: 0.0,
                        left: 0.0,
                    }
                };

                let bar = sw::search_bar_view(
                    SEARCH_WIDGET_ID,
                    query.as_str(),
                    current_idx,
                    total,
                    |msg| {
                        Message::repo(super::super::super::RepositoryMessage::DiffPanel(
                            DiffPanelAction::TextSearch(msg),
                        ))
                    },
                );

                iced::widget::container(bar)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(alignment::Horizontal::Right)
                    .align_y(alignment::Vertical::Top)
                    .padding(pad)
                    .into()
            })
            .into(),
        )
    }
}

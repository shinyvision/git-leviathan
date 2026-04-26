//! Diff panel — the center view swaps between the graph and four
//! diff "modes": dirty (working-tree), commit (single-commit file diff),
//! merged (multi-commit file diff), and conflict (three-buffer resolver).
//!
//! The `DiffPanel` struct owns state for all four modes simultaneously
//! (most fields are `Option<ModeState>` and only one is populated at a
//! time), plus the selection state machine + shared text search that work
//! uniformly across every buffer.
//!
//! Each mode file (`dirty`/`commit`/`merged`/`conflict`) holds its own
//! state type and lifecycle helpers; `search`/`selection` host cross-mode
//! behaviour; `view` renders. Generic diff-load handling is expressed via
//! the `DiffLoadMode` trait and the `on_diff_loaded` free function — the
//! three previously-identical `on_*_diff_loaded` paths now share one body.

use std::sync::Arc;

use iced::Element;

use crate::{
    message::Message,
    services::{HighlightedFile, WorkingTreeDiffLine, WorkingTreeDiffResult},
    widgets::{
        diff_canvas::{DiffCanvasData, DiffCanvasId, DiffPosition},
        search_widget::{SearchWidget, TextMatch},
    },
};

use super::super::panel_messages::DiffPanelAction;

mod auto_scroll;
mod commit;
mod conflict;
mod dirty;
mod merged;
mod search;
mod selection;
mod update;
mod view;

pub(in crate::screens::repository) use auto_scroll::tick as auto_scroll_tick;
pub(in crate::screens::repository) use update::update as update_diff;

pub(in crate::screens::repository) use commit::CommitFileDiffState;
pub(in crate::screens::repository) use conflict::{
    conflict_resolution_output_text, ConflictFileResolutionState, ConflictScrollTarget,
    ConflictSide,
};
pub(in crate::screens::repository) use dirty::DirtyFileDiffState;
pub(in crate::screens::repository) use merged::MergedFileDiffState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CenterViewMode {
    Graph,
    DiffView,
}

pub(in crate::screens::repository) struct DiffPanel {
    pub center_view_mode: CenterViewMode,
    pub dirty_file_diff: Option<DirtyFileDiffState>,
    pub conflict_file_resolution: Option<ConflictFileResolutionState>,
    pub commit_file_diff: Option<CommitFileDiffState>,
    pub merged_file_diff: Option<MergedFileDiffState>,
    /// Active text selection plus the canvas it belongs to. Cleared on
    /// file switch or when a new selection begins in a different canvas.
    pub diff_selection: Option<(DiffCanvasId, crate::widgets::diff_canvas::DiffSelection)>,
    /// Vertical scroll offset of the diff content scrollable, mirrored into
    /// the sticky gutter canvas so it scrolls in sync. Reset on file switch.
    pub diff_scroll_y: f32,
    /// Current x scroll offset of the diff content scrollable. Tracked so we
    /// can issue relative horizontal `scroll_to` commands for shift+wheel.
    pub diff_scroll_x: f32,
    /// Scrollable viewport rect (window coords) captured on drag start. Used
    /// with the window-level cursor position to compute off-canvas distance
    /// for auto-scroll — works even when the cursor leaves the widget, which
    /// suppresses in-widget CursorMoved events.
    pub diff_viewport_rect: Option<iced::Rectangle>,
    /// Canvas data captured at drag start. Lets the auto-scroll tick
    /// re-hit-test the cursor against updated scroll offsets to extend the
    /// selection while the viewport scrolls underneath a held cursor.
    pub diff_drag_canvas_data: Option<Arc<DiffCanvasData>>,
    /// Time + position of the last selection-begin press. Used to detect a
    /// double-click (same position, < DOUBLE_CLICK_MS apart) and promote the
    /// new drag to word-select mode.
    pub diff_last_click: Option<(std::time::Instant, DiffCanvasId, DiffPosition)>,
    /// Whether the Shift modifier is currently held. View uses this to wrap
    /// the diff scrollable in a wheel-intercepting `mouse_area` so shift+wheel
    /// turns into horizontal-only scroll.
    pub shift_held: bool,
    /// Shared text-buffer search. A single widget state is re-rendered in the
    /// buffer currently under the mouse cursor (`hovered_canvas`); moving the
    /// cursor between conflict buffers moves the bar without resetting focus
    /// or query. One widget instance serves all three conflict panes and the
    /// regular diff view.
    pub text_search: Option<SearchWidget<TextMatch>>,
    /// Canvas the mouse cursor is currently inside. Drives where the shared
    /// search bar renders and which buffer's content matches are computed
    /// against. `None` before the first `on_enter` fires on any buffer.
    pub hovered_canvas: Option<DiffCanvasId>,
}

impl DiffPanel {
    pub(in crate::screens::repository) fn new() -> Self {
        Self {
            center_view_mode: CenterViewMode::Graph,
            dirty_file_diff: None,
            conflict_file_resolution: None,
            commit_file_diff: None,
            merged_file_diff: None,
            diff_selection: None,
            diff_scroll_y: 0.0,
            diff_scroll_x: 0.0,
            diff_viewport_rect: None,
            diff_drag_canvas_data: None,
            diff_last_click: None,
            shift_held: false,
            text_search: None,
            hovered_canvas: None,
        }
    }

    pub(in crate::screens::repository) fn is_active(&self) -> bool {
        self.center_view_mode == CenterViewMode::DiffView
    }

    pub(in crate::screens::repository) fn active_diff_file_path(&self) -> Option<&str> {
        self.dirty_file_diff
            .as_ref()
            .map(|d| d.file_path.as_str())
            .or_else(|| {
                self.conflict_file_resolution
                    .as_ref()
                    .map(|d| d.file_path.as_str())
            })
            .or_else(|| self.commit_file_diff.as_ref().map(|d| d.file_path.as_str()))
            .or_else(|| self.merged_file_diff.as_ref().map(|d| d.file_path.as_str()))
    }

    pub(in crate::screens::repository) fn close(&mut self) {
        self.center_view_mode = CenterViewMode::Graph;
        self.dirty_file_diff = None;
        self.conflict_file_resolution = None;
        self.commit_file_diff = None;
        self.merged_file_diff = None;
        self.diff_selection = None;
        self.diff_scroll_y = 0.0;
        self.diff_scroll_x = 0.0;
        self.diff_viewport_rect = None;
        self.diff_drag_canvas_data = None;
        self.diff_last_click = None;
        self.hovered_canvas = None;
        self.text_search = None;
        crate::services::release_syntax_caches();
        crate::services::release_text_caches();
    }

    pub(in crate::screens::repository) fn navigate_file(
        &mut self,
        delta: i32,
        commits: &[crate::core::Commit],
        commit_diff_states: &[crate::view_model::CommitDiffState],
        merged_files: Option<&[crate::core::ChangedFile]>,
    ) -> Option<DiffPanelAction> {
        if self.dirty_file_diff.is_some() || self.conflict_file_resolution.is_some() {
            let dirty_commit = commits.first()?;
            if dirty_commit.kind != crate::core::CommitKind::Dirty {
                return None;
            }

            let all_files: Vec<_> = dirty_commit
                .conflicted_files
                .iter()
                .map(|f| (f.path.clone(), false))
                .chain(
                    dirty_commit
                        .unstaged_files
                        .iter()
                        .map(|f| (f.path.clone(), false)),
                )
                .chain(
                    dirty_commit
                        .staged_files
                        .iter()
                        .map(|f| (f.path.clone(), true)),
                )
                .collect();

            if all_files.is_empty() {
                return None;
            }

            let current_idx = self
                .dirty_file_diff
                .as_ref()
                .map(|state| state.selected_file_idx)
                .or_else(|| {
                    self.conflict_file_resolution
                        .as_ref()
                        .map(|state| state.selected_file_idx)
                })
                .unwrap_or(0);
            let file_count = all_files.len();
            let new_idx = if delta < 0 {
                (current_idx + file_count - delta.unsigned_abs() as usize) % file_count
            } else {
                (current_idx + delta as usize) % file_count
            };

            let (new_path, new_is_staged) = &all_files[new_idx];
            let is_conflicted = dirty_commit
                .conflicted_files
                .iter()
                .any(|f| f.path == *new_path);

            let action =
                self.open_dirty_file_view(new_path.clone(), *new_is_staged, new_idx, is_conflicted);
            return Some(action);
        } else if let Some(diff_state) = self.commit_file_diff.as_ref() {
            let commit_idx = diff_state.commit_idx;
            let _commit = commits.get(commit_idx)?;
            let commit_diff = commit_diff_states.get(commit_idx)?;

            if commit_diff.files.is_empty() {
                return None;
            }

            let current_idx = diff_state.selected_file_idx;
            let commit_hash = diff_state.commit_hash.clone();
            let file_count = commit_diff.files.len();
            let new_idx = if delta < 0 {
                (current_idx + file_count - delta.unsigned_abs() as usize) % file_count
            } else {
                (current_idx + delta as usize) % file_count
            };

            let new_path = commit_diff.files[new_idx].path.clone();
            let action = self.open_commit_file(commit_idx, commit_hash, new_path, new_idx);
            return Some(action);
        } else if let Some(diff_state) = self.merged_file_diff.as_ref() {
            let files = merged_files?;
            if files.is_empty() {
                return None;
            }

            let current_idx = diff_state.selected_file_idx;
            let hashes = diff_state.hashes.clone();
            let file_count = files.len();
            let new_idx = if delta < 0 {
                (current_idx + file_count - delta.unsigned_abs() as usize) % file_count
            } else {
                (current_idx + delta as usize) % file_count
            };

            let new_path = files[new_idx].path.clone();
            let action = self.open_merged_file(hashes, new_path, new_idx);
            return Some(action);
        }

        None
    }

    pub(in crate::screens::repository) fn view_or_passthrough<'a>(
        &'a self,
        graph_view: Element<'a, Message>,
    ) -> Element<'a, Message> {
        match self.center_view_mode {
            CenterViewMode::Graph => graph_view,
            CenterViewMode::DiffView => {
                if let Some(conflict_state) = &self.conflict_file_resolution {
                    use crate::widgets::conflict_canvas::{
                        CANVAS_ID_OURS, CANVAS_ID_OUTPUT, CANVAS_ID_THEIRS,
                    };
                    let model = conflict::ConflictResolverViewModel {
                        file_path: &conflict_state.file_path,
                        result: conflict_state.result.as_ref(),
                        selections: &conflict_state.selections,
                        ours_highlighted: conflict_state.ours_highlighted.clone(),
                        theirs_highlighted: conflict_state.theirs_highlighted.clone(),
                        ours_scroll_offset_y: conflict_state.ours_scroll_offset_y,
                        theirs_scroll_offset_y: conflict_state.theirs_scroll_offset_y,
                        output_scroll_offset_y: conflict_state.output_scroll_offset_y,
                        ours_selection: self.selection_for(CANVAS_ID_OURS),
                        theirs_selection: self.selection_for(CANVAS_ID_THEIRS),
                        output_selection: self.selection_for(CANVAS_ID_OUTPUT),
                        shift_held: self.shift_held,
                        search_overlay: self.conflict_view_search_overlay(),
                    };
                    view::conflict_center_view(model)
                } else if let Some(state) = self.active_single_file_diff() {
                    view::diff_center_view(self.build_single_file_view_model(state))
                } else {
                    graph_view
                }
            }
        }
    }

    fn active_single_file_diff(&self) -> Option<&dyn SingleFileDiffView> {
        if let Some(s) = &self.dirty_file_diff {
            return Some(s);
        }
        if let Some(s) = &self.commit_file_diff {
            return Some(s);
        }
        if let Some(s) = &self.merged_file_diff {
            return Some(s);
        }
        None
    }

    fn build_single_file_view_model<'a>(
        &'a self,
        state: &'a dyn SingleFileDiffView,
    ) -> view::DiffViewModel<'a> {
        view::DiffViewModel {
            file_path: state.file_path(),
            diff_lines: state.diff_lines(),
            old_highlighted: state.old_highlighted(),
            new_highlighted: state.new_highlighted(),
            selection: self.selection_for(crate::widgets::diff_canvas::CANVAS_ID),
            scroll_y: self.diff_scroll_y,
            shift_held: self.shift_held,
            search_bar: self.diff_view_search_bar(),
        }
    }
}

/// View-layer projection for the three single-file diff states. Lets
/// `DiffPanel::build_single_file_view_model` assemble a `DiffViewModel`
/// without caring whether the active state is dirty/commit/merged — only
/// the field source differs, the projection shape does not. Impls live in
/// each mode file.
pub(super) trait SingleFileDiffView {
    fn file_path(&self) -> &str;
    fn diff_lines(&self) -> &[WorkingTreeDiffLine];
    fn old_highlighted(&self) -> Option<&HighlightedFile>;
    fn new_highlighted(&self) -> Option<&HighlightedFile>;
}

/// Per-mode hook into the generic diff-load handler. Each of the three
/// single-file diff state types (`DirtyFileDiffState`, `CommitFileDiffState`,
/// `MergedFileDiffState`) implements this trait so `on_diff_loaded` can
/// validate the incoming `WorkingTreeDiffResult` and schedule the matching
/// syntax-highlight task without the caller caring which mode is active.
pub(in crate::screens::repository) trait DiffLoadMode {
    fn file_path(&self) -> &str;
    fn set_lines(&mut self, lines: Vec<WorkingTreeDiffLine>);
    fn clear_highlights(&mut self);
    fn highlight_action(
        &self,
        file_path: String,
        old_content: Option<String>,
        new_content: Option<String>,
    ) -> DiffPanelAction;
}

/// Generic "diff loaded for a single file" handler. Validates that the
/// result's file path still matches the mode's active file (so a stale
/// response for a file the user already navigated away from is dropped),
/// stores the diff lines, clears the old highlight (forcing a re-highlight),
/// and returns the per-mode highlight action the caller should dispatch.
pub(in crate::screens::repository) fn on_diff_loaded<S: DiffLoadMode>(
    slot: &mut Option<S>,
    result: Result<WorkingTreeDiffResult, crate::services::GitError>,
) -> Option<DiffPanelAction> {
    let Ok(diff_result) = result else {
        return None;
    };
    let state = slot.as_mut()?;
    if state.file_path() != diff_result.file_path {
        return None;
    }
    let WorkingTreeDiffResult {
        file_path,
        lines,
        old_file_content,
        new_file_content,
        ..
    } = diff_result;
    state.set_lines(lines);
    state.clear_highlights();
    Some(state.highlight_action(file_path, old_file_content, new_file_content))
}

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
    services::{
        syntax_highlight::{installation::GrammarInstallStatus, SyntaxGrammarStatus},
        DiffFallbacks, HighlightDocument, WorkingTreeDiffLine, WorkingTreeDiffResult,
    },
    widgets::{
        diff_canvas::{
            CachedDiffHighlightProvider, DiffCanvasData, DiffCanvasId, DiffHighlightSide,
            DiffPosition,
        },
        search_widget::{SearchWidget, TextMatch},
    },
};

use super::super::panel_messages::DiffPanelAction;

mod auto_scroll;
mod commit;
mod conflict;
mod dirty;
mod highlight_schedule;
mod media;
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
pub(in crate::screens::repository) use dirty::{DirtyDiffSyncResult, DirtyFileDiffState};
pub(in crate::screens::repository) use media::{
    AbsentReason, CompareMode, DifferenceState, MediaAction, MediaDiffState,
    MediaSideState, TransportCommand, SEEK_STEP_SECS,
};
pub(in crate::screens::repository) use merged::MergedFileDiffState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CenterViewMode {
    Graph,
    DiffView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SingleFileDiffKind {
    Dirty { is_staged: bool },
    Commit { commit_hash: String },
    Merged,
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
    pub diff_viewport_height: Option<f32>,
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
    highlight_scheduler: highlight_schedule::DiffHighlightScheduler,
    next_generation: u64,
    grammar_status_cache: std::cell::RefCell<
        Option<(
            highlight_schedule::DiffHighlightDocumentIds,
            Option<DiffGrammarStatusView>,
        )>,
    >,
    highlight_in_flight: bool,
    highlight_rescan_requested: bool,
    /// Media (image/audio/video) viewer state for the active file. Present
    /// alongside the mode state that tracks which file is open.
    pub media: Option<MediaDiffState>,
    /// File the user explicitly asked to see as text instead of media.
    force_text_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::screens::repository) struct DiffGrammarStatusView {
    pub label: String,
    pub action: Option<DiffGrammarStatusAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::screens::repository) struct DiffGrammarStatusAction {
    pub command_id: &'static str,
    pub label: &'static str,
    pub language: String,
}

// Separate from DiffPanelAction because the provider isn't Debug; all fields are Send.
pub(in crate::screens::repository) struct VisibleHighlightJob {
    pub generation: u64,
    pub requests: Vec<highlight_schedule::DiffLineHighlightRequest>,
    pub document_ids: highlight_schedule::DiffHighlightDocumentIds,
    pub old_document: Option<HighlightDocument>,
    pub new_document: Option<HighlightDocument>,
    pub provider: Arc<CachedDiffHighlightProvider>,
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
            diff_viewport_height: None,
            diff_viewport_rect: None,
            diff_drag_canvas_data: None,
            diff_last_click: None,
            shift_held: false,
            text_search: None,
            hovered_canvas: None,
            highlight_scheduler: highlight_schedule::DiffHighlightScheduler::default(),
            next_generation: 1,
            grammar_status_cache: std::cell::RefCell::new(None),
            highlight_in_flight: false,
            highlight_rescan_requested: false,
            media: None,
            force_text_path: None,
        }
    }

    pub(in crate::screens::repository) fn is_media_active(&self) -> bool {
        self.is_active() && self.media.is_some()
    }

    fn next_diff_generation(&mut self) -> u64 {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        generation
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
        self.diff_viewport_height = None;
        self.diff_viewport_rect = None;
        self.diff_drag_canvas_data = None;
        self.diff_last_click = None;
        self.hovered_canvas = None;
        self.text_search = None;
        self.highlight_scheduler.reset();
        self.highlight_in_flight = false;
        self.highlight_rescan_requested = false;
        // Dropping the media state stops playback and kills FFmpeg decoders.
        self.media = None;
        self.force_text_path = None;
        self.invalidate_grammar_status_cache();
        // Deliberately NOT releasing the process-wide syntax/text caches here:
        // they're bounded LRUs shared by every open tab, so wiping them when
        // one diff panel closes forces all other tabs to re-parse. They evict
        // naturally under pressure.
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
        git_write_busy: bool,
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
                        ours_canvas: conflict_state.ours_canvas.clone(),
                        theirs_canvas: conflict_state.theirs_canvas.clone(),
                        output_canvas: conflict_state.output_canvas.clone(),
                        ours_scroll_offset_y: conflict_state.ours_scroll_offset_y,
                        theirs_scroll_offset_y: conflict_state.theirs_scroll_offset_y,
                        output_scroll_offset_y: conflict_state.output_scroll_offset_y,
                        ours_selection: self.selection_for(CANVAS_ID_OURS),
                        theirs_selection: self.selection_for(CANVAS_ID_THEIRS),
                        output_selection: self.selection_for(CANVAS_ID_OUTPUT),
                        shift_held: self.shift_held,
                        search_overlay: self.conflict_view_search_overlay(),
                        save_busy: git_write_busy,
                    };
                    view::conflict_center_view(model)
                } else if let Some(media) = &self.media {
                    view::media_center_view(view::MediaViewModel {
                        file_path: &media.file_path,
                        state: media,
                    })
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
            render_data: state.render_data(),
            selection: self.selection_for(crate::widgets::diff_canvas::CANVAS_ID),
            scroll_y: self.diff_scroll_y,
            shift_held: self.shift_held,
            search_bar: self.diff_view_search_bar(),
            grammar_status: self.active_grammar_status_view(),
        }
    }

    pub(in crate::screens::repository) fn on_single_file_render_ready(
        &mut self,
        kind: SingleFileDiffKind,
        generation: u64,
        file_path: String,
        data: Arc<DiffCanvasData>,
    ) -> bool {
        let mut should_schedule = false;
        match kind {
            SingleFileDiffKind::Dirty { is_staged } => {
                if let Some(state) = self.dirty_file_diff.as_mut() {
                    if state.file_path == file_path
                        && state.is_staged == is_staged
                        && state.render_generation == generation
                    {
                        state.render_data = Some(data);
                        should_schedule = true;
                    }
                }
            }
            SingleFileDiffKind::Commit { commit_hash } => {
                if let Some(state) = self.commit_file_diff.as_mut() {
                    if state.file_path == file_path
                        && state.commit_hash == commit_hash
                        && state.render_generation == generation
                    {
                        state.render_data = Some(data);
                        should_schedule = true;
                    }
                }
            }
            SingleFileDiffKind::Merged => {
                if let Some(state) = self.merged_file_diff.as_mut() {
                    if state.file_path == file_path && state.render_generation == generation {
                        state.render_data = Some(data);
                        should_schedule = true;
                    }
                }
            }
        }
        if should_schedule {
            self.highlight_scheduler.reset();
            self.highlight_in_flight = false;
            self.highlight_rescan_requested = false;
        }
        should_schedule
    }

    pub(in crate::screens::repository) fn request_visible_highlight(
        &mut self,
    ) -> Option<VisibleHighlightJob> {
        if self.highlight_in_flight {
            self.highlight_rescan_requested = true;
            return None;
        }
        let job = self.schedule_visible_highlights()?;
        self.highlight_in_flight = true;
        self.highlight_rescan_requested = false;
        Some(job)
    }

    pub(in crate::screens::repository) fn finish_visible_highlight(
        &mut self,
        highlighted: usize,
    ) -> bool {
        self.highlight_in_flight = false;
        highlighted > 0 || std::mem::take(&mut self.highlight_rescan_requested)
    }

    pub(in crate::screens::repository) fn schedule_visible_highlights(
        &mut self,
    ) -> Option<VisibleHighlightJob> {
        let (generation, data) = self.active_single_file_render_data()?;
        let (old_document, new_document, provider) =
            self.active_highlight_documents_and_provider(generation)?;
        let old_document = old_document.cloned();
        let new_document = new_document.cloned();
        let document_ids = highlight_schedule::DiffHighlightDocumentIds::from_documents(
            old_document.as_ref(),
            new_document.as_ref(),
        );
        let viewport_height = self
            .diff_viewport_height
            .unwrap_or(highlight_schedule::INITIAL_HIGHLIGHT_VIEWPORT_HEIGHT);
        let requests = self
            .highlight_scheduler
            .schedule(
                generation,
                document_ids,
                data.as_ref(),
                provider.as_ref(),
                self.diff_scroll_y,
                viewport_height,
            )
            .to_vec();
        if requests.is_empty() {
            return None;
        }
        let schedule_stats = self.highlight_scheduler.last_stats();
        let first = requests[0];
        let first_side = match first.reference.side {
            DiffHighlightSide::Old => "old",
            DiffHighlightSide::New => "new",
        };
        crate::perf::Span::new("ui.diff_highlight_schedule")
            .field("generation", generation)
            .field("request_generation", first.generation)
            .field("first_side", first_side)
            .field("first_line", first.reference.line_number)
            .field("scroll_y", self.diff_scroll_y)
            .field("viewport_height", viewport_height)
            .field("visible_rows", schedule_stats.visible_rows)
            .field("candidate_rows", schedule_stats.candidate_rows)
            .field("overscan_rows", schedule_stats.overscan_rows)
            .field("request_budget", schedule_stats.request_budget)
            .finish_with("requests", requests.len());
        Some(VisibleHighlightJob {
            generation,
            requests,
            document_ids,
            old_document,
            new_document,
            provider,
        })
    }

    pub(in crate::screens::repository) fn on_syntax_grammar_assets_changed(
        &mut self,
    ) -> Option<DiffPanelAction> {
        self.highlight_scheduler.reset();
        self.invalidate_grammar_status_cache();
        if self.dirty_file_diff.is_some() {
            let generation = self.next_diff_generation();
            return rebuild_single_file_after_grammar_change(
                self.dirty_file_diff.as_mut()?,
                generation,
            );
        }
        if self.commit_file_diff.is_some() {
            let generation = self.next_diff_generation();
            return rebuild_single_file_after_grammar_change(
                self.commit_file_diff.as_mut()?,
                generation,
            );
        }
        if self.merged_file_diff.is_some() {
            let generation = self.next_diff_generation();
            return rebuild_single_file_after_grammar_change(
                self.merged_file_diff.as_mut()?,
                generation,
            );
        }
        if self.conflict_file_resolution.is_some() {
            let generation = self.next_diff_generation();
            if let Some(state) = self.conflict_file_resolution.as_mut() {
                state.render_generation = generation;
                state.ours_highlighted = None;
                state.theirs_highlighted = None;
                state.rebuild_canvases();
                let result = state.result.as_ref()?;
                let file_path = state.file_path.clone();
                let ours_content =
                    conflict::conflict_side_highlight_content(result, ConflictSide::Ours);
                let theirs_content =
                    conflict::conflict_side_highlight_content(result, ConflictSide::Theirs);
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

    fn active_single_file_render_data(&self) -> Option<(u64, Arc<DiffCanvasData>)> {
        if let Some(state) = &self.dirty_file_diff {
            return state
                .render_data
                .clone()
                .map(|data| (state.render_generation, data));
        }
        if let Some(state) = &self.commit_file_diff {
            return state
                .render_data
                .clone()
                .map(|data| (state.render_generation, data));
        }
        if let Some(state) = &self.merged_file_diff {
            return state
                .render_data
                .clone()
                .map(|data| (state.render_generation, data));
        }
        None
    }

    fn active_highlight_documents_and_provider(
        &self,
        generation: u64,
    ) -> Option<(
        Option<&HighlightDocument>,
        Option<&HighlightDocument>,
        Arc<CachedDiffHighlightProvider>,
    )> {
        if let Some(state) = &self.dirty_file_diff {
            if state.render_generation == generation {
                return Some((
                    state.old_highlight_document.as_ref(),
                    state.new_highlight_document.as_ref(),
                    state.highlight_provider.clone(),
                ));
            }
        }
        if let Some(state) = &self.commit_file_diff {
            if state.render_generation == generation {
                return Some((
                    state.old_highlight_document.as_ref(),
                    state.new_highlight_document.as_ref(),
                    state.highlight_provider.clone(),
                ));
            }
        }
        if let Some(state) = &self.merged_file_diff {
            if state.render_generation == generation {
                return Some((
                    state.old_highlight_document.as_ref(),
                    state.new_highlight_document.as_ref(),
                    state.highlight_provider.clone(),
                ));
            }
        }
        None
    }

    fn active_grammar_status_view(&self) -> Option<DiffGrammarStatusView> {
        let (old, new) = self.active_single_file_documents()?;
        let ids = highlight_schedule::DiffHighlightDocumentIds::from_documents(old, new);
        if let Some((cached_ids, cached_view)) = self.grammar_status_cache.borrow().as_ref() {
            if *cached_ids == ids {
                return cached_view.clone();
            }
        }
        let statuses = [old, new]
            .into_iter()
            .flatten()
            .filter_map(grammar_status_for_document)
            .collect::<Vec<_>>();
        let view = grammar_status_view(statuses);
        *self.grammar_status_cache.borrow_mut() = Some((ids, view.clone()));
        view
    }

    pub(in crate::screens::repository) fn invalidate_grammar_status_cache(&self) {
        self.grammar_status_cache.borrow_mut().take();
    }

    fn active_single_file_documents(
        &self,
    ) -> Option<(Option<&HighlightDocument>, Option<&HighlightDocument>)> {
        if let Some(state) = &self.dirty_file_diff {
            return Some((
                state.old_highlight_document.as_ref(),
                state.new_highlight_document.as_ref(),
            ));
        }
        if let Some(state) = &self.commit_file_diff {
            return Some((
                state.old_highlight_document.as_ref(),
                state.new_highlight_document.as_ref(),
            ));
        }
        if let Some(state) = &self.merged_file_diff {
            return Some((
                state.old_highlight_document.as_ref(),
                state.new_highlight_document.as_ref(),
            ));
        }
        None
    }
}

/// View-layer projection for the three single-file diff states. Lets
/// `DiffPanel::build_single_file_view_model` assemble a `DiffViewModel`
/// without caring whether the active state is dirty/commit/merged — only
/// the field source differs, the projection shape does not. Impls live in
/// each mode file.
pub(super) trait SingleFileDiffView {
    fn file_path(&self) -> &str;
    fn render_data(&self) -> Option<Arc<DiffCanvasData>>;
}

/// Per-mode hook into the generic diff-load handler. Each of the three
/// single-file diff state types (`DirtyFileDiffState`, `CommitFileDiffState`,
/// `MergedFileDiffState`) implements this trait so `on_diff_loaded` can
/// validate the incoming `WorkingTreeDiffResult` and build render rows without
/// the caller caring which mode is active.
pub(in crate::screens::repository) trait DiffLoadMode {
    fn file_path(&self) -> &str;
    fn lines(&self) -> Option<Arc<Vec<WorkingTreeDiffLine>>>;
    fn set_lines(&mut self, lines: Arc<Vec<WorkingTreeDiffLine>>);
    fn generation(&self) -> u64;
    fn set_generation(&mut self, generation: u64);
    fn fallbacks(&self) -> DiffFallbacks;
    fn set_fallbacks(&mut self, fallbacks: DiffFallbacks);
    fn set_highlight_documents(
        &mut self,
        old: Option<HighlightDocument>,
        new: Option<HighlightDocument>,
    );
    fn reset_highlight_provider(&mut self);
    fn highlight_provider(&self) -> Arc<CachedDiffHighlightProvider>;
    fn render_kind(&self) -> SingleFileDiffKind;
}

fn rebuild_single_file_after_grammar_change<S: DiffLoadMode>(
    state: &mut S,
    generation: u64,
) -> Option<DiffPanelAction> {
    let lines = state.lines()?;
    let file_path = state.file_path().to_string();
    let fallbacks = state.fallbacks();
    state.reset_highlight_provider();
    let highlight_provider = state.highlight_provider();
    let kind = state.render_kind();
    state.set_generation(generation);
    Some(DiffPanelAction::RunSingleFileRenderBuild {
        generation,
        kind,
        file_path,
        lines,
        fallbacks,
        highlight_provider,
    })
}

fn grammar_status_for_document(document: &HighlightDocument) -> Option<DiffGrammarStatusView> {
    let status = crate::services::syntax_highlight::get_syntax_service()
        .syntax_status_for_document(document);
    let SyntaxGrammarStatus::Missing {
        language_id,
        syntax_key,
        registry_checked,
        installable,
        install_status,
    } = status
    else {
        return None;
    };

    let display_language = if language_id == "plain_text" {
        syntax_key
    } else {
        language_id
    };
    let label = grammar_status_label(
        &display_language,
        registry_checked,
        installable,
        install_status,
    );
    let action = grammar_status_action(&display_language, installable, install_status);
    Some(DiffGrammarStatusView { label, action })
}

fn grammar_status_label(
    language: &str,
    registry_checked: bool,
    installable: bool,
    install_status: GrammarInstallStatus,
) -> String {
    match install_status {
        GrammarInstallStatus::Queued => format!("{language} grammar queued"),
        GrammarInstallStatus::Installing => format!("{language} grammar installing"),
        GrammarInstallStatus::Failed => format!("{language} grammar failed"),
        _ if installable => format!("{language} grammar missing"),
        _ if registry_checked => format!("{language} grammar unavailable"),
        _ => format!("{language} grammar missing"),
    }
}

fn grammar_status_action(
    language: &str,
    installable: bool,
    install_status: GrammarInstallStatus,
) -> Option<DiffGrammarStatusAction> {
    if !installable {
        return None;
    }
    let (command_id, label) = match install_status {
        GrammarInstallStatus::Missing | GrammarInstallStatus::Queued => {
            ("syntax.grammar.install", "Install")
        }
        GrammarInstallStatus::Failed => ("syntax.grammar.update", "Retry"),
        _ => return None,
    };
    Some(DiffGrammarStatusAction {
        command_id,
        label,
        language: language.to_string(),
    })
}

fn grammar_status_view(mut statuses: Vec<DiffGrammarStatusView>) -> Option<DiffGrammarStatusView> {
    statuses.sort_by(|a, b| a.label.cmp(&b.label));
    statuses.dedup_by(|a, b| a.label == b.label && a.action == b.action);
    match statuses.len() {
        0 => None,
        1 => statuses.pop(),
        count => Some(DiffGrammarStatusView {
            label: format!("{count} grammars need attention"),
            action: None,
        }),
    }
}

/// Generic "diff loaded for a single file" handler. Validates that the
/// result's file path still matches the mode's active file (so a stale
/// response for a file the user already navigated away from is dropped),
/// stores the diff lines and returns the render-build action the caller should
/// dispatch. Syntax spans are materialized lazily from visible-line requests.
pub(in crate::screens::repository) fn on_diff_loaded<S: DiffLoadMode>(
    slot: &mut Option<S>,
    next_generation: &mut u64,
    generation: u64,
    result: Result<WorkingTreeDiffResult, crate::services::GitError>,
) -> Vec<DiffPanelAction> {
    let Ok(diff_result) = result else {
        return Vec::new();
    };
    let Some(state) = slot.as_mut() else {
        return Vec::new();
    };
    let span = crate::perf::Span::new("ui.diff_result_apply")
        .field("active_path", state.file_path())
        .field("result_path", &diff_result.file_path)
        .field("lines", diff_result.lines.len());
    if state.file_path() != diff_result.file_path || state.generation() != generation {
        span.finish_with("applied", false);
        return Vec::new();
    }
    if let Some(kind) = diff_result.fallbacks.media_kind() {
        // Binary content that sniffs as media behind a non-media extension:
        // hand over to the media viewer instead of rendering a "binary" diff.
        span.finish_with("applied", "media");
        return vec![DiffPanelAction::Media(MediaAction::SwitchToMedia { kind })];
    }
    let WorkingTreeDiffResult {
        file_path,
        lines,
        old_file_content,
        new_file_content,
        fallbacks,
        ..
    } = diff_result;
    let lines = Arc::new(lines);
    state.set_lines(lines.clone());
    state.set_fallbacks(fallbacks.clone());
    let old_document = old_file_content
        .as_ref()
        .map(|content| HighlightDocument::from_path(content, file_path.as_str()));
    let new_document = new_file_content
        .as_ref()
        .map(|content| HighlightDocument::from_path(content, file_path.as_str()));
    state.set_highlight_documents(old_document, new_document);
    state.reset_highlight_provider();
    let highlight_provider = state.highlight_provider();
    let presentation_generation = *next_generation;
    *next_generation = next_generation.wrapping_add(1).max(1);
    state.set_generation(presentation_generation);
    span.field(
        "extension",
        crate::services::file_extension_from_path(&file_path),
    )
    .finish_with("applied", true);
    vec![DiffPanelAction::RunSingleFileRenderBuild {
        generation: presentation_generation,
        kind: state.render_kind(),
        file_path,
        lines,
        fallbacks,
        highlight_provider,
    }]
}

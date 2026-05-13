//! Diff panel action handler. Migrated out of `RepositoryScreen` as part of
//! the package-layout extraction.

use iced::Task;

use crate::{
    message::{AppMessage, Message},
    services::{highlight_document, HighlightDocument},
    widgets::diff_canvas,
    work::{git_read_work, git_write_work, presentation_work},
};

use super::super::super::{
    panel_messages::DiffPanelAction,
    state::{
        conflict_ours_scroll_id, conflict_output_scroll_id, conflict_theirs_scroll_id,
        diff_content_scroll_id, FocusedPanel, OperationKind,
    },
    RepositoryMessage,
};
use super::super::ScreenCtx;
use super::conflict::{
    conflict_resolution_output_lines, displayed_conflict_hunk_len, CONFLICT_RESOLUTION_LINE_HEIGHT,
};
use super::{
    conflict_resolution_output_text, ConflictFileResolutionState, ConflictScrollTarget,
    ConflictSide, DiffPanel,
};

const KEYBOARD_DIFF_SCROLL_DELTA_Y: f32 = crate::widgets::text::DEFAULT_CONTENT_LINE_HEIGHT;
const SCROLL_TO_BOTTOM_Y: f32 = 1_000_000_000.0;

#[derive(Debug, Clone, Copy)]
enum DiffKeyboardScroll {
    Up,
    Down,
    Top,
    Bottom,
}

pub(in crate::screens::repository) fn update(
    panel: &mut DiffPanel,
    action: DiffPanelAction,
    ctx: &mut ScreenCtx<'_>,
) -> Task<Message> {
    if action_focuses_diff(&action) {
        ctx.input.focused_panel = FocusedPanel::Center;
    }

    match action {
        DiffPanelAction::ScrollUp => scroll_focused_diff(panel, DiffKeyboardScroll::Up),
        DiffPanelAction::ScrollDown => scroll_focused_diff(panel, DiffKeyboardScroll::Down),
        DiffPanelAction::ScrollTop => scroll_focused_diff(panel, DiffKeyboardScroll::Top),
        DiffPanelAction::ScrollBottom => scroll_focused_diff(panel, DiffKeyboardScroll::Bottom),
        DiffPanelAction::NavigateFileUp => {
            let merged_files = ctx.merged_diff.result().map(|m| m.files.as_slice());
            if let Some(follow_up) = panel.navigate_file(
                -1,
                ctx.data.snapshot.commits(),
                ctx.data.cache.states(),
                merged_files,
            ) {
                update(panel, follow_up, ctx)
            } else {
                Task::none()
            }
        }
        DiffPanelAction::NavigateFileDown => {
            let merged_files = ctx.merged_diff.result().map(|m| m.files.as_slice());
            if let Some(follow_up) = panel.navigate_file(
                1,
                ctx.data.snapshot.commits(),
                ctx.data.cache.states(),
                merged_files,
            ) {
                update(panel, follow_up, ctx)
            } else {
                Task::none()
            }
        }
        DiffPanelAction::DiffSelectionBegin {
            canvas_id,
            row,
            col,
            viewport_rect,
            data,
        } => {
            use crate::widgets::diff_canvas::DiffPosition;
            const DOUBLE_CLICK_MS: u128 = 400;
            let now = std::time::Instant::now();
            let pos = DiffPosition { row, col };
            let double = panel.diff_last_click.is_some_and(|(t, id, p)| {
                id == canvas_id && p == pos && now.duration_since(t).as_millis() < DOUBLE_CLICK_MS
            });
            if double {
                panel.begin_selection_word(canvas_id, row, col, &data);
                panel.diff_last_click = None;
            } else {
                panel.begin_selection_char(canvas_id, row, col);
                panel.diff_last_click = Some((now, canvas_id, pos));
            }
            panel.diff_viewport_rect = Some(viewport_rect);
            panel.diff_drag_canvas_data = Some(data);
            Task::none()
        }
        DiffPanelAction::DiffSelectionExtend {
            canvas_id,
            row,
            col,
        } => {
            use crate::widgets::diff_canvas::SelectionMode;
            let mode = panel.diff_selection.as_ref().and_then(|(id, s)| {
                if *id == canvas_id {
                    Some(s.mode)
                } else {
                    None
                }
            });
            match mode {
                Some(SelectionMode::Word) => {
                    if let Some(data) = panel.diff_drag_canvas_data.clone() {
                        panel.extend_selection_word(canvas_id, row, col, &data);
                    } else {
                        panel.extend_selection(canvas_id, row, col);
                    }
                }
                _ => {
                    panel.extend_selection(canvas_id, row, col);
                }
            }
            Task::none()
        }
        DiffPanelAction::DiffSelectionEnd { canvas_id } => {
            panel.finalize_selection(canvas_id);
            Task::none()
        }
        DiffPanelAction::DiffSelectionCleared => {
            panel.diff_selection = None;
            panel.diff_viewport_rect = None;
            panel.diff_drag_canvas_data = None;
            Task::none()
        }
        DiffPanelAction::DiffGutterClicked {
            canvas_id,
            row: _,
            meta,
        } => {
            if let Some(side) = crate::widgets::conflict_canvas::side_for_canvas_id(canvas_id) {
                let hunk_idx = meta as usize;
                panel.toggle_hunk_side(hunk_idx, side);
            }
            Task::none()
        }
        DiffPanelAction::DiffContentScrolled { viewport } => {
            let off = viewport.absolute_offset();
            panel.diff_scroll_y = off.y;
            panel.diff_scroll_x = off.x;
            panel.diff_viewport_height = Some(viewport.bounds().height);
            if panel.schedule_visible_highlights() {
                continue_visible_highlighting_task(ctx.tab_id)
            } else {
                Task::none()
            }
        }
        DiffPanelAction::ContinueVisibleHighlighting => {
            if panel.schedule_visible_highlights() {
                continue_visible_highlighting_task(ctx.tab_id)
            } else {
                Task::none()
            }
        }
        DiffPanelAction::SyntaxGrammarAssetsChanged => {
            if let Some(follow_up) = panel.on_syntax_grammar_assets_changed() {
                update(panel, follow_up, ctx)
            } else {
                Task::none()
            }
        }
        DiffPanelAction::DiffShiftWheel { delta_lines } => {
            const PX_PER_LINE: f32 = 60.0;
            let new_x = (panel.diff_scroll_x - delta_lines * PX_PER_LINE).max(0.0);
            let y = panel.diff_scroll_y;
            iced::widget::operation::scroll_to(
                diff_content_scroll_id(),
                iced::widget::scrollable::AbsoluteOffset { x: new_x, y },
            )
        }
        DiffPanelAction::DiffCopyRequested => {
            if let Some(text) = panel.copy_selection_text() {
                if !text.is_empty() {
                    return Task::done(Message::App(AppMessage::CopyToClipboard(text)));
                }
            }
            Task::none()
        }
        DiffPanelAction::ConflictHunkSideToggled { hunk_idx, side } => {
            panel.toggle_hunk_side(hunk_idx, side);
            Task::none()
        }
        DiffPanelAction::ConflictSideAllToggled(side) => {
            panel.toggle_all_conflict_side(side);
            Task::none()
        }
        DiffPanelAction::ConflictBufferScrolled { side, viewport } => {
            panel.handle_conflict_buffer_scrolled(side, viewport)
        }
        DiffPanelAction::ConflictOutputScrolled { viewport } => {
            panel.handle_conflict_output_scrolled(viewport)
        }
        DiffPanelAction::ConflictShiftWheel {
            target,
            delta_lines,
        } => {
            const PX_PER_LINE: f32 = 60.0;
            let Some(state) = panel.conflict_file_resolution.as_ref() else {
                return Task::none();
            };
            let (scroll_id, cur_x, y) = match target {
                ConflictScrollTarget::Ours => (
                    conflict_ours_scroll_id(),
                    state.ours_scroll_offset_x,
                    state.ours_scroll_offset_y,
                ),
                ConflictScrollTarget::Theirs => (
                    conflict_theirs_scroll_id(),
                    state.theirs_scroll_offset_x,
                    state.theirs_scroll_offset_y,
                ),
                ConflictScrollTarget::Output => (
                    conflict_output_scroll_id(),
                    state.output_scroll_offset_x,
                    state.output_scroll_offset_y,
                ),
            };
            let new_x = (cur_x - delta_lines * PX_PER_LINE).max(0.0);
            iced::widget::operation::scroll_to(
                scroll_id,
                iced::widget::scrollable::AbsoluteOffset { x: new_x, y },
            )
        }
        DiffPanelAction::ConflictResolutionSaveRequested => {
            if let Some(_action) = panel.save_conflict_resolution() {
                let Some(state) = panel.conflict_file_resolution.as_ref() else {
                    return Task::none();
                };
                let Some(result) = state.result.as_ref() else {
                    return Task::none();
                };

                let path = state.file_path.clone();
                let content = conflict_resolution_output_text(result, &state.selections);
                let Some(operation_id) = ctx
                    .data
                    .operations
                    .begin_write_kind(OperationKind::ResolveConflict)
                else {
                    return Task::none();
                };
                let repo = ctx.repository.clone();
                let presenter = ctx.presenter.clone();
                let tab_id = ctx.tab_id;
                Task::perform(
                    git_write_work(move || {
                        repo.save_conflict_resolution(&path, &content)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(
                            tab_id,
                            RepositoryMessage::ConflictResolutionSaved {
                                operation_id,
                                result,
                            },
                        )
                    },
                )
            } else {
                Task::none()
            }
        }
        DiffPanelAction::ConflictHighlightReady {
            generation,
            ours,
            theirs,
        } => {
            panel.on_conflict_highlight_ready(generation, ours, theirs);
            panel.refresh_text_search_after_buffer_change();
            Task::none()
        }
        DiffPanelAction::LoadCommitFileDiff {
            generation,
            commit_hash,
            path,
        } => {
            let repo = ctx.repository.clone();
            let tab_id = ctx.tab_id;
            let repo_path = ctx.fleet.active_path().display().to_string();
            let hash_for_log = commit_hash.clone();
            let path_for_log = path.clone();
            Task::perform(
                git_read_work(move || {
                    let span = crate::perf::Span::new("git.single_file_diff_load")
                        .field("tab", tab_id)
                        .field("repo", &repo_path)
                        .field("kind", "commit")
                        .field("hash", &hash_for_log)
                        .field("path", &path_for_log);
                    let result = repo.load_commit_file_diff(&commit_hash, &path);
                    match &result {
                        Ok(diff) => span
                            .field(
                                "old_bytes",
                                diff.old_file_content.as_ref().map_or(0, |c| c.len()),
                            )
                            .field(
                                "new_bytes",
                                diff.new_file_content.as_ref().map_or(0, |c| c.len()),
                            )
                            .field(
                                "old_lines",
                                diff.old_file_content
                                    .as_ref()
                                    .map_or(0, |c| c.lines().count()),
                            )
                            .field(
                                "new_lines",
                                diff.new_file_content
                                    .as_ref()
                                    .map_or(0, |c| c.lines().count()),
                            )
                            .finish_with("lines", diff.lines.len()),
                        Err(_) => span.finish_with("outcome", "err"),
                    }
                    result
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::CommitFileDiffLoaded { generation, result },
                    )
                },
            )
        }
        DiffPanelAction::LoadDirtyFileDiff {
            generation,
            path,
            is_staged,
        } => {
            let repo = ctx.repository.clone();
            let tab_id = ctx.tab_id;
            let repo_path = ctx.fleet.active_path().display().to_string();
            let path_for_log = path.clone();
            Task::perform(
                git_read_work(move || {
                    let span = crate::perf::Span::new("git.single_file_diff_load")
                        .field("tab", tab_id)
                        .field("repo", &repo_path)
                        .field("kind", "dirty")
                        .field("path", &path_for_log)
                        .field("staged", is_staged);
                    let result = repo.load_working_tree_diff(&path, is_staged);
                    match &result {
                        Ok(diff) => span
                            .field(
                                "old_bytes",
                                diff.old_file_content.as_ref().map_or(0, |c| c.len()),
                            )
                            .field(
                                "new_bytes",
                                diff.new_file_content.as_ref().map_or(0, |c| c.len()),
                            )
                            .field(
                                "old_lines",
                                diff.old_file_content
                                    .as_ref()
                                    .map_or(0, |c| c.lines().count()),
                            )
                            .field(
                                "new_lines",
                                diff.new_file_content
                                    .as_ref()
                                    .map_or(0, |c| c.lines().count()),
                            )
                            .finish_with("lines", diff.lines.len()),
                        Err(_) => span.finish_with("outcome", "err"),
                    }
                    result
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DirtyFileDiffLoaded { generation, result },
                    )
                },
            )
        }
        DiffPanelAction::LoadConflictResolution { generation, path } => {
            let repo = ctx.repository.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                git_read_work(move || repo.load_conflict_resolution(&path)),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::ConflictResolutionLoaded { generation, result },
                    )
                },
            )
        }
        DiffPanelAction::RunConflictHighlight {
            generation,
            file_path,
            ours_content,
            theirs_content,
        } => {
            let tab_id = ctx.tab_id;
            let path_for_log = file_path.clone();
            Task::perform(
                presentation_work(move || {
                    let ours_doc = build_highlight_document(ours_content, &file_path);
                    let theirs_doc = build_highlight_document(theirs_content, &file_path);
                    let ours_bytes = ours_doc.as_ref().map_or(0, |doc| doc.byte_count());
                    let theirs_bytes = theirs_doc.as_ref().map_or(0, |doc| doc.byte_count());
                    let ours_lines = ours_doc.as_ref().map_or(0, |doc| doc.line_count());
                    let theirs_lines = theirs_doc.as_ref().map_or(0, |doc| doc.line_count());
                    let span = crate::perf::Span::new("cpu.syntax_highlight")
                        .field("tab", tab_id)
                        .field("kind", "conflict")
                        .field("path", &path_for_log)
                        .field("ours_bytes", ours_bytes)
                        .field("theirs_bytes", theirs_bytes)
                        .field("ours_lines", ours_lines)
                        .field("theirs_lines", theirs_lines);
                    let ours = ours_doc.as_ref().map(highlight_document);
                    let theirs = theirs_doc.as_ref().map(highlight_document);
                    let ours_spans = ours.as_ref().map_or(0, |file| file.span_count());
                    let theirs_spans = theirs.as_ref().map_or(0, |file| file.span_count());
                    span.field("line_count", ours_lines + theirs_lines)
                        .field("ours_spans", ours_spans)
                        .field("theirs_spans", theirs_spans)
                        .finish_with("span_count", ours_spans + theirs_spans);
                    (ours, theirs)
                }),
                move |(ours, theirs)| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DiffPanel(DiffPanelAction::ConflictHighlightReady {
                            generation,
                            ours,
                            theirs,
                        }),
                    )
                },
            )
        }
        DiffPanelAction::LoadMergedFileDiff {
            generation,
            hashes,
            path,
        } => {
            let repo = ctx.repository.clone();
            let tab_id = ctx.tab_id;
            let repo_path = ctx.fleet.active_path().display().to_string();
            let path_for_log = path.clone();
            let commit_count = hashes.len();
            Task::perform(
                git_read_work(move || {
                    let span = crate::perf::Span::new("git.single_file_diff_load")
                        .field("tab", tab_id)
                        .field("repo", &repo_path)
                        .field("kind", "merged")
                        .field("path", &path_for_log)
                        .field("commits", commit_count);
                    let result = repo.load_merged_commit_file_diff(&hashes, &path);
                    match &result {
                        Ok(diff) => span
                            .field(
                                "old_bytes",
                                diff.old_file_content.as_ref().map_or(0, |c| c.len()),
                            )
                            .field(
                                "new_bytes",
                                diff.new_file_content.as_ref().map_or(0, |c| c.len()),
                            )
                            .field(
                                "old_lines",
                                diff.old_file_content
                                    .as_ref()
                                    .map_or(0, |c| c.lines().count()),
                            )
                            .field(
                                "new_lines",
                                diff.new_file_content
                                    .as_ref()
                                    .map_or(0, |c| c.lines().count()),
                            )
                            .finish_with("lines", diff.lines.len()),
                        Err(_) => span.finish_with("outcome", "err"),
                    }
                    result
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::MergedCommitFileDiffLoaded { generation, result },
                    )
                },
            )
        }
        DiffPanelAction::RunSingleFileRenderBuild {
            generation,
            kind,
            file_path,
            lines,
            fallbacks,
            highlight_provider,
        } => {
            let tab_id = ctx.tab_id;
            let cb_kind = kind.clone();
            let cb_path = file_path.clone();
            Task::perform(
                presentation_work(move || {
                    let highlight_provider: std::sync::Arc<dyn diff_canvas::DiffHighlightProvider> =
                        highlight_provider;
                    let rows = super::view::build_diff_rows_public(
                        lines.as_ref(),
                        &fallbacks,
                        Some(highlight_provider),
                    );
                    diff_canvas::build_canvas_data(rows, diff_canvas::diff_char_width())
                }),
                move |data| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DiffPanel(DiffPanelAction::SingleFileRenderReady {
                            generation,
                            kind: cb_kind.clone(),
                            file_path: cb_path.clone(),
                            data,
                        }),
                    )
                },
            )
        }
        DiffPanelAction::SingleFileRenderReady {
            generation,
            kind,
            file_path,
            data,
        } => {
            let continue_highlighting =
                panel.on_single_file_render_ready(kind, generation, file_path, data);
            panel.refresh_text_search_after_buffer_change();
            if continue_highlighting {
                continue_visible_highlighting_task(ctx.tab_id)
            } else {
                Task::none()
            }
        }
        DiffPanelAction::TextSearch(msg) => {
            let shift = ctx.input.modifiers.shift();
            panel.handle_text_search(msg, shift)
        }
        DiffPanelAction::CanvasHoverEntered(canvas_id) => {
            panel.on_canvas_hover_entered(canvas_id);
            Task::none()
        }
        DiffPanelAction::None => Task::none(),
    }
}

fn action_focuses_diff(action: &DiffPanelAction) -> bool {
    matches!(
        action,
        DiffPanelAction::DiffSelectionBegin { .. }
            | DiffPanelAction::DiffGutterClicked { .. }
            | DiffPanelAction::DiffShiftWheel { .. }
            | DiffPanelAction::DiffCopyRequested
            | DiffPanelAction::ConflictHunkSideToggled { .. }
            | DiffPanelAction::ConflictSideAllToggled(_)
            | DiffPanelAction::ConflictShiftWheel { .. }
            | DiffPanelAction::ConflictResolutionSaveRequested
            | DiffPanelAction::TextSearch(_)
    )
}

fn build_highlight_document(content: Option<String>, file_path: &str) -> Option<HighlightDocument> {
    content.map(|content| HighlightDocument::from_path(content, file_path))
}

fn scroll_focused_diff(panel: &mut DiffPanel, direction: DiffKeyboardScroll) -> Task<Message> {
    if panel.conflict_file_resolution.is_some() {
        return scroll_focused_conflict_buffer(panel, direction);
    }

    if !panel.is_active() {
        return Task::none();
    }

    let max_y = single_file_max_scroll_y(panel);
    let target_y = keyboard_scroll_y(panel.diff_scroll_y, direction, max_y);

    if should_mirror_keyboard_scroll(direction, max_y) {
        panel.diff_scroll_y = target_y;
    }

    iced::widget::operation::scroll_to(
        diff_content_scroll_id(),
        iced::widget::scrollable::AbsoluteOffset {
            x: panel.diff_scroll_x,
            y: target_y,
        },
    )
}

fn scroll_focused_conflict_buffer(
    panel: &mut DiffPanel,
    direction: DiffKeyboardScroll,
) -> Task<Message> {
    use crate::widgets::conflict_canvas::{CANVAS_ID_OURS, CANVAS_ID_OUTPUT, CANVAS_ID_THEIRS};

    let Some(state) = panel.conflict_file_resolution.as_mut() else {
        return Task::none();
    };

    let target = match panel.hovered_canvas {
        Some(CANVAS_ID_OURS) => ConflictScrollTarget::Ours,
        Some(CANVAS_ID_THEIRS) => ConflictScrollTarget::Theirs,
        Some(CANVAS_ID_OUTPUT) => ConflictScrollTarget::Output,
        _ => ConflictScrollTarget::Output,
    };

    let (scroll_id, current_x, current_y) = match target {
        ConflictScrollTarget::Ours => (
            conflict_ours_scroll_id(),
            state.ours_scroll_offset_x,
            state.ours_scroll_offset_y,
        ),
        ConflictScrollTarget::Theirs => (
            conflict_theirs_scroll_id(),
            state.theirs_scroll_offset_x,
            state.theirs_scroll_offset_y,
        ),
        ConflictScrollTarget::Output => (
            conflict_output_scroll_id(),
            state.output_scroll_offset_x,
            state.output_scroll_offset_y,
        ),
    };

    let max_y = conflict_max_scroll_y(state, target);
    let target_y = keyboard_scroll_y(current_y, direction, max_y);
    if should_mirror_keyboard_scroll(direction, max_y) {
        match target {
            ConflictScrollTarget::Ours => state.ours_scroll_offset_y = target_y,
            ConflictScrollTarget::Theirs => state.theirs_scroll_offset_y = target_y,
            ConflictScrollTarget::Output => state.output_scroll_offset_y = target_y,
        }
    }

    iced::widget::operation::scroll_to(
        scroll_id,
        iced::widget::scrollable::AbsoluteOffset {
            x: current_x,
            y: target_y,
        },
    )
}

fn single_file_max_scroll_y(panel: &DiffPanel) -> Option<f32> {
    let viewport_height = panel.diff_viewport_height?;
    let content_height = panel
        .active_single_file_diff()?
        .render_data()?
        .total_height();
    Some(max_scroll_y(content_height, viewport_height))
}

fn conflict_max_scroll_y(
    state: &ConflictFileResolutionState,
    target: ConflictScrollTarget,
) -> Option<f32> {
    let result = state.result.as_ref()?;
    let (content_height, viewport_height) = match target {
        ConflictScrollTarget::Ours => (
            conflict_side_row_count(result, ConflictSide::Ours) as f32
                * CONFLICT_RESOLUTION_LINE_HEIGHT,
            state.ours_viewport_height?,
        ),
        ConflictScrollTarget::Theirs => (
            conflict_side_row_count(result, ConflictSide::Theirs) as f32
                * CONFLICT_RESOLUTION_LINE_HEIGHT,
            state.theirs_viewport_height?,
        ),
        ConflictScrollTarget::Output => (
            conflict_resolution_output_lines(result, &state.selections).len() as f32
                * CONFLICT_RESOLUTION_LINE_HEIGHT,
            state.output_viewport_height?,
        ),
    };

    Some(max_scroll_y(content_height, viewport_height))
}

fn conflict_side_row_count(
    result: &crate::services::ConflictResolutionResult,
    side: ConflictSide,
) -> usize {
    result
        .blocks
        .iter()
        .map(|block| match block {
            crate::services::ConflictBlock::Context(lines) => lines.len(),
            crate::services::ConflictBlock::Conflict(hunk) => match side {
                ConflictSide::Ours => displayed_conflict_hunk_len(&hunk.ours_lines),
                ConflictSide::Theirs => displayed_conflict_hunk_len(&hunk.theirs_lines),
            },
        })
        .sum()
}

fn max_scroll_y(content_height: f32, viewport_height: f32) -> f32 {
    (content_height - viewport_height).max(0.0)
}

fn keyboard_scroll_y(current_y: f32, direction: DiffKeyboardScroll, max_y: Option<f32>) -> f32 {
    let raw_y = match direction {
        DiffKeyboardScroll::Up => (current_y - KEYBOARD_DIFF_SCROLL_DELTA_Y).max(0.0),
        DiffKeyboardScroll::Down => current_y + KEYBOARD_DIFF_SCROLL_DELTA_Y,
        DiffKeyboardScroll::Top => 0.0,
        DiffKeyboardScroll::Bottom => SCROLL_TO_BOTTOM_Y,
    };

    match direction {
        DiffKeyboardScroll::Bottom => max_y.unwrap_or(raw_y),
        _ => max_y.map_or(raw_y, |max_y| raw_y.min(max_y)),
    }
}

fn should_mirror_keyboard_scroll(direction: DiffKeyboardScroll, max_y: Option<f32>) -> bool {
    max_y.is_some() || matches!(direction, DiffKeyboardScroll::Up | DiffKeyboardScroll::Top)
}

fn continue_visible_highlighting_task(tab_id: crate::core::TabId) -> Task<Message> {
    Task::perform(
        async {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        },
        move |_| {
            Message::tab(
                tab_id,
                RepositoryMessage::DiffPanel(DiffPanelAction::ContinueVisibleHighlighting),
            )
        },
    )
}

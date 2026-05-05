//! Diff panel action handler. Migrated out of `RepositoryScreen` as part of
//! the package-layout extraction.

use iced::Task;

use crate::{
    message::{AppMessage, Message},
    widgets::diff_canvas,
    work::{git_read_work, git_write_work, presentation_work},
};

use super::super::super::{
    panel_messages::DiffPanelAction,
    state::{
        conflict_ours_scroll_id, conflict_output_scroll_id, conflict_theirs_scroll_id,
        diff_content_scroll_id, OperationKind,
    },
    RepositoryMessage,
};
use super::super::ScreenCtx;
use super::{conflict_resolution_output_text, ConflictScrollTarget, DiffPanel};

pub(in crate::screens::repository) fn update(
    panel: &mut DiffPanel,
    action: DiffPanelAction,
    ctx: &mut ScreenCtx<'_>,
) -> Task<Message> {
    match action {
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
            Task::none()
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
        DiffPanelAction::DirtyFileHighlightReady {
            generation,
            file_path,
            is_staged,
            old,
            new,
        } => {
            if let Some(action) =
                panel.on_dirty_highlight_ready(generation, file_path, is_staged, old, new)
            {
                update(panel, action, ctx)
            } else {
                Task::none()
            }
        }
        DiffPanelAction::CommitFileHighlightReady {
            generation,
            commit_hash,
            file_path,
            old,
            new,
        } => {
            if let Some(action) =
                panel.on_commit_highlight_ready(generation, commit_hash, file_path, old, new)
            {
                update(panel, action, ctx)
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
                        Ok(diff) => span.finish_with("lines", diff.lines.len()),
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
                        Ok(diff) => span.finish_with("lines", diff.lines.len()),
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
        DiffPanelAction::RunDirtyHighlight {
            generation,
            file_path,
            is_staged,
            old_content,
            new_content,
        } => {
            let tab_id = ctx.tab_id;
            let ext = crate::services::file_extension_from_path(&file_path);
            let cb_path = file_path.clone();
            let path_for_log = file_path.clone();
            Task::perform(
                presentation_work(move || {
                    let old_bytes = old_content.as_ref().map_or(0, |c| c.len());
                    let new_bytes = new_content.as_ref().map_or(0, |c| c.len());
                    let span = crate::perf::Span::new("cpu.syntax_highlight")
                        .field("tab", tab_id)
                        .field("kind", "dirty")
                        .field("path", &path_for_log)
                        .field("old_bytes", old_bytes)
                        .field("new_bytes", new_bytes);
                    let old = old_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let new = new_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let old_lines = old.as_ref().map_or(0, |file| file.line_count());
                    let new_lines = new.as_ref().map_or(0, |file| file.line_count());
                    span.finish_with("lines", old_lines + new_lines);
                    (old, new)
                }),
                move |(old, new)| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DiffPanel(DiffPanelAction::DirtyFileHighlightReady {
                            file_path: cb_path.clone(),
                            generation,
                            is_staged,
                            old,
                            new,
                        }),
                    )
                },
            )
        }
        DiffPanelAction::RunCommitHighlight {
            generation,
            commit_hash,
            file_path,
            old_content,
            new_content,
        } => {
            let tab_id = ctx.tab_id;
            let ext = crate::services::file_extension_from_path(&file_path);
            let cb_hash = commit_hash.clone();
            let cb_path = file_path.clone();
            let path_for_log = file_path.clone();
            let hash_for_log = commit_hash.clone();
            Task::perform(
                presentation_work(move || {
                    let old_bytes = old_content.as_ref().map_or(0, |c| c.len());
                    let new_bytes = new_content.as_ref().map_or(0, |c| c.len());
                    let span = crate::perf::Span::new("cpu.syntax_highlight")
                        .field("tab", tab_id)
                        .field("kind", "commit")
                        .field("hash", &hash_for_log)
                        .field("path", &path_for_log)
                        .field("old_bytes", old_bytes)
                        .field("new_bytes", new_bytes);
                    let old = old_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let new = new_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let old_lines = old.as_ref().map_or(0, |file| file.line_count());
                    let new_lines = new.as_ref().map_or(0, |file| file.line_count());
                    span.finish_with("lines", old_lines + new_lines);
                    (old, new)
                }),
                move |(old, new)| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DiffPanel(DiffPanelAction::CommitFileHighlightReady {
                            commit_hash: cb_hash.clone(),
                            file_path: cb_path.clone(),
                            generation,
                            old,
                            new,
                        }),
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
            let ext = crate::services::file_extension_from_path(&file_path);
            let path_for_log = file_path.clone();
            Task::perform(
                presentation_work(move || {
                    let ours_bytes = ours_content.as_ref().map_or(0, |c| c.len());
                    let theirs_bytes = theirs_content.as_ref().map_or(0, |c| c.len());
                    let span = crate::perf::Span::new("cpu.syntax_highlight")
                        .field("tab", tab_id)
                        .field("kind", "conflict")
                        .field("path", &path_for_log)
                        .field("ours_bytes", ours_bytes)
                        .field("theirs_bytes", theirs_bytes);
                    let ours = ours_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let theirs = theirs_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let ours_lines = ours.as_ref().map_or(0, |file| file.line_count());
                    let theirs_lines = theirs.as_ref().map_or(0, |file| file.line_count());
                    span.finish_with("lines", ours_lines + theirs_lines);
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
                        Ok(diff) => span.finish_with("lines", diff.lines.len()),
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
        DiffPanelAction::RunMergedHighlight {
            generation,
            file_path,
            old_content,
            new_content,
        } => {
            let tab_id = ctx.tab_id;
            let ext = crate::services::file_extension_from_path(&file_path);
            let cb_path = file_path.clone();
            let path_for_log = file_path.clone();
            Task::perform(
                presentation_work(move || {
                    let old_bytes = old_content.as_ref().map_or(0, |c| c.len());
                    let new_bytes = new_content.as_ref().map_or(0, |c| c.len());
                    let span = crate::perf::Span::new("cpu.syntax_highlight")
                        .field("tab", tab_id)
                        .field("kind", "merged")
                        .field("path", &path_for_log)
                        .field("old_bytes", old_bytes)
                        .field("new_bytes", new_bytes);
                    let old = old_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let new = new_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let old_lines = old.as_ref().map_or(0, |file| file.line_count());
                    let new_lines = new.as_ref().map_or(0, |file| file.line_count());
                    span.finish_with("lines", old_lines + new_lines);
                    (old, new)
                }),
                move |(old, new)| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DiffPanel(DiffPanelAction::MergedFileHighlightReady {
                            generation,
                            file_path: cb_path.clone(),
                            old,
                            new,
                        }),
                    )
                },
            )
        }
        DiffPanelAction::MergedFileHighlightReady {
            generation,
            file_path,
            old,
            new,
        } => {
            if let Some(action) = panel.on_merged_highlight_ready(generation, file_path, old, new) {
                update(panel, action, ctx)
            } else {
                Task::none()
            }
        }
        DiffPanelAction::RunSingleFileRenderBuild {
            generation,
            kind,
            file_path,
            lines,
            fallbacks,
            old_highlighted,
            new_highlighted,
        } => {
            let tab_id = ctx.tab_id;
            let cb_kind = kind.clone();
            let cb_path = file_path.clone();
            Task::perform(
                presentation_work(move || {
                    let rows = super::view::build_diff_rows_public(
                        lines.as_ref(),
                        old_highlighted.as_deref(),
                        new_highlighted.as_deref(),
                        &fallbacks,
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
            panel.on_single_file_render_ready(kind, generation, file_path, data);
            panel.refresh_text_search_after_buffer_change();
            Task::none()
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

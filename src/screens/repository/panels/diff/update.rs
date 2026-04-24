use iced::Task;

use crate::message::{AppMessage, Message};

use super::super::super::{
    gateway_work,
    panel_messages::DiffPanelAction,
    state::{
        conflict_ours_scroll_id, conflict_output_scroll_id, conflict_theirs_scroll_id,
        diff_content_scroll_id,
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
                let repo = ctx.repository.clone();
                let presenter = ctx.presenter.clone();
                let tab_id = ctx.tab_id;
                Task::perform(
                    gateway_work(move || {
                        repo.save_conflict_resolution(&path, &content)
                            .map(|s| presenter.project_loaded(s))
                    }),
                    move |result| {
                        Message::tab(tab_id, RepositoryMessage::ConflictResolutionSaved(result))
                    },
                )
            } else {
                Task::none()
            }
        }
        DiffPanelAction::DirtyFileHighlightReady {
            file_path,
            is_staged,
            old,
            new,
        } => {
            panel.on_dirty_highlight_ready(file_path, is_staged, old, new);
            panel.refresh_text_search_after_buffer_change();
            Task::none()
        }
        DiffPanelAction::CommitFileHighlightReady {
            commit_hash,
            file_path,
            old,
            new,
        } => {
            panel.on_commit_highlight_ready(commit_hash, file_path, old, new);
            panel.refresh_text_search_after_buffer_change();
            Task::none()
        }
        DiffPanelAction::ConflictHighlightReady { ours, theirs } => {
            panel.on_conflict_highlight_ready(ours, theirs);
            panel.refresh_text_search_after_buffer_change();
            Task::none()
        }
        DiffPanelAction::LoadCommitFileDiff { commit_hash, path } => {
            let repo = ctx.repository.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                gateway_work(move || repo.load_commit_file_diff(&commit_hash, &path)),
                move |result| Message::tab(tab_id, RepositoryMessage::CommitFileDiffLoaded(result)),
            )
        }
        DiffPanelAction::LoadDirtyFileDiff { path, is_staged } => {
            let repo = ctx.repository.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                gateway_work(move || repo.load_working_tree_diff(&path, is_staged)),
                move |result| Message::tab(tab_id, RepositoryMessage::DirtyFileDiffLoaded(result)),
            )
        }
        DiffPanelAction::LoadConflictResolution { path } => {
            let repo = ctx.repository.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                gateway_work(move || repo.load_conflict_resolution(&path)),
                move |result| {
                    Message::tab(tab_id, RepositoryMessage::ConflictResolutionLoaded(result))
                },
            )
        }
        DiffPanelAction::RunDirtyHighlight {
            file_path,
            is_staged,
            old_content,
            new_content,
        } => {
            let tab_id = ctx.tab_id;
            let ext = crate::services::file_extension_from_path(&file_path);
            let cb_path = file_path.clone();
            Task::perform(
                async move {
                    let old = old_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let new = new_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    (old, new)
                },
                move |(old, new)| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DiffPanel(DiffPanelAction::DirtyFileHighlightReady {
                            file_path: cb_path.clone(),
                            is_staged,
                            old,
                            new,
                        }),
                    )
                },
            )
        }
        DiffPanelAction::RunCommitHighlight {
            commit_hash,
            file_path,
            old_content,
            new_content,
        } => {
            let tab_id = ctx.tab_id;
            let ext = crate::services::file_extension_from_path(&file_path);
            let cb_hash = commit_hash.clone();
            let cb_path = file_path.clone();
            Task::perform(
                async move {
                    let old = old_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let new = new_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    (old, new)
                },
                move |(old, new)| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DiffPanel(DiffPanelAction::CommitFileHighlightReady {
                            commit_hash: cb_hash.clone(),
                            file_path: cb_path.clone(),
                            old,
                            new,
                        }),
                    )
                },
            )
        }
        DiffPanelAction::RunConflictHighlight {
            file_path,
            ours_content,
            theirs_content,
        } => {
            let tab_id = ctx.tab_id;
            let ext = crate::services::file_extension_from_path(&file_path);
            Task::perform(
                async move {
                    let ours = ours_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let theirs = theirs_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    (ours, theirs)
                },
                move |(ours, theirs)| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DiffPanel(DiffPanelAction::ConflictHighlightReady {
                            ours,
                            theirs,
                        }),
                    )
                },
            )
        }
        DiffPanelAction::LoadMergedFileDiff { hashes, path } => {
            let repo = ctx.repository.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                gateway_work(move || repo.load_merged_commit_file_diff(&hashes, &path)),
                move |result| {
                    Message::tab(tab_id, RepositoryMessage::MergedCommitFileDiffLoaded(result))
                },
            )
        }
        DiffPanelAction::RunMergedHighlight {
            file_path,
            old_content,
            new_content,
        } => {
            let tab_id = ctx.tab_id;
            let ext = crate::services::file_extension_from_path(&file_path);
            let cb_path = file_path.clone();
            Task::perform(
                async move {
                    let old = old_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    let new = new_content
                        .map(|c| crate::services::highlight_file(&c.replace('\t', "    "), &ext));
                    (old, new)
                },
                move |(old, new)| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DiffPanel(DiffPanelAction::MergedFileHighlightReady {
                            file_path: cb_path.clone(),
                            old,
                            new,
                        }),
                    )
                },
            )
        }
        DiffPanelAction::MergedFileHighlightReady {
            file_path,
            old,
            new,
        } => {
            panel.on_merged_highlight_ready(file_path, old, new);
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

//! Center panel action handler. Migrated out of `RepositoryScreen` as part of
//! the package-layout extraction: the screen is a composer, each panel's update lives
//! beside its state and view.

use std::time::Duration;

use iced::{clipboard, Point, Task};

use crate::{
    message::Message,
    toast::ToastData,
    work::{git_write_work, timer_work},
};

use super::super::super::{
    overlays::{
        cherry_pick_confirm, create_branch, create_tag, delete_branch, delete_tag, rename_branch,
        revert_confirm, stash_delete, ActiveDialog,
    },
    panel_messages::CenterAction,
    state::{eligible_tag_push_remote_names, CommitContextMenuState, ContextMenuState},
    RepositoryMessage,
};
use super::super::sidebar::{commit_selection_changed_task, load_more_commits};
use super::super::ScreenCtx;
use super::CenterPanel;

pub(in crate::screens::repository) fn update(
    panel: &mut CenterPanel,
    action: CenterAction,
    ctx: &mut ScreenCtx<'_>,
) -> Task<Message> {
    match action {
        CenterAction::CommitSelected(idx) => {
            ctx.data.branch_popout.close();
            let mods = ctx.input.modifiers;
            if mods.shift() {
                ctx.data.selection.extend_range_to(idx);
            } else if mods.control() || mods.command() {
                ctx.data.selection.toggle_commit(idx);
            } else {
                ctx.data.selection.select_commit(idx);
            }
            commit_selection_changed_task(ctx, idx)
        }
        CenterAction::ModifiersChanged(mods) => {
            ctx.input.modifiers = mods;
            Task::none()
        }
        CenterAction::NavigateUp => {
            let current = ctx.data.selection.selected_commit();
            if current > 0 {
                let follow_up = panel.select_commit(
                    current - 1,
                    &mut ctx.data.selection,
                    &mut ctx.data.branch_popout,
                );
                Task::batch(vec![
                    update(panel, follow_up, ctx),
                    panel.scroll_to_commit(current - 1).unwrap_or(Task::none()),
                ])
            } else {
                Task::none()
            }
        }
        CenterAction::NavigateDown => {
            let current = ctx.data.selection.selected_commit();
            let max = ctx.data.snapshot.commits().len().saturating_sub(1);
            if current < max {
                let follow_up = panel.select_commit(
                    current + 1,
                    &mut ctx.data.selection,
                    &mut ctx.data.branch_popout,
                );
                Task::batch(vec![
                    update(panel, follow_up, ctx),
                    panel.scroll_to_commit(current + 1).unwrap_or(Task::none()),
                ])
            } else {
                Task::none()
            }
        }
        CenterAction::BranchLabelClicked {
            branch_name,
            is_remote_only,
            remote_ref,
        } => {
            let follow_up = CenterPanel::handle_branch_label_pressed(
                &mut ctx.data.branch_popout,
                branch_name,
                is_remote_only,
                remote_ref,
            );
            update(panel, follow_up, ctx)
        }
        CenterAction::BranchLabelPressed(branch_name) => {
            // On confirmed double-click checkout: if the branch is checked out in
            // another worktree, route to focus swap — libgit2 would otherwise let
            // a plain checkout proceed and smear that worktree's state onto the
            // current workdir as uncommitted changes.
            if let Some(path) = ctx.data.snapshot.worktree_path_for_branch(&branch_name) {
                if !ctx.fleet.is_active(path) {
                    return super::super::super::commands::focus_swap_to_worktree(
                        ctx,
                        path.to_path_buf(),
                    );
                }
            }
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                git_write_work(move || {
                    repo.checkout_branch(&branch_name)
                        .map(|s| presenter.project_loaded(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::WriteRepoLoaded {
                            operation_id,
                            result,
                        },
                    )
                },
            )
        }
        CenterAction::RemoteBranchLabelPressed {
            branch_name,
            remote_ref,
        } => {
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            let checkout_ref = remote_ref.unwrap_or_else(|| branch_name.clone());
            Task::perform(
                git_write_work(move || {
                    repo.checkout_remote_branch(&checkout_ref)
                        .map(|o| presenter.project_remote_checkout(o))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::RemoteCheckoutCompleted {
                            operation_id,
                            result,
                        },
                    )
                },
            )
        }
        CenterAction::BranchLabelRightClicked {
            branch_name,
            is_remote,
            has_remote,
            is_tag,
            remote_name,
            remote_ref: _,
            remote_branch_name,
        } => {
            let tag_remote_names = if is_tag {
                ctx.repository.tag_remotes_for(&branch_name)
            } else {
                Vec::new()
            };
            let default_remote_name = if is_tag {
                ctx.data
                    .snapshot
                    .default_remote_name()
                    .map(|s| s.to_string())
            } else {
                None
            };
            let tag_push_remote_names = if is_tag {
                eligible_tag_push_remote_names(
                    ctx.data.snapshot.remote_names(),
                    default_remote_name.as_deref(),
                    &tag_remote_names,
                )
            } else {
                Vec::new()
            };
            let current = ctx.data.snapshot.current_branch().to_string();
            let can_fast_forward = !is_tag
                && !is_remote
                && !current.is_empty()
                && current != branch_name
                && ctx.data.snapshot.can_fast_forward_to(&branch_name);
            let position = ctx.input.last_pointer_position.unwrap_or(Point::ORIGIN);
            ctx.data.branch_popout.open_context_menu(ContextMenuState {
                branch_name,
                tag_remote_names,
                tag_push_remote_names,
                is_remote,
                has_remote,
                is_tag,
                remote_name,
                remote_branch_name,
                can_fast_forward,
                position,
            });
            Task::none()
        }
        CenterAction::BranchMergeRequested {
            source_branch,
            target_branch,
        } => {
            ctx.data.branch_popout.close_context_menu();
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            let source_for_task = source_branch.clone();
            let target_for_task = target_branch.clone();
            Task::perform(
                git_write_work(move || {
                    repo.merge_branch_into(&source_for_task, &target_for_task)
                        .map(|o| presenter.project_branch_merge(o))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::BranchMerged {
                            operation_id: Some(operation_id),
                            source_branch: source_branch.clone(),
                            target_branch: target_branch.clone(),
                            result,
                        },
                    )
                },
            )
        }
        CenterAction::BranchFastForwardRequested {
            source_branch,
            target_branch,
        } => {
            ctx.data.branch_popout.close_context_menu();
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            let src = source_branch.clone();
            let tgt = target_branch.clone();
            Task::perform(
                git_write_work(move || {
                    repo.fast_forward_branch_to_branch(&src, &tgt)
                        .map(|s| presenter.project_loaded(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::WriteRepoLoaded {
                            operation_id,
                            result,
                        },
                    )
                },
            )
        }
        CenterAction::BranchRebaseRequested {
            source_branch,
            target_ref,
            target_display,
            target_remote_ref: _,
        } => {
            ctx.data.branch_popout.close_context_menu();
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            let src_for_task = source_branch.clone();
            let target_for_task = target_ref.clone();
            Task::perform(
                git_write_work(move || {
                    repo.rebase_current_onto(&src_for_task, &target_for_task)
                        .map(|s| presenter.project_loaded(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::BranchRebased {
                            operation_id: Some(operation_id),
                            source_branch: source_branch.clone(),
                            target_display: target_display.clone(),
                            result,
                        },
                    )
                },
            )
        }
        CenterAction::ResetSubmenuRequested {
            commit_idx: _,
            commit_hash,
        } => {
            if ctx.data.snapshot.current_branch().is_empty() {
                return Task::none();
            }
            let position = ctx.input.last_pointer_position.unwrap_or(Point::ORIGIN);
            ctx.data
                .branch_popout
                .open_reset_submenu(commit_hash, position);
            Task::none()
        }
        CenterAction::ResetParentHoverChanged {
            commit_hash,
            hovered,
        } => {
            let tab_id = ctx.tab_id;
            let tracker = ctx.data.branch_popout.reset_hover_mut();
            tracker.commit_hash = commit_hash;
            tracker.parent_hovered = hovered;
            if hovered {
                tracker.open_gen = tracker.open_gen.wrapping_add(1);
                let gen = tracker.open_gen;
                Task::perform(timer_work(Duration::from_millis(750)), move |_| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::Center(CenterAction::ResetOpenTimer(gen)),
                    )
                })
            } else if !tracker.submenu_hovered {
                tracker.close_gen = tracker.close_gen.wrapping_add(1);
                let gen = tracker.close_gen;
                Task::perform(timer_work(Duration::from_millis(500)), move |_| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::Center(CenterAction::ResetCloseTimer(gen)),
                    )
                })
            } else {
                Task::none()
            }
        }
        CenterAction::ResetSubmenuHoverChanged(hovered) => {
            let tab_id = ctx.tab_id;
            let tracker = ctx.data.branch_popout.reset_hover_mut();
            tracker.submenu_hovered = hovered;
            if !hovered && !tracker.parent_hovered {
                tracker.close_gen = tracker.close_gen.wrapping_add(1);
                let gen = tracker.close_gen;
                Task::perform(timer_work(Duration::from_millis(500)), move |_| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::Center(CenterAction::ResetCloseTimer(gen)),
                    )
                })
            } else {
                Task::none()
            }
        }
        CenterAction::ResetOpenTimer(gen) => {
            let tracker = ctx.data.branch_popout.reset_hover();
            if tracker.open_gen == gen
                && (tracker.parent_hovered || tracker.submenu_hovered)
                && ctx.data.branch_popout.active_reset_submenu().is_none()
                && !ctx.data.snapshot.current_branch().is_empty()
            {
                let hash = tracker.commit_hash.clone();
                let position = ctx.input.last_pointer_position.unwrap_or(Point::ORIGIN);
                ctx.data.branch_popout.open_reset_submenu(hash, position);
            }
            Task::none()
        }
        CenterAction::ResetCloseTimer(gen) => {
            let tracker = ctx.data.branch_popout.reset_hover();
            if tracker.close_gen == gen && !tracker.parent_hovered && !tracker.submenu_hovered {
                ctx.data.branch_popout.close_reset_submenu_only();
            }
            Task::none()
        }
        CenterAction::ResetToCommitRequested { commit_hash, mode } => {
            ctx.data.branch_popout.close_context_menu();
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                git_write_work(move || {
                    repo.reset_current_branch_to_commit(&commit_hash, mode)
                        .map(|s| presenter.project_loaded(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::WriteRepoLoaded {
                            operation_id,
                            result,
                        },
                    )
                },
            )
        }
        CenterAction::BranchDeleteRequested {
            branch_name,
            is_remote,
            has_remote,
            remote_name,
            remote_ref,
        } => {
            ctx.data.branch_popout.close_context_menu();

            if !ctx.overlay_manager.can_confirm_branch_delete(
                ctx.data.snapshot.current_branch(),
                &branch_name,
                is_remote,
            ) {
                return Task::batch([
                    panel.restore_center_list_scroll(),
                    Task::done(Message::show_toast(ToastData::delete_failed(
                        &branch_name,
                        is_remote,
                        "Cannot delete currently checked out branch. Switch branches first.",
                    ))),
                ]);
            }

            ctx.overlay_manager
                .open(ActiveDialog::DeleteBranch(delete_branch::State {
                    branch_name,
                    is_remote,
                    has_remote,
                    remote_name,
                    remote_ref,
                }));
            panel.restore_center_list_scroll()
        }
        CenterAction::BranchRenameRequested {
            branch_name,
            is_remote,
            remote_name,
            remote_ref,
        } => {
            ctx.data.branch_popout.close_context_menu();
            ctx.overlay_manager
                .open(ActiveDialog::RenameBranch(rename_branch::State {
                    branch_name: branch_name.clone(),
                    is_remote,
                    new_branch_input: branch_name,
                    needs_focus: true,
                    remote_name,
                    remote_ref,
                }));
            panel.restore_center_list_scroll()
        }
        CenterAction::CommitRightClicked { commit_idx } => {
            ctx.data.branch_popout.close();
            if matches!(
                ctx.overlay_manager.active(),
                Some(
                    ActiveDialog::DeleteBranch(_)
                        | ActiveDialog::RenameBranch(_)
                        | ActiveDialog::CreateBranchHere(_)
                )
            ) {
                ctx.overlay_manager.close();
            }

            if let Some(hash) = ctx.data.commit_hash(commit_idx) {
                let (stash_index, stash_display_name) =
                    match ctx.data.stash_info_for_commit(commit_idx) {
                        Some((idx, name)) => (Some(idx), Some(name)),
                        None => (None, None),
                    };
                let selected_indices = ctx.data.selection.selected_indices();
                let selected_hashes = selected_indices
                    .iter()
                    .filter_map(|&idx| ctx.data.commit_hash(idx))
                    .collect();
                let position = ctx.input.last_pointer_position.unwrap_or(Point::ORIGIN);
                ctx.data
                    .branch_popout
                    .open_commit_context_menu(CommitContextMenuState {
                        commit_idx,
                        commit_hash: hash,
                        position,
                        stash_index,
                        stash_display_name,
                        selected_indices,
                        selected_hashes,
                    });
            }
            Task::none()
        }
        CenterAction::StashCreateRequested => {
            ctx.data.branch_popout.close_context_menu();
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                git_write_work(move || repo.create_stash().map(|s| presenter.project_loaded(s))),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::DirtyIndexChanged {
                            operation_id: Some(operation_id),
                            result,
                        },
                    )
                },
            )
        }
        CenterAction::StashApplyRequested { stash_index } => {
            ctx.data.branch_popout.close_context_menu();
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                git_write_work(move || {
                    repo.apply_stash(stash_index)
                        .map(|o| presenter.project_stash_apply(o))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::StashApplyCompleted {
                            operation_id: Some(operation_id),
                            result,
                        },
                    )
                },
            )
        }
        CenterAction::StashPopRequested { stash_index } => {
            ctx.data.branch_popout.close_context_menu();
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                git_write_work(move || {
                    repo.pop_stash(stash_index)
                        .map(|o| presenter.project_stash_apply(o))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::StashPopCompleted {
                            operation_id: Some(operation_id),
                            result,
                        },
                    )
                },
            )
        }
        CenterAction::StashDeleteRequested {
            stash_index,
            display_name,
        } => {
            ctx.data.branch_popout.close_context_menu();
            ctx.overlay_manager
                .open(ActiveDialog::StashDelete(stash_delete::State {
                    stash_index,
                    display_name,
                }));
            panel.restore_center_list_scroll()
        }
        CenterAction::CreateBranchHereRequested {
            commit_idx: _,
            commit_hash,
        } => {
            ctx.data.branch_popout.close_context_menu();
            ctx.overlay_manager
                .open(ActiveDialog::CreateBranchHere(create_branch::State {
                    commit_hash,
                    branch_name_input: String::new(),
                    needs_focus: true,
                }));
            panel.restore_center_list_scroll()
        }
        CenterAction::SquashCommitsRequested { indices: _, hashes } => {
            ctx.data.branch_popout.close_context_menu();
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            Task::perform(
                git_write_work(move || {
                    repo.squash_commits(hashes)
                        .map(|s| presenter.project_loaded(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::SquashCompleted {
                            operation_id,
                            result,
                        },
                    )
                },
            )
        }
        CenterAction::CenterListScrolled(viewport) => {
            let viewport_changed = panel.center_list.update(viewport);
            if viewport_changed {
                ctx.data.branch_popout.close_popout();
            }
            let follow_up = panel.load_more_commits_if_needed(ctx.data);
            match follow_up {
                CenterAction::LoadMoreCommitsRequested => load_more_commits(ctx, panel),
                _ => Task::none(),
            }
        }
        CenterAction::BranchLabelTriggerEntered(idx) => {
            let hash = ctx.data.commit_hash(idx).unwrap_or_default();
            if ctx.data.branch_popout.open(idx, hash) {
                CenterPanel::measure_branch_popout_bounds()
            } else {
                Task::none()
            }
        }
        CenterAction::BranchLabelTriggerBounds(bounds) => {
            ctx.data
                .branch_popout
                .update_trigger_bounds(bounds, ctx.input.last_pointer_position);
            if ctx.data.branch_popout.needs_panel_bounds_measurement() {
                CenterPanel::measure_branch_popout_bounds()
            } else {
                Task::none()
            }
        }
        CenterAction::BranchLabelPopoutBounds(bounds) => {
            ctx.data
                .branch_popout
                .update_panel_bounds(bounds, ctx.input.last_pointer_position);
            Task::none()
        }
        CenterAction::BranchLabelContentBounds(bounds) => {
            ctx.data.branch_popout.update_content_bounds(bounds);
            if ctx.data.branch_popout.needs_panel_bounds_measurement() {
                CenterPanel::measure_branch_popout_bounds()
            } else {
                Task::none()
            }
        }
        CenterAction::CopyBranchName(branch_name) => {
            ctx.data.branch_popout.close_context_menu();
            clipboard::write(branch_name)
        }
        CenterAction::CloseContextMenu => {
            ctx.data.branch_popout.close_context_menu();
            panel.restore_center_list_scroll()
        }
        CenterAction::PanelFocused(focus) => {
            ctx.input.focused_panel = focus;
            Task::none()
        }
        CenterAction::PointerMoved(position) => {
            ctx.input.last_pointer_position = Some(position);
            ctx.data.branch_popout.pointer_moved(position);
            let resize = &mut ctx.data.resize;
            if !resize.any_resizing() {
                return Task::none();
            }
            if resize.sidebar_resizing {
                resize.handle_sidebar(position.x);
            }
            if resize.detail_resizing {
                resize.handle_detail(position.x.floor());
            }
            if resize.detail_height_resizing {
                resize.handle_detail_height(position.y.floor());
            }
            Task::none()
        }
        CenterAction::PointerLeftWindow => {
            ctx.data.branch_popout.pointer_left_window();
            ctx.data.resize.stop_all();
            Task::none()
        }
        CenterAction::DetailResizeStarted { effective_width } => {
            ctx.data.resize.stop_sidebar();
            let pointer_x = ctx
                .input
                .last_pointer_position
                .map(|p| p.x.floor())
                .unwrap_or(0.0);
            ctx.data.resize.start_detail(pointer_x, effective_width);
            Task::none()
        }
        CenterAction::DetailHeightResizeStarted => {
            ctx.data.resize.stop_sidebar();
            ctx.data.resize.stop_detail();
            let pointer_y = ctx
                .input
                .last_pointer_position
                .map(|p| p.y.floor())
                .unwrap_or(0.0);
            ctx.data.resize.start_detail_height(pointer_y);
            Task::none()
        }
        CenterAction::ResizeReleased => {
            ctx.data.resize.stop_all();
            Task::none()
        }
        CenterAction::LoadMoreCommitsRequested => load_more_commits(ctx, panel),
        CenterAction::CreateTagHereRequested { commit_hash } => {
            ctx.data.branch_popout.close_context_menu();
            ctx.overlay_manager
                .open(ActiveDialog::CreateTagHere(create_tag::State {
                    commit_hash,
                    tag_name_input: String::new(),
                    needs_focus: true,
                }));
            panel.restore_center_list_scroll()
        }
        CenterAction::CherryPickRequested { commit_hash } => {
            ctx.data.branch_popout.close_context_menu();
            ctx.overlay_manager
                .open(ActiveDialog::CherryPick(cherry_pick_confirm::State {
                    commit_hash,
                }));
            panel.restore_center_list_scroll()
        }
        CenterAction::RevertCommitRequested { commit_hash } => {
            ctx.data.branch_popout.close_context_menu();
            ctx.overlay_manager
                .open(ActiveDialog::Revert(revert_confirm::State { commit_hash }));
            panel.restore_center_list_scroll()
        }
        CenterAction::PushTagRequested {
            tag_name,
            remote_name,
        } => {
            ctx.data.branch_popout.close_context_menu();
            let Some(operation_id) = ctx.data.operations.begin_write() else {
                return Task::none();
            };
            let repo = ctx.repository.clone();
            let presenter = ctx.presenter.clone();
            let tab_id = ctx.tab_id;
            let tag_clone = tag_name.clone();
            let remote_clone = remote_name.clone();
            Task::perform(
                git_write_work(move || {
                    repo.push_tag(&remote_clone, &tag_clone)
                        .map(|s| presenter.project_loaded(s))
                }),
                move |result| {
                    Message::tab(
                        tab_id,
                        RepositoryMessage::TagPushed {
                            operation_id,
                            tag_name: tag_name.clone(),
                            remote_name: remote_name.clone(),
                            result,
                        },
                    )
                },
            )
        }
        CenterAction::DeleteTagRequested {
            tag_name,
            tag_remote_names,
        } => {
            ctx.data.branch_popout.close_context_menu();
            ctx.overlay_manager
                .open(ActiveDialog::DeleteTag(delete_tag::State {
                    tag_name,
                    tag_remote_names,
                }));
            panel.restore_center_list_scroll()
        }
        CenterAction::None => Task::none(),
    }
}

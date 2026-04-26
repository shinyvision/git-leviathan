use iced::{Point, Task};

use crate::message::Message;

use super::super::super::state::{FocusedPanel, PendingFocus};
use super::super::super::RepositoryMessage;
use super::super::center::CenterPanel;
use super::super::ScreenCtx;
use super::{SidebarAction, SidebarPanel};

use super::super::super::gateway_work;

/// Resolves a pending focus request. Mirrors the screen-level helper but
/// operates on the panels directly — returns the `Task` to run plus a flag
/// indicating whether focus was consumed.
pub(in crate::screens::repository) fn try_resolve_pending_focus(
    ctx: &mut ScreenCtx<'_>,
    repository_panel: &mut CenterPanel,
) -> Option<Task<Message>> {
    let focus = ctx.data.pending_focus.clone()?;
    let idx = match &focus {
        PendingFocus::Branch { name, is_remote } => {
            ctx.data.commit_index_for_branch(name, *is_remote)
        }
        PendingFocus::Stash { hash } => ctx.data.commit_index_for_hash(hash),
        PendingFocus::Commit { hash } => ctx.data.commit_index_for_hash(hash),
    };
    if let Some(idx) = idx {
        ctx.data.branch_popout.close();
        ctx.data.selection.select_commit(idx);
        ctx.data.pending_focus = None;
        let select_task = commit_selection_changed_task(ctx, idx);
        let scroll_task = repository_panel
            .scroll_to_commit(idx)
            .unwrap_or(Task::none());
        Some(Task::batch(vec![select_task, scroll_task]))
    } else if repository_panel.has_more_commits {
        Some(load_more_commits_for_pending_focus(ctx, repository_panel))
    } else {
        ctx.data.pending_focus = None;
        None
    }
}

fn load_more_commits_for_pending_focus(
    ctx: &mut ScreenCtx<'_>,
    repository_panel: &mut CenterPanel,
) -> Task<Message> {
    if repository_panel.loading_more_commits || !repository_panel.has_more_commits {
        if !repository_panel.has_more_commits {
            ctx.data.pending_focus = None;
        }
        return Task::none();
    }
    repository_panel.loading_more_commits = true;
    load_more_commits(ctx, repository_panel)
}

pub(in crate::screens::repository) fn load_more_commits(
    ctx: &mut ScreenCtx<'_>,
    repository_panel: &mut CenterPanel,
) -> Task<Message> {
    use crate::services::COMMIT_LOAD_LIMIT;
    let next_commit_limit = ctx.data.snapshot.commits().len() + COMMIT_LOAD_LIMIT;
    let repo_version = repository_panel.repo_version;
    let repo = ctx.repository.clone();
    let presenter = ctx.presenter.clone();
    let tab_id = ctx.tab_id;
    Task::perform(
        gateway_work(move || {
            repo.load_repo(next_commit_limit)
                .map(|s| presenter.project_loaded(s))
        }),
        move |result| {
            Message::tab(
                tab_id,
                RepositoryMessage::MoreCommitsLoaded {
                    repo_version,
                    result,
                },
            )
        },
    )
}

pub(in crate::screens::repository) fn commit_selection_changed_task(
    ctx: &mut ScreenCtx<'_>,
    clicked_idx: usize,
) -> Task<Message> {
    if ctx.data.selection.is_multi() {
        let version = ctx.merged_diff.invalidate();
        let hashes: Vec<String> = ctx
            .data
            .selection
            .selected_indices()
            .into_iter()
            .filter_map(|i| ctx.data.commit_hash(i))
            .collect();
        if hashes.is_empty() {
            return Task::none();
        }
        let repo = ctx.repository.clone();
        let tab_id = ctx.tab_id;
        return Task::perform(
            gateway_work(move || repo.load_merged_commit_diff(hashes)),
            move |result| {
                Message::tab(
                    tab_id,
                    RepositoryMessage::MergedCommitDiffLoaded { version, result },
                )
            },
        );
    }

    ctx.merged_diff.invalidate();

    let idx = clicked_idx;
    if !ctx.data.commit_needs_diff(idx) {
        return Task::none();
    }
    let Some(hash) = ctx.data.commit_hash(idx) else {
        return Task::none();
    };
    let repo = ctx.repository.clone();
    let tab_id = ctx.tab_id;
    Task::perform(
        gateway_work(move || repo.load_commit_diff(idx, &hash)),
        move |result| Message::tab(tab_id, RepositoryMessage::CommitDiffLoaded(result)),
    )
}

pub(super) fn update(
    panel: &mut SidebarPanel,
    action: SidebarAction,
    ctx: &mut ScreenCtx<'_>,
    repository_panel: &mut CenterPanel,
) -> Task<Message> {
    match action {
        SidebarAction::SectionToggled(idx) => {
            if let Some((kind, expanded)) =
                panel.toggle_section(ctx.data.snapshot.sidebar_sections(), idx)
            {
                let repo_path = ctx.fleet.primary_path().to_string_lossy().to_string();
                if let Ok(settings) = crate::services::SettingsService::new() {
                    let _ = settings.set_sidebar_section(&repo_path, kind.db_key(), expanded);
                }
            }
            Task::none()
        }
        SidebarAction::BranchPressed {
            branch_name,
            is_remote,
        } => {
            if panel.register_branch_click(&branch_name) {
                // Double-click on a branch that's already checked out in some
                // worktree other than the active one → swap to that worktree
                // instead of issuing a checkout (which libgit2 would let run
                // and would just drag the branch's tree onto the current
                // workdir as uncommitted changes).
                if !is_remote {
                    if let Some(path) = ctx.data.snapshot.worktree_path_for_branch(&branch_name) {
                        if !ctx.fleet.is_active(path) {
                            return super::super::super::commands::focus_swap_to_worktree(
                                ctx,
                                path.to_path_buf(),
                            );
                        }
                    }
                }
                checkout_task(ctx, branch_name, is_remote)
            } else {
                ctx.data.pending_focus = Some(PendingFocus::Branch {
                    name: branch_name,
                    is_remote,
                });
                try_resolve_pending_focus(ctx, repository_panel).unwrap_or(Task::none())
            }
        }
        SidebarAction::BranchRightClicked {
            branch_name,
            is_remote,
            is_tag,
            remote_name,
        } => {
            let tag_remote_names = if is_tag {
                ctx.repository.tag_remotes_for(&branch_name)
            } else {
                Vec::new()
            };
            let default_remote_name = if is_tag {
                ctx.data.snapshot.default_remote_name().map(|s| s.to_string())
            } else {
                None
            };
            let current = ctx.data.snapshot.current_branch().to_string();
            let can_fast_forward = !is_tag
                && !is_remote
                && !current.is_empty()
                && current != branch_name
                && ctx.data.snapshot.can_fast_forward_to(&branch_name);
            let position = ctx.input.last_pointer_position.unwrap_or(Point::ORIGIN);
            ctx.data.branch_popout.open_sidebar_context_menu(
                branch_name,
                is_remote,
                is_tag,
                remote_name,
                tag_remote_names,
                default_remote_name,
                can_fast_forward,
                position,
            );
            Task::none()
        }
        SidebarAction::StashPressed { hash } => {
            ctx.data.pending_focus = Some(PendingFocus::Stash { hash });
            try_resolve_pending_focus(ctx, repository_panel).unwrap_or(Task::none())
        }
        SidebarAction::WorktreeEntryPressed { path } => {
            if panel.register_worktree_click(&path) {
                // Double-click → focus swap.
                super::super::super::commands::focus_swap_to_worktree(ctx, path)
            } else {
                // Single-click → pending-focus to the worktree's branch head commit.
                let branch_name = ctx
                    .data
                    .snapshot
                    .sidebar_sections()
                    .iter()
                    .find(|s| s.kind == crate::view_model::SidebarSectionKind::Worktrees)
                    .and_then(|s| {
                        s.branches
                            .iter()
                            .find(|b| b.worktree_path.as_deref() == Some(&path))
                            .map(|b| b.name.clone())
                    })
                    .unwrap_or_default();
                if !branch_name.is_empty() {
                    ctx.data.pending_focus = Some(PendingFocus::Branch {
                        name: branch_name,
                        is_remote: false,
                    });
                    try_resolve_pending_focus(ctx, repository_panel).unwrap_or(Task::none())
                } else {
                    eprintln!(
                        "git_leviathan: worktree entry at '{}' has no matching branch in sidebar sections",
                        path.display(),
                    );
                    Task::none()
                }
            }
        }
        SidebarAction::WorktreeEntryRightClicked { path, branch_name } => {
            let position = ctx.input.last_pointer_position.unwrap_or(iced::Point::ORIGIN);
            let is_active = ctx.fleet.is_active(&path);
            ctx.data.branch_popout.open_worktree_context_menu(
                path,
                branch_name,
                is_active,
                position,
            );
            Task::none()
        }
        SidebarAction::ResizeStarted { effective_width } => {
            ctx.data.resize.start_sidebar(effective_width);
            ctx.data.resize.stop_detail();
            Task::none()
        }
        SidebarAction::Focused => {
            ctx.input.focused_panel = FocusedPanel::Sidebar;
            Task::none()
        }
        SidebarAction::FilterChanged(q) => {
            panel.set_filter_query(q);
            Task::none()
        }
    }
}

fn checkout_task(ctx: &mut ScreenCtx<'_>, branch_name: String, is_remote: bool) -> Task<Message> {
    let repo = ctx.repository.clone();
    let presenter = ctx.presenter.clone();
    let tab_id = ctx.tab_id;
    if is_remote {
        Task::perform(
            gateway_work(move || {
                repo.checkout_remote_branch(&branch_name)
                    .map(|o| presenter.project_remote_checkout(o))
            }),
            move |result| Message::tab(tab_id, RepositoryMessage::RemoteCheckoutCompleted(result)),
        )
    } else {
        Task::perform(
            gateway_work(move || {
                repo.checkout_branch(&branch_name)
                    .map(|s| presenter.project_loaded(s))
            }),
            move |result| Message::tab(tab_id, RepositoryMessage::RepoLoaded(result)),
        )
    }
}

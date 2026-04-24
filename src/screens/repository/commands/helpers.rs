//! Shared helpers used by the command-result dispatchers. Live as free
//! functions (not `pub fn` on `RepositoryScreen`) so the command modules can
//! call them without leaking internals through the `Screen` trait impl.

use iced::Task;

use crate::{
    message::Message,
    services::COMMIT_LOAD_LIMIT,
    view_model::{LoadedBranchMergeOutcome, LoadedRefs, LoadedRemoteCheckoutOutcome, LoadedRepo},
};

use super::super::commit_search;
use super::super::overlays::{conflict_checkout, ActiveDialog};
use super::super::panels::center::CenterPanel;
use super::super::state::{PendingFocus, RepositoryData};
use super::super::{gateway_work, RepositoryMessage, RepositoryScreen};

pub(in crate::screens::repository) fn load_more_commits(
    screen: &mut RepositoryScreen,
) -> Task<Message> {
    let next_commit_limit = screen.data.snapshot.commits().len() + COMMIT_LOAD_LIMIT;
    let repo_version = screen.panels.center.repo_version;
    let repo = screen.fleet.active().clone();
    let presenter = screen.presenter.clone();
    let tab_id = screen.tab_id;
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

pub(in crate::screens::repository) fn load_more_commits_for_pending_focus(
    screen: &mut RepositoryScreen,
) -> Task<Message> {
    if screen.panels.center.loading_more_commits || !screen.panels.center.has_more_commits {
        if !screen.panels.center.has_more_commits {
            screen.data.pending_focus = None;
        }
        return Task::none();
    }

    screen.panels.center.loading_more_commits = true;
    load_more_commits(screen)
}

pub(in crate::screens::repository) fn try_resolve_pending_focus(
    screen: &mut RepositoryScreen,
) -> Option<Task<Message>> {
    let focus = screen.data.pending_focus.clone()?;
    let idx = match &focus {
        PendingFocus::Branch { name, is_remote } => {
            screen.data.commit_index_for_branch(name, *is_remote)
        }
        PendingFocus::Stash { hash } => screen.data.commit_index_for_hash(hash),
        PendingFocus::Commit { hash } => screen.data.commit_index_for_hash(hash),
    };
    if let Some(idx) = idx {
        let select_action = screen.panels.center.select_commit(
            idx,
            &mut screen.data.selection,
            &mut screen.data.branch_popout,
        );
        screen.data.pending_focus = None;
        let select_msg = screen.handle_center_action(select_action);
        let scroll_msg = screen
            .panels
            .center
            .scroll_to_commit(idx)
            .unwrap_or(Task::none());
        Some(Task::batch(vec![select_msg, scroll_msg]))
    } else if screen.panels.center.has_more_commits {
        Some(load_more_commits_for_pending_focus(screen))
    } else {
        screen.data.pending_focus = None;
        None
    }
}

pub(in crate::screens::repository) fn handle_repo_loaded(
    screen: &mut RepositoryScreen,
    loaded: LoadedRepo,
) -> Task<Message> {
    let has_more_commits = loaded.has_more_commits;
    screen.data.replace_loaded(loaded);
    screen.panels.center.on_repo_loaded(
        screen.data.snapshot.commits(),
        has_more_commits,
        &mut screen.data.branch_popout,
    );
    commit_search::refresh_matches(screen);

    let reload_dirty_diff = sync_dirty_diff_after_reload(screen);

    let primary = try_resolve_pending_focus(screen).unwrap_or_else(Task::none);
    let load_selected = load_selected_commit_diff_task(screen);
    Task::batch(vec![primary, load_selected, reload_dirty_diff])
}

pub(in crate::screens::repository) fn sync_dirty_diff_after_reload(
    screen: &mut RepositoryScreen,
) -> Task<Message> {
    let repo = screen.fleet.active().clone();
    let action = {
        let dirty_commit = screen
            .data
            .snapshot
            .commits()
            .first()
            .filter(|c| c.kind == crate::core::CommitKind::Dirty);
        screen.panels.diff.sync_and_reload_dirty_diff_action(
            dirty_commit,
            |path, is_staged| repo.compute_dirty_file_signature(path, is_staged),
        )
    };
    let Some(action) = action else {
        return Task::none();
    };
    screen.handle_diff_panel_action(action)
}

/// If the currently selected commit needs its diff loaded, produce a task
/// that loads it. Shared by full-reload and refs-reload paths so the details
/// panel does not get stuck on "Loading files…" when the selection shifts
/// onto a commit whose diff has not yet been cached.
pub(in crate::screens::repository) fn load_selected_commit_diff_task(
    screen: &mut RepositoryScreen,
) -> Task<Message> {
    let idx = screen.data.selection.selected_commit();
    if !screen.data.commit_needs_diff(idx) {
        return Task::none();
    }
    let Some(hash) = screen.data.commit_hash(idx) else {
        return Task::none();
    };
    let repo = screen.fleet.active().clone();
    let tab_id = screen.tab_id;
    Task::perform(
        gateway_work(move || repo.load_commit_diff(idx, &hash)),
        move |result| Message::tab(tab_id, RepositoryMessage::CommitDiffLoaded(result)),
    )
}

/// Capture the hashes of the anchor and every selected commit from the
/// current snapshot. Used as the first half of a hash-preserving reload so
/// selection can be re-located by hash after `data` is swapped.
pub(in crate::screens::repository) fn capture_selection_hashes(
    data: &RepositoryData,
) -> (Option<String>, Vec<String>) {
    let commits = data.snapshot.commits();
    let anchor_hash = commits.get(data.selection.anchor()).map(|c| c.hash.clone());
    let selected_hashes: Vec<String> = data
        .selection
        .selected_indices()
        .into_iter()
        .filter_map(|idx| commits.get(idx).map(|c| c.hash.clone()))
        .collect();
    (anchor_hash, selected_hashes)
}

/// Second half of a hash-preserving reload: re-anchor selection onto the
/// new commit list by looking up prior hashes. Falls back to HEAD when no
/// prior hash survives — covers force-pushes, branch switches, and the
/// external-WIP-commit case where the prior Dirty anchor had hash `""` that
/// no longer matches anything in the snapshot.
pub(in crate::screens::repository) fn restore_selection_by_hash(
    data: &mut RepositoryData,
    prior_anchor_hash: Option<String>,
    prior_selected_hashes: Vec<String>,
) {
    let (new_anchor, new_indices): (Option<usize>, Vec<usize>) = {
        let new_commits = data.snapshot.commits();
        let new_indices: Vec<usize> = prior_selected_hashes
            .iter()
            .filter_map(|hash| new_commits.iter().position(|c| &c.hash == hash))
            .collect();
        let new_anchor = prior_anchor_hash
            .as_ref()
            .and_then(|hash| new_commits.iter().position(|c| &c.hash == hash));
        (new_anchor, new_indices)
    };

    match (new_anchor, new_indices.is_empty()) {
        (Some(anchor), false) => {
            let mut indices = new_indices;
            if !indices.contains(&anchor) {
                indices.push(anchor);
            }
            data.selection.replace_with_set(anchor, &indices);
        }
        (None, false) => {
            let anchor = new_indices[0];
            data.selection.replace_with_set(anchor, &new_indices);
        }
        _ => {
            let head_idx = data
                .snapshot
                .head_hash()
                .and_then(|head| data.snapshot.commits().iter().position(|c| c.hash == head))
                .unwrap_or(0);
            data.selection.replace_with_single(head_idx);
        }
    }
}

pub(in crate::screens::repository) fn handle_remote_checkout(
    screen: &mut RepositoryScreen,
    outcome: LoadedRemoteCheckoutOutcome,
) -> Task<Message> {
    match outcome {
        LoadedRemoteCheckoutOutcome::Success(_) => {
            screen.data.pending_focus = None;
            screen.reload_task()
        }
        LoadedRemoteCheckoutOutcome::LocalAheadOfRemote {
            branch_name,
            remote_ref,
        } => {
            screen
                .overlay_manager
                .open(ActiveDialog::ConflictCheckout(conflict_checkout::State {
                    branch_name,
                    remote_ref,
                    new_branch_input: String::new(),
                }));
            Task::none()
        }
    }
}

pub(in crate::screens::repository) fn handle_branch_merge_outcome(
    screen: &mut RepositoryScreen,
    source: String,
    target: String,
    outcome: LoadedBranchMergeOutcome,
) -> Task<Message> {
    match outcome {
        LoadedBranchMergeOutcome::Merged(_) => screen.reload_task(),
        LoadedBranchMergeOutcome::Conflicted(_) => {
            screen
                .panels
                .detail
                .set_commit_message_from_merge(&source, &target);
            screen.reload_refs_task()
        }
    }
}

/// Apply a post-fetch refs reload. Deliberately a free function so its
/// parameter list enumerates the only state the fetch path may touch.
/// `overlay_panel`, `commit_details_panel`, `diff_panel`, and their children
/// cannot be referenced from here — the architectural guarantee is enforced
/// by the function signature.
pub(in crate::screens::repository) fn apply_fetched_refs(
    data: &mut RepositoryData,
    panel: &mut CenterPanel,
    refs: LoadedRefs,
) {
    let (prior_anchor_hash, prior_selected_hashes) = capture_selection_hashes(data);
    let has_more = refs.has_more_commits;

    data.apply_refs_update(refs);
    panel.on_refs_updated(
        data.snapshot.commits(),
        has_more,
        &mut data.branch_popout,
    );

    restore_selection_by_hash(data, prior_anchor_hash, prior_selected_hashes);
}

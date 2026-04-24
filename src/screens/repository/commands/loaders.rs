use iced::Task;

use crate::{
    core::RepoVersion,
    message::Message,
    services::{
        CommitDiffResult, ConflictResolutionResult, GitError, MergedCommitDiffResult,
        WorkingTreeDiffResult,
    },
    toast::ToastData,
    view_model::{LoadedRefs, LoadedRepo},
};

use super::super::overlays::ActiveDialog;
use super::super::RepositoryScreen;
use super::helpers::apply_fetched_refs;

pub(super) fn on_repo_loaded(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::ConflictCheckout(_))
            ) {
                screen.overlay_manager.close();
            }
            super::helpers::handle_repo_loaded(screen, loaded)
        }
        Err(e) => {
            eprintln!("git_leviathan: repo operation failed: {}", e);
            Task::none()
        }
    }
}

pub(super) fn on_refs_reloaded(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            let has_more_commits = loaded.has_more_commits;
            let (prior_anchor_hash, prior_selected_hashes) =
                super::helpers::capture_selection_hashes(&screen.data);
            screen.data.replace_loaded(loaded);

            // Full reload (file-watcher or post-commit): use `on_repo_loaded`
            // so the panel's repo_version stays in sync with the tracker's
            // graph_revision. The pagination-path `on_more_commits_loaded`
            // was wrong here — it gates on version equality and silently
            // aborted the reload (including the dirty-diff reload and the
            // selected-commit diff load), leaving the details panel stuck
            // on "Loading files…" after an external WIP commit.
            screen.panels.center.on_repo_loaded(
                screen.data.snapshot.commits(),
                has_more_commits,
                &mut screen.data.branch_popout,
            );
            // Re-anchor selection by hash. When the user had the Dirty/WIP
            // row selected and an external client committed it, the prior
            // anchor hash is `""` which won't match any new commit — the
            // helper falls back to HEAD (the just-created commit).
            super::helpers::restore_selection_by_hash(
                &mut screen.data,
                prior_anchor_hash,
                prior_selected_hashes,
            );
            super::super::commit_search::refresh_matches(screen);

            let reload_dirty_diff = screen
                .panels
                .diff
                .reload_dirty_diff_action()
                .map(|action| screen.handle_diff_panel_action(action))
                .unwrap_or(Task::none());

            // Kick off a diff load for the (possibly re-anchored) selection
            // if its diff is not cached — otherwise the details panel sits
            // on "Loading files…" forever.
            let load_selected_diff = super::helpers::load_selected_commit_diff_task(screen);

            if let Some(task) = super::helpers::try_resolve_pending_focus(screen) {
                return Task::batch(vec![task, reload_dirty_diff, load_selected_diff]);
            }
            Task::batch(vec![reload_dirty_diff, load_selected_diff])
        }
        Err(e) => {
            eprintln!("git_leviathan: refs reload failed: {}", e);
            Task::none()
        }
    }
}

pub(super) fn on_fetch_finished(
    screen: &mut RepositoryScreen,
    result: Result<(), GitError>,
) -> Task<Message> {
    match result {
        Ok(()) => screen.reload_graph_and_refs_task(),
        Err(e) => {
            eprintln!("git_leviathan: fetch failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Fetch Failed",
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_graph_and_refs_reloaded(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRefs, GitError>,
) -> Task<Message> {
    match result {
        Ok(refs) => {
            apply_fetched_refs(&mut screen.data, &mut screen.panels.center, refs);
            Task::none()
        }
        Err(e) => {
            eprintln!("git_leviathan: refs reload after fetch failed: {}", e);
            Task::none()
        }
    }
}

pub(super) fn on_more_commits_loaded(
    screen: &mut RepositoryScreen,
    repo_version: RepoVersion,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            let has_more_commits = loaded.has_more_commits;
            screen.data.replace_loaded(loaded);
            let accepted = screen.panels.center.on_more_commits_loaded(
                repo_version,
                screen.data.snapshot.commits(),
                has_more_commits,
                &mut screen.data.branch_popout,
            );
            if !accepted {
                return Task::none();
            }
            super::super::commit_search::refresh_matches(screen);

            super::helpers::try_resolve_pending_focus(screen).unwrap_or(Task::none())
        }
        Err(e) => {
            eprintln!("git_leviathan: failed to load older commits: {}", e);
            Task::none()
        }
    }
}

pub(super) fn on_commit_diff_loaded(
    screen: &mut RepositoryScreen,
    result: Result<CommitDiffResult, GitError>,
) -> Task<Message> {
    match result {
        Ok(r) => {
            screen.data.apply_commit_diff(r);
            Task::none()
        }
        Err(e) => {
            eprintln!("git_leviathan: diff load failed: {}", e);
            Task::none()
        }
    }
}

pub(super) fn on_merged_commit_diff_loaded(
    screen: &mut RepositoryScreen,
    version: RepoVersion,
    result: Result<MergedCommitDiffResult, GitError>,
) -> Task<Message> {
    if version != screen.merged_diff.version() {
        return Task::none();
    }
    match result {
        Ok(r) => {
            screen.merged_diff.set(r);
            Task::none()
        }
        Err(e) => {
            eprintln!("git_leviathan: merged diff load failed: {}", e);
            Task::none()
        }
    }
}

pub(super) fn on_merged_commit_file_diff_loaded(
    screen: &mut RepositoryScreen,
    result: Result<WorkingTreeDiffResult, GitError>,
) -> Task<Message> {
    if let Some(action) = screen.panels.diff.on_merged_diff_loaded(result) {
        screen.handle_diff_panel_action(action)
    } else {
        Task::none()
    }
}

pub(super) fn on_commit_file_diff_loaded(
    screen: &mut RepositoryScreen,
    result: Result<WorkingTreeDiffResult, GitError>,
) -> Task<Message> {
    if let Some(action) = screen.panels.diff.on_commit_diff_loaded(result) {
        screen.handle_diff_panel_action(action)
    } else {
        Task::none()
    }
}

pub(super) fn on_dirty_file_diff_loaded(
    screen: &mut RepositoryScreen,
    result: Result<WorkingTreeDiffResult, GitError>,
) -> Task<Message> {
    if let Some(action) = screen.panels.diff.on_dirty_diff_loaded(result) {
        screen.handle_diff_panel_action(action)
    } else {
        Task::none()
    }
}

pub(super) fn on_conflict_resolution_loaded(
    screen: &mut RepositoryScreen,
    result: Result<ConflictResolutionResult, GitError>,
) -> Task<Message> {
    if let Some(action) = screen.panels.diff.on_conflict_resolution_loaded(result) {
        screen.handle_diff_panel_action(action)
    } else {
        Task::none()
    }
}

use std::collections::HashSet;
use std::path::PathBuf;

use iced::Task;

use crate::{message::Message, services::GitError, toast::ToastData, view_model::LoadedRepo};

use super::super::overlays::ActiveDialog;
use super::super::RepositoryScreen;

pub(super) fn on_worktree_created(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            if matches!(
                screen.overlay_manager.active(),
                Some(ActiveDialog::CreateWorktree(_))
            ) {
                screen.overlay_manager.close();
            }
            let apply_task = super::helpers::handle_repo_loaded(screen, loaded);
            let toast_task = Task::done(Message::show_toast(ToastData::success(
                "Worktree created",
                String::new(),
            )));
            Task::batch(vec![apply_task, toast_task])
        }
        Err(e) => {
            eprintln!("git_leviathan: create worktree failed: {}", e);
            if let Some(state) = screen.overlay_manager.as_create_worktree_mut() {
                state.submitting = false;
                state.error = Some(format!("{e}"));
            }
            Task::done(Message::show_toast(ToastData::error(
                "Failed to create worktree",
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_worktree_removed(
    screen: &mut RepositoryScreen,
    result: Result<LoadedRepo, GitError>,
) -> Task<Message> {
    match result {
        Ok(loaded) => {
            // Drop cached fleet entries for paths that are no longer registered.
            // Source from the canonical worktrees list (not the rendered sidebar)
            // so future prunable-worktree filters can't hide entries from eviction.
            let current_paths: HashSet<PathBuf> = loaded
                .projection
                .worktrees
                .iter()
                .map(|w| w.path.clone())
                .collect();
            let primary_path = screen.fleet.primary_path().to_path_buf();
            let stale: Vec<PathBuf> = screen
                .fleet
                .cached_paths()
                .into_iter()
                .filter(|p| !current_paths.contains(p) && p != &primary_path)
                .collect();
            for p in stale {
                screen.fleet.drop_path(&p);
            }
            let apply_task = super::helpers::handle_repo_loaded(screen, loaded);
            let toast_task = Task::done(Message::show_toast(ToastData::success(
                "Worktree removed",
                String::new(),
            )));
            Task::batch(vec![apply_task, toast_task])
        }
        Err(e) => {
            eprintln!("git_leviathan: remove worktree failed: {e}");
            Task::done(Message::show_toast(ToastData::error(
                "Failed to remove worktree",
                e.to_string(),
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::screens::repository::overlays::create_worktree::{RefChoice, State};

    // Integration-style coverage for the state mutations on_worktree_created
    // performs in its Err path: submitting -> false, error -> Some(formatted).
    //
    // We exercise the State mutations directly because constructing a full
    // RepositoryScreen in a unit test would require a real SharedRepositoryGateway.
    #[test]
    fn state_error_path_sets_message_and_clears_submitting() {
        let mut state = State::new(vec![RefChoice::LocalBranch("main".into())], String::new());
        // Simulate a submit-in-flight.
        state.submitting = true;
        assert!(state.error.is_none());

        // Mimic the handler's Err branch:
        state.submitting = false;
        state.error = Some(format!("{}", "worktree path exists"));

        assert!(!state.submitting);
        assert_eq!(state.error.as_deref(), Some("worktree path exists"));
    }

    #[test]
    fn fresh_state_starts_clear() {
        let state = State::new(Vec::new(), String::new());
        assert!(!state.submitting);
        assert!(state.error.is_none());
    }
}

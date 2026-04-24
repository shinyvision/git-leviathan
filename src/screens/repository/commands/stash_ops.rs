use iced::Task;

use crate::{
    message::Message, services::GitError, toast::ToastData, view_model::LoadedStashApplyOutcome,
};

use super::super::RepositoryScreen;

pub(super) fn on_stash_apply_completed(
    screen: &mut RepositoryScreen,
    result: Result<LoadedStashApplyOutcome, GitError>,
) -> Task<Message> {
    match result {
        Ok(LoadedStashApplyOutcome::Applied(loaded)) => super::helpers::handle_repo_loaded(screen, loaded),
        Ok(LoadedStashApplyOutcome::Conflicted(loaded)) => {
            let load_task = super::helpers::handle_repo_loaded(screen, loaded);
            let toast_task = Task::done(Message::show_toast(ToastData::error(
                "Merge conflict found",
                "Your stash cannot be cleanly applied to the current working tree. Resolve conflicts to proceed.",
            )));
            Task::batch(vec![load_task, toast_task])
        }
        Err(e) => {
            eprintln!("git_leviathan: stash apply failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Stash Apply Failed",
                e.to_string(),
            )))
        }
    }
}

pub(super) fn on_stash_pop_completed(
    screen: &mut RepositoryScreen,
    result: Result<LoadedStashApplyOutcome, GitError>,
) -> Task<Message> {
    match result {
        Ok(LoadedStashApplyOutcome::Applied(loaded)) => super::helpers::handle_repo_loaded(screen, loaded),
        Ok(LoadedStashApplyOutcome::Conflicted(loaded)) => {
            let load_task = super::helpers::handle_repo_loaded(screen, loaded);
            let toast_task = Task::done(Message::show_toast(ToastData::error(
                "Could not pop stash",
                "Merge conflict found with current working tree. Resolve conflicts first.",
            )));
            Task::batch(vec![load_task, toast_task])
        }
        Err(e) => {
            eprintln!("git_leviathan: stash pop failed: {}", e);
            Task::done(Message::show_toast(ToastData::error(
                "Stash Pop Failed",
                e.to_string(),
            )))
        }
    }
}

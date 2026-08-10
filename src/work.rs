use std::time::Duration;

use iced::Task;

use crate::services::GitError;

pub(crate) async fn git_read_work<T>(
    f: impl FnOnce() -> Result<T, GitError> + Send + 'static,
) -> Result<T, GitError>
where
    T: Send + 'static,
{
    blocking_git_work("git read", f).await
}

pub(crate) async fn git_write_work<T>(
    f: impl FnOnce() -> Result<T, GitError> + Send + 'static,
) -> Result<T, GitError>
where
    T: Send + 'static,
{
    blocking_git_work("git write", f).await
}

async fn blocking_git_work<T>(
    panic_label: &'static str,
    f: impl FnOnce() -> Result<T, GitError> + Send + 'static,
) -> Result<T, GitError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|e| Err(GitError::Other(format!("{panic_label} task panicked: {e}"))))
}

/// Run a CPU-bound presentation closure off the UI thread. Returns `None` if
/// the closure panicked (or the runtime cancelled it) rather than re-panicking
/// inside the iced task future, which would take down the whole runtime — the
/// caller drops the update instead.
pub(crate) async fn presentation_work<T>(f: impl FnOnce() -> T + Send + 'static) -> Option<T>
where
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(value) => Some(value),
        Err(e) => {
            eprintln!("git_leviathan: presentation task failed: {e}");
            None
        }
    }
}

pub(crate) async fn timer_work(delay: Duration) {
    tokio::time::sleep(delay).await;
}

pub(crate) fn ui_message<M: Send + 'static>(message: M) -> Task<M> {
    Task::done(message)
}

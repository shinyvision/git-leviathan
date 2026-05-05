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

pub(crate) async fn presentation_work<T>(f: impl FnOnce() -> T + Send + 'static) -> T
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .expect("presentation task panicked")
}

pub(crate) async fn timer_work(delay: Duration) {
    tokio::time::sleep(delay).await;
}

pub(crate) fn ui_message<M: Send + 'static>(message: M) -> Task<M> {
    Task::done(message)
}

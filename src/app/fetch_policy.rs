//! `FetchPolicy` — single-slot remote-fetch orchestration.
//!
//! Extracted from `App` in keymaps. The app only ever runs one user-driven
//! remote fetch at a time; this struct owns the in-flight slot, its abort
//! handle, and the Ctrl+Tab post-switch debounce handle.

use std::time::{Duration, Instant};

use iced::Task;

use crate::message::{AppMessage, Message};

pub struct FetchPolicy {
    /// `Some(_)` iff a remote fetch is currently in flight. Doubles as the
    /// "spinner start" timestamp that the toolbar reads to render progress.
    started_at: Option<Instant>,
    /// Abort handle for the in-flight fetch. `Some(_)` iff `started_at` is.
    abort: Option<iced::task::Handle>,
    /// Abort handle for the post-Ctrl+Tab debounce timer. Independent of the
    /// fetch itself — the timer fires a `FetchDebounceElapsed` message that
    /// then tries to start a fetch.
    debounce: Option<iced::task::Handle>,
}

impl FetchPolicy {
    pub fn new() -> Self {
        Self {
            started_at: None,
            abort: None,
            debounce: None,
        }
    }

    pub fn is_fetching(&self) -> bool {
        self.started_at.is_some()
    }

    pub fn started_at(&self) -> Option<Instant> {
        self.started_at
    }

    /// Kick off a fetch. Asserts no fetch is already in flight (caller checks
    /// `is_fetching` first). Wraps the task as abortable so `cancel` can
    /// drop the pending `FetchCompleted` message on Ctrl+Tab away.
    pub fn start(&mut self, fetch_task: Task<Message>) -> Task<Message> {
        debug_assert!(
            self.started_at.is_none(),
            "FetchPolicy::start called while fetch already in flight"
        );
        self.started_at = Some(Instant::now());
        let (task, handle) = fetch_task.abortable();
        self.abort = Some(handle);
        task
    }

    /// Called on `FetchCompleted`: clears the fetch slot so the next tick /
    /// tab switch / window focus can start a new fetch.
    pub fn on_completed(&mut self) {
        self.started_at = None;
        self.abort = None;
    }

    /// Abort any in-flight fetch and any pending debounce. Called on Ctrl+Tab
    /// so the single fetch slot is freed for the newly active tab.
    pub fn cancel(&mut self) {
        if let Some(handle) = self.abort.take() {
            handle.abort();
        }
        if let Some(handle) = self.debounce.take() {
            handle.abort();
        }
        self.started_at = None;
    }

    /// Schedule a debounced auto-fetch after `delay`. Used post-Ctrl+Tab so
    /// rapid fly-bys don't each trigger a fetch on the transient landing tab.
    pub fn schedule_debounced(&mut self, delay: Duration) -> Task<Message> {
        let (task, handle) = Task::perform(
            async move {
                tokio::time::sleep(delay).await;
            },
            |_| Message::App(AppMessage::FetchDebounceElapsed),
        )
        .abortable();
        self.debounce = Some(handle);
        task
    }

    /// Called on `FetchDebounceElapsed`: clears the debounce handle so a
    /// fresh schedule can install a new one.
    pub fn on_debounce_elapsed(&mut self) {
        self.debounce = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_idle() {
        let p = FetchPolicy::new();
        assert!(!p.is_fetching());
        assert_eq!(p.started_at(), None);
    }

    #[test]
    fn on_completed_clears_fetch_slot() {
        let mut p = FetchPolicy::new();
        // Simulate an active fetch without kicking off a real iced task.
        p.started_at = Some(Instant::now());
        assert!(p.is_fetching());
        p.on_completed();
        assert!(!p.is_fetching());
    }

    #[test]
    fn cancel_clears_fetch_slot() {
        let mut p = FetchPolicy::new();
        p.started_at = Some(Instant::now());
        p.cancel();
        assert!(!p.is_fetching());
    }
}

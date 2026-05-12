//! `FetchPolicy` — single-slot remote-fetch orchestration.
//!
//! Extracted from `App` in keymaps. The app only ever runs one user-driven
//! remote fetch at a time; this struct owns the in-flight slot, its abort
//! handle, and the Ctrl+Tab post-switch debounce handle.

use std::time::{Duration, Instant};

use iced::Task;

use crate::core::TabId;
use crate::message::{AppMessage, Message};
use crate::screens::repository::state::OperationId;
use crate::work::timer_work;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FetchCancellation {
    pub tab_id: TabId,
    pub operation_id: OperationId,
}

#[derive(Debug, Clone, Copy)]
struct ActiveFetch {
    tab_id: TabId,
    operation_id: OperationId,
    started_at: Instant,
}

pub struct FetchPolicy {
    active: Option<ActiveFetch>,
    abort: Option<iced::task::Handle>,
    /// Abort handle for the post-Ctrl+Tab debounce timer. Independent of the
    /// fetch itself — the timer fires a `FetchDebounceElapsed` message that
    /// then tries to start a fetch.
    debounce: Option<iced::task::Handle>,
}

impl FetchPolicy {
    pub(super) fn new() -> Self {
        Self {
            active: None,
            abort: None,
            debounce: None,
        }
    }

    pub(super) fn is_fetching(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn started_at(&self) -> Option<Instant> {
        self.active.map(|active| active.started_at)
    }

    /// Kick off a fetch. Asserts no fetch is already in flight (caller checks
    /// `is_fetching` first). Wraps the task as abortable so `cancel` can
    /// drop the pending `FetchCompleted` message on Ctrl+Tab away.
    pub(super) fn start(
        &mut self,
        tab_id: TabId,
        operation_id: OperationId,
        fetch_task: Task<Message>,
    ) -> Task<Message> {
        debug_assert!(
            self.active.is_none(),
            "FetchPolicy::start called while fetch already in flight"
        );
        self.active = Some(ActiveFetch {
            tab_id,
            operation_id,
            started_at: Instant::now(),
        });
        let (task, handle) = fetch_task.abortable();
        self.abort = Some(handle);
        task
    }

    /// Called on `FetchCompleted`: clears the fetch slot so the next tick /
    /// tab switch / window focus can start a new fetch.
    pub(super) fn on_completed(&mut self, tab_id: TabId, operation_id: OperationId) -> bool {
        if !self
            .active
            .is_some_and(|active| active.tab_id == tab_id && active.operation_id == operation_id)
        {
            return false;
        }
        self.active = None;
        self.abort = None;
        true
    }

    pub(super) fn cancel_active_fetch(&mut self) -> Option<FetchCancellation> {
        let active = self.active.take()?;
        if let Some(handle) = self.abort.take() {
            handle.abort();
        }
        Some(FetchCancellation {
            tab_id: active.tab_id,
            operation_id: active.operation_id,
        })
    }

    /// Abort any in-flight fetch and any pending debounce. Called on Ctrl+Tab
    /// so the single fetch slot is freed for the newly active tab.
    pub(super) fn cancel(&mut self) -> Option<FetchCancellation> {
        let cancelled = self.cancel_active_fetch();
        if let Some(handle) = self.debounce.take() {
            handle.abort();
        }
        cancelled
    }

    /// Schedule a debounced auto-fetch after `delay`. Used post-Ctrl+Tab so
    /// rapid fly-bys don't each trigger a fetch on the transient landing tab.
    pub(super) fn schedule_debounced(&mut self, delay: Duration) -> Task<Message> {
        let (task, handle) = Task::perform(timer_work(delay), |_| {
            Message::App(AppMessage::FetchDebounceElapsed)
        })
        .abortable();
        self.debounce = Some(handle);
        task
    }

    /// Called on `FetchDebounceElapsed`: clears the debounce handle so a
    /// fresh schedule can install a new one.
    pub(super) fn on_debounce_elapsed(&mut self) {
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
        let mut ops = crate::screens::repository::state::OperationCoordinator::new();
        let operation_id = ops.begin_write().unwrap();
        p.active = Some(ActiveFetch {
            tab_id: TabId(1),
            operation_id,
            started_at: Instant::now(),
        });
        assert!(p.is_fetching());
        assert!(p.on_completed(TabId(1), operation_id));
        assert!(!p.is_fetching());
    }

    #[test]
    fn cancel_clears_fetch_slot() {
        let mut p = FetchPolicy::new();
        let mut ops = crate::screens::repository::state::OperationCoordinator::new();
        let operation_id = ops.begin_write().unwrap();
        p.active = Some(ActiveFetch {
            tab_id: TabId(1),
            operation_id,
            started_at: Instant::now(),
        });
        let cancelled = p.cancel();
        assert!(!p.is_fetching());
        assert_eq!(
            cancelled,
            Some(FetchCancellation {
                tab_id: TabId(1),
                operation_id
            })
        );
    }
}

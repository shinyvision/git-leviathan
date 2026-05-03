//! App-level fetch orchestration: kicking off user/auto fetches and
//! file-watcher-driven refs reloads. Works on top of `FetchPolicy` (which
//! owns the single-slot state) by wiring it to the active-tab screen.
//! Extracted from `update.rs` in Phase 9.

use iced::Task;

use crate::core::TabId;
use crate::message::Message;

use super::App;

impl App {
    pub(super) fn try_start_fetch(&mut self) -> Task<Message> {
        self.try_start_fetch_for_tab(self.tabs.active_tab_id())
    }

    /// On window focus regain: kick an immediate refs reload + auto-fetch on
    /// the active tab rather than waiting up to one fetch-tick interval. The
    /// background watcher/timer keep running while unfocused, so this is only
    /// a latency optimization on regain. No-ops in no-git / empty-tabs states.
    pub(super) fn on_window_focused(&mut self) -> Task<Message> {
        if self.no_git_screen.is_some() || self.tabs.is_empty() {
            return Task::none();
        }
        let reload_task = self
            .tabs
            .active_screen()
            .map(|s| s.reload_refs_task())
            .unwrap_or(Task::none());
        let fetch_task = self.try_start_fetch();
        Task::batch(vec![reload_task, fetch_task])
    }

    pub(super) fn try_start_fetch_for_tab(&mut self, tab_id: TabId) -> Task<Message> {
        if self.fetch.is_fetching() {
            return Task::none();
        }
        let Some(screen) = self.tabs.screen(tab_id) else {
            return Task::none();
        };
        let task = self.fetch.start(screen.fetch_task());
        self.tabs.persist_most_recent_if_needed(tab_id);
        self.plugin_host.fire_event("FetchStart");
        task
    }

    /// Produce a (coalesced) reload task for a tab after a file-watcher event.
    ///
    /// Two suppression paths keep the cascade under control:
    ///   • If a user-driven network op (fetch/push/pull) is in flight on the
    ///     tab, skip entirely — that op's completion message already publishes
    ///     a fresh snapshot, so running a parallel reload just duplicates work
    ///     and piles up tasks blocked on the gateway's write-lock.
    ///   • Otherwise, abort any already-queued `reload_refs_task`. Bursts of
    ///     `.git` events then collapse to the single most-recent reload.
    pub(super) fn reload_refs_for_tab(&mut self, tab_id: TabId) -> Task<Message> {
        let network_op_active = tab_id == self.tabs.active_tab_id() && self.fetch.is_fetching();
        let screen_busy = self
            .tabs
            .screen(tab_id)
            .is_some_and(|screen| screen.is_network_op_in_flight());
        if network_op_active || screen_busy {
            return Task::none();
        }
        let Some(screen) = self.tabs.screen(tab_id) else {
            return Task::none();
        };
        if let Some(handle) = self.reload_refs_abort.take() {
            handle.abort();
        }
        let (task, handle) = screen.reload_refs_task().abortable();
        self.reload_refs_abort = Some(handle);
        task
    }
}

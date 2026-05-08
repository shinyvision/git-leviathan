//! App-level fetch orchestration: kicking off user/auto fetches and
//! file-watcher-driven refs reloads. Works on top of `FetchPolicy` (which
//! owns the single-slot state) by wiring it to the active-tab screen.
//! Extracted from `update.rs` in keymaps.

use iced::Task;
use std::path::PathBuf;

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
        if !self.git_queue.is_empty() || self.repository_write_in_flight() {
            return Task::none();
        }
        self.start_fetch_for_tab(tab_id)
    }

    pub(super) fn start_fetch_for_tab(&mut self, tab_id: TabId) -> Task<Message> {
        if self.fetch.is_fetching() {
            return Task::none();
        }
        let Some(screen) = self.tabs.screen_mut(tab_id) else {
            return Task::none();
        };
        let Some(operation_id) = screen.begin_git_fetch() else {
            return Task::none();
        };
        let task = self
            .fetch
            .start(tab_id, operation_id, screen.fetch_task(operation_id));
        self.tabs.persist_most_recent_if_needed(tab_id);
        self.plugin_host
            .fire_event_typed("FetchStarted", Self::fetch_all_remotes_payload());
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
    pub(super) fn reload_refs_for_tab(&mut self, tab_id: TabId, path: PathBuf) -> Task<Message> {
        let span = crate::perf::Span::new("ui.file_watcher_reload_task").field("tab", tab_id);
        let Some(screen) = self.tabs.screen(tab_id) else {
            span.finish_with("outcome", "missing_tab");
            return Task::none();
        };
        if screen.active_worktree_path() != path {
            span.finish_with("outcome", "stale_path");
            return Task::none();
        }
        let network_op_active = tab_id == self.tabs.active_tab_id() && self.fetch.is_fetching();
        let screen_busy = screen.is_git_write_in_flight();
        if network_op_active || screen_busy {
            if let Some(screen) = self.tabs.screen_mut(tab_id) {
                screen.mark_watcher_reload_pending(path);
            }
            span.finish_with("outcome", "suppressed");
            return Task::none();
        }
        if let Some(handle) = self.reload_refs_abort.take() {
            handle.abort();
        }
        let (task, handle) = screen.reload_refs_task().abortable();
        self.reload_refs_abort = Some(handle);
        span.finish_with("outcome", "created");
        task
    }
}

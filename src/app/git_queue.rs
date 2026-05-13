use std::collections::VecDeque;

use iced::Task;

use crate::core::TabId;
use crate::message::Message;
use crate::plugin::ui::focus::FocusReason;
use crate::screens::repository::RepositoryMessage;

use super::fetch_policy::FetchCancellation;
use super::App;

#[derive(Debug, Default)]
pub(crate) struct GitOperationQueue {
    pending: VecDeque<QueuedGitOperation>,
}

#[derive(Debug, Clone)]
pub(super) enum QueuedGitOperation {
    Repository {
        tab_id: TabId,
        message: Box<RepositoryMessage>,
    },
}

impl GitOperationQueue {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub(super) fn push_back_repository(&mut self, tab_id: TabId, message: RepositoryMessage) {
        self.pending.push_back(QueuedGitOperation::Repository {
            tab_id,
            message: Box::new(message),
        });
    }

    pub(super) fn pop_front(&mut self) -> Option<QueuedGitOperation> {
        self.pending.pop_front()
    }
}

impl App {
    pub(super) fn update_repository_message_for_tab(
        &mut self,
        tab_id: TabId,
        message: RepositoryMessage,
    ) -> Task<Message> {
        if matches!(message, RepositoryMessage::FetchRequested) {
            return self.try_start_fetch_for_tab(tab_id);
        }
        if matches!(message, RepositoryMessage::FocusPanel(_)) {
            self.pending_focus_reason = Some(FocusReason::Keymap);
        }
        match message.git_write_intent() {
            Some(_) => self.request_git_operation(tab_id, message),
            None => self.dispatch_repository_message(tab_id, message),
        }
    }

    pub(super) fn dispatch_repository_message(
        &mut self,
        tab_id: TabId,
        message: RepositoryMessage,
    ) -> Task<Message> {
        match self.tabs.screen_mut(tab_id) {
            Some(screen) => screen.update(message),
            None => Task::none(),
        }
    }

    fn request_git_operation(
        &mut self,
        tab_id: TabId,
        message: RepositoryMessage,
    ) -> Task<Message> {
        let _ = self.cancel_active_fetch();

        if self.git_operation_in_flight() {
            self.git_queue.push_back_repository(tab_id, message);
            return Task::none();
        }

        if self.git_queue.is_empty() {
            self.dispatch_repository_message(tab_id, message)
        } else {
            self.git_queue.push_back_repository(tab_id, message);
            self.drain_git_operation_queue()
        }
    }

    pub(super) fn drain_git_operation_queue(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        while !self.git_operation_in_flight() {
            let Some(operation) = self.git_queue.pop_front() else {
                break;
            };
            let task = match operation {
                QueuedGitOperation::Repository { tab_id, message } => {
                    if self.tabs.screen(tab_id).is_none() {
                        continue;
                    }
                    self.dispatch_repository_message(tab_id, *message)
                }
            };
            tasks.push(task);
        }
        Task::batch(tasks)
    }

    pub(super) fn git_operation_in_flight(&self) -> bool {
        self.fetch.is_fetching() || self.repository_write_in_flight()
    }

    pub(super) fn repository_write_in_flight(&self) -> bool {
        self.tabs.tabs().iter().any(|tab| {
            self.tabs
                .screen(tab.id)
                .is_some_and(|screen| screen.is_git_write_in_flight())
        })
    }

    pub(super) fn cancel_active_fetch(&mut self) -> Option<FetchCancellation> {
        let cancelled = self.fetch.cancel_active_fetch()?;
        self.finish_cancelled_fetch(cancelled);
        Some(cancelled)
    }

    pub(super) fn cancel_fetch_and_debounce(&mut self) -> Option<FetchCancellation> {
        let cancelled = self.fetch.cancel()?;
        self.finish_cancelled_fetch(cancelled);
        Some(cancelled)
    }

    fn finish_cancelled_fetch(&mut self, cancelled: FetchCancellation) {
        crate::services::kill_running_fetch_processes();
        if let Some(screen) = self.tabs.screen_mut(cancelled.tab_id) {
            let _ = screen.finish_git_write(cancelled.operation_id);
        }
        self.plugin_host
            .fire_event_typed("FetchFinished", Self::fetch_all_remotes_payload());
    }
}

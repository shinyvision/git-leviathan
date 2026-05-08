use std::collections::VecDeque;

use iced::Task;

use crate::core::TabId;
use crate::message::Message;
use crate::screens::repository::{GitWriteIntent, RepositoryMessage};

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

    pub(super) fn push_front_repository(&mut self, tab_id: TabId, message: RepositoryMessage) {
        self.pending.push_front(QueuedGitOperation::Repository {
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
        match message.git_write_intent() {
            Some(intent) => self.request_git_operation(tab_id, message, intent),
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
        intent: GitWriteIntent,
    ) -> Task<Message> {
        if intent == GitWriteIntent::FastLocal
            && self.only_fetch_in_flight()
            && self.cancel_active_fetch().is_some()
        {
            if self.git_queue.is_empty() {
                return self.dispatch_repository_message(tab_id, message);
            }
            self.git_queue.push_front_repository(tab_id, message);
            return self.drain_git_operation_queue();
        }

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

    fn only_fetch_in_flight(&self) -> bool {
        let Some(active_fetch) = self.fetch.active() else {
            return false;
        };
        self.tabs.tabs().iter().all(|tab| {
            let Some(screen) = self.tabs.screen(tab.id) else {
                return true;
            };
            !screen.is_git_write_in_flight() || tab.id == active_fetch.tab_id
        })
    }

    fn finish_cancelled_fetch(&mut self, cancelled: FetchCancellation) {
        if let Some(screen) = self.tabs.screen_mut(cancelled.tab_id) {
            let _ = screen.finish_git_write(cancelled.operation_id);
        }
        self.plugin_host
            .fire_event_typed("FetchFinished", Self::fetch_all_remotes_payload());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_insert_runs_before_existing_work() {
        let mut queue = GitOperationQueue::new();
        queue.push_back_repository(TabId(1), RepositoryMessage::PullRequested);
        queue.push_front_repository(TabId(3), RepositoryMessage::PushRequested);

        assert!(matches!(
            queue.pop_front(),
            Some(QueuedGitOperation::Repository {
                tab_id: TabId(3),
                ..
            })
        ));
        assert!(matches!(
            queue.pop_front(),
            Some(QueuedGitOperation::Repository {
                tab_id: TabId(1),
                ..
            })
        ));
    }
}

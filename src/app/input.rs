//! Keyboard-input dispatch for `App`. Extracted from `update.rs` in keymaps.

use iced::{keyboard, Task};

use crate::message::Message;
use crate::screens::{BlankMessage, Screen};

use super::{App, FETCH_DEBOUNCE_AFTER_TAB_SWITCH};

impl App {
    pub(super) fn handle_key_pressed(
        &mut self,
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> Task<Message> {
        let is_ctrl_tab =
            modifiers.control() && matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab));

        if is_ctrl_tab {
            let target = if modifiers.shift() {
                self.tabs.prev_tab_id_circular()
            } else {
                self.tabs.next_tab_id_circular()
            };
            let Some(target) = target else {
                return Task::none();
            };
            if self.tabs.is_empty() {
                return Task::none();
            }
            let was_fetching = self.fetch.is_fetching();
            let finished_remote = if was_fetching {
                Some(self.fetch_remote_name_for_tab(self.tabs.active_tab_id()))
            } else {
                None
            };
            self.fetch.cancel();
            if let Some(remote_name) = finished_remote {
                self.plugin_host
                    .fire_event_typed("FetchFinished", Self::fetch_remote_payload(remote_name));
            }
            let screen_task = self.tabs.activate_tab(target);
            let debounce_task = self
                .fetch
                .schedule_debounced(FETCH_DEBOUNCE_AFTER_TAB_SWITCH);
            return Task::batch(vec![screen_task, debounce_task]);
        }

        if self.no_git_screen.is_some() {
            return Task::none();
        }
        if self.tabs.is_empty() {
            return self
                .blank_screen
                .update(BlankMessage::KeyPressed(key, modifiers));
        }
        self.tabs
            .active_screen_mut()
            .and_then(|screen| screen.handle_key_pressed(key, modifiers))
            .unwrap_or(Task::none())
    }
}

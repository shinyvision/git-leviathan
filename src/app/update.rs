//! Top-level update dispatchers for `App`. Extracted from `mod.rs` in keymaps.
//! Individual handler methods live in sibling modules (`input`, `commands`,
//! `fetch_ops`, `animation`); this file only routes.

use iced::Task;

use crate::message::{AppMessage, Message, ScreenMessage, ScreenRouted, ToastMessage};
use crate::plugin::PluginMessage;
use crate::screens::repository::RepositoryMessage;
use crate::screens::Screen;
use crate::services::detect_git;

use super::{commands::open_url, App};

impl App {
    pub(super) fn update_plugin(&mut self, msg: PluginMessage) -> Task<Message> {
        match msg {
            PluginMessage::SlotClicked {
                plugin_id,
                region,
                container,
                slot_id,
                event,
                value,
            } => {
                self.plugin_host
                    .dispatch_slot_click(&plugin_id, &region, &container, &slot_id, &event, value);
            }
            PluginMessage::Event {
                plugin_id,
                screen_id,
                event,
                value,
            } => {
                self.plugin_host
                    .dispatch_event(&plugin_id, &screen_id, &event, value);
            }
            PluginMessage::OverlayEvent {
                plugin_id,
                overlay_id,
                event,
                value,
            } => {
                self.plugin_host
                    .dispatch_overlay_event(&plugin_id, &overlay_id, &event, value);
            }
            PluginMessage::RepositoryDialogButtonPressed {
                plugin_id,
                dialog_id,
                button_id,
            } => {
                self.plugin_host
                    .dispatch_dialog_button_callback(&plugin_id, &dialog_id, &button_id);
            }
            PluginMessage::SplitDragBegin {
                split_key,
                divider_index,
                child_count,
                is_vertical,
                limits,
            } => {
                self.plugin_host.split_drag_begin(
                    &split_key,
                    divider_index,
                    child_count,
                    is_vertical,
                    limits,
                );
            }
            PluginMessage::SplitDragged { x, y } => {
                self.plugin_host.split_drag_moved(x, y);
            }
            PluginMessage::SplitDragReleased => {
                self.plugin_host.split_drag_released();
            }
        }
        Task::batch(vec![
            self.drain_pending_navigation_effects(),
            self.focus_plugin_overlay_input(),
        ])
    }

    pub(super) fn update_app(&mut self, msg: AppMessage) -> Task<Message> {
        match msg {
            AppMessage::NoOp => Task::none(),
            AppMessage::CopyToClipboard(text) => iced::clipboard::write::<Message>(text),
            AppMessage::OpenUrl(url) => {
                open_url(&url);
                Task::none()
            }
            AppMessage::GitRecheckRequested => {
                let status = detect_git();
                if status.is_available() {
                    self.no_git_screen = None;
                    return self.load_initial_repos();
                } else if let Some(screen) = self.no_git_screen.as_mut() {
                    screen.set_status(status);
                }
                Task::none()
            }
            AppMessage::ShutdownRequested => {
                crate::services::kill_running_git_processes();
                std::process::exit(0);
            }
            AppMessage::KeyPressed {
                key,
                modified_key,
                modifiers,
            } => self.handle_key_pressed(key, modified_key, modifiers),
            AppMessage::ModifiersChanged(modifiers) => {
                if self.no_git_screen.is_some() || self.tabs.is_empty() {
                    return Task::none();
                }
                if self.tabs.active_plugin_screen().is_some() {
                    return Task::none();
                }
                if let Some(screen) = self.tabs.active_screen_mut() {
                    screen.on_modifiers_changed(modifiers);
                }
                Task::none()
            }
            AppMessage::OpenRepoDialog => self.open_repo_dialog(),
            AppMessage::RepoPathChosen(Some(path)) => self.open_repo_from_path(path),
            AppMessage::RepoPathChosen(None) => Task::none(),
            AppMessage::InvokeCommand { id, args } => {
                let _ = self.plugin_host.invoke_command(&id, args);
                Task::none()
            }
            AppMessage::SyntaxGrammarCommandFinished(outcome) => {
                self.handle_syntax_grammar_command_finished(outcome)
            }
            AppMessage::EagerGrammarInstallRequested(language) => {
                self.eager_install_grammar(language)
            }
            AppMessage::TabRegistryOp(op) => {
                if let crate::plugin::tab_snapshot::TabRegistryOp::Select(ref path) = op {
                    if self.tabs.tab_id_for_path(path) == Some(self.tabs.active_tab_id()) {
                        return Task::none();
                    }
                }
                let is_select = matches!(op, crate::plugin::tab_snapshot::TabRegistryOp::Select(_));
                let screen_task = self.apply_tab_registry_op(op).unwrap_or_else(Task::none);
                if is_select {
                    Task::batch(vec![screen_task, self.try_start_fetch()])
                } else {
                    screen_task
                }
            }
            AppMessage::TabOpened { tab_id, result } => self.handle_tab_opened(tab_id, result),
            AppMessage::AnimationTick(timestamp) => self.handle_animation_tick(timestamp),
            AppMessage::PluginRuntimeTick(timestamp) => {
                let _ = timestamp;
                self.plugin_host.tick();
                Task::none()
            }
            AppMessage::RepoFilesChanged { tab_id, path } => self.reload_refs_for_tab(tab_id, path),
            AppMessage::WindowFocused => self.on_window_focused(),
            AppMessage::WindowUnfocused => {
                self.key_chord.clear();
                Task::none()
            }
            AppMessage::FetchTick(_) => {
                if !self.tabs.is_empty() {
                    self.try_start_fetch()
                } else {
                    Task::none()
                }
            }
            AppMessage::FetchDebounceElapsed => {
                self.fetch.on_debounce_elapsed();
                if !self.tabs.is_empty() {
                    self.try_start_fetch()
                } else {
                    Task::none()
                }
            }
            AppMessage::FetchCompleted {
                tab_id,
                operation_id,
                result,
            } => {
                if self.fetch.on_completed(tab_id, operation_id) {
                    self.plugin_host
                        .fire_event_typed("FetchFinished", Self::fetch_all_remotes_payload());
                }
                if let Some(screen) = self.tabs.screen_mut(tab_id) {
                    return screen.update(RepositoryMessage::FetchFinished {
                        operation_id,
                        result,
                    });
                }
                Task::none()
            }
        }
    }

    pub(super) fn update_screen(&mut self, routed: ScreenRouted) -> Task<Message> {
        match routed {
            ScreenRouted::Active(sm) => match sm {
                ScreenMessage::Blank(bm) => self.blank_screen.update(bm),
                ScreenMessage::NoGit(ngm) => match self.no_git_screen.as_mut() {
                    Some(screen) => screen.update(ngm),
                    None => Task::none(),
                },
                ScreenMessage::Repository(rm) => {
                    if self.no_git_screen.is_some() || self.tabs.is_empty() {
                        return Task::none();
                    }
                    self.update_repository_message_for_tab(self.tabs.active_tab_id(), *rm)
                }
            },
            ScreenRouted::Tab(tab_id, rm) => {
                // File-watcher coalescer (see `reload_refs_for_tab`) holds an
                // abortable handle; clear it once the scoped reload lands so
                // the next burst isn't suppressed as "already in flight".
                if matches!(&*rm, RepositoryMessage::RefsReloaded(_)) {
                    self.reload_refs_abort = None;
                }
                self.update_repository_message_for_tab(tab_id, *rm)
            }
        }
    }

    pub(super) fn update_toast(&mut self, msg: ToastMessage) -> Task<Message> {
        match msg {
            ToastMessage::Show(data) => self.show_toast(data),
            ToastMessage::Dismiss(id) => {
                self.toasts.dismiss(id);
                Task::none()
            }
        }
    }
}

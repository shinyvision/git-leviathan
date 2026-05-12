//! Keyboard-input dispatch for `App`. Extracted from `update.rs` in keymaps.

use std::borrow::Cow;

use iced::{keyboard, Task};

use crate::message::Message;
use crate::plugin::extensions::OverlayRecord;
use crate::plugin::keymap::{KeymapDispatchOutcome, Keystroke};
use crate::screens::{BlankMessage, Screen};

use super::{App, FETCH_DEBOUNCE_AFTER_TAB_SWITCH};

impl App {
    pub(super) fn handle_key_pressed(
        &mut self,
        key: keyboard::Key,
        modified_key: keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> Task<Message> {
        let is_ctrl_tab =
            modifiers.control() && matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab));

        if is_ctrl_tab {
            self.key_chord.clear();
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
            let _ = self.cancel_fetch_and_debounce();
            let screen_task = self.tabs.activate_tab(target);
            let debounce_task = self
                .fetch
                .schedule_debounced(FETCH_DEBOUNCE_AFTER_TAB_SWITCH);
            return Task::batch(vec![screen_task, debounce_task]);
        }

        if is_escape_key(&key) {
            if let Some(screen) = self.tabs.active_screen_mut() {
                if let Some(task) = screen.handle_overlay_key_pressed(&key, modifiers) {
                    self.key_chord.clear();
                    return task;
                }
            }
        }

        if is_escape_key(&key) && self.key_chord.is_active() {
            self.key_chord.clear();
            return Task::none();
        }

        if is_escape_key(&key) {
            let top_overlay = self
                .plugin_host
                .extension_overlays()
                .into_iter()
                .find(|overlay| overlay.dismissible);
            if let Some(overlay) = top_overlay {
                self.plugin_host.dispatch_overlay_event(
                    &overlay.plugin_id,
                    &overlay.id,
                    "escape",
                    serde_json::Value::Null,
                );
                return self.focus_plugin_overlay_input();
            }
        }

        let overlays = self.plugin_host.extension_overlays();
        if let Some(event) = overlay_key_event(&overlays, &key, modifiers) {
            self.plugin_host.dispatch_overlay_event(
                &event.plugin_id,
                &event.overlay_id,
                "key",
                event.payload,
            );
            return self.focus_plugin_overlay_input();
        }

        if let Some(screen) = self.tabs.active_screen_mut() {
            if let Some(task) = screen.handle_overlay_key_pressed(&key, modifiers) {
                return task;
            }
        }

        if let Some(keystroke) = keystroke_from_iced_key(&modified_key, modifiers) {
            let context = self.keymap_context();
            if self.handle_keymap_candidate(context, keystroke) {
                return self.focus_plugin_overlay_input();
            }
        }

        if self.no_git_screen.is_some() {
            return Task::none();
        }
        if let Some(screen) = self.tabs.active_plugin_screen() {
            let msg = screen.handle_key_pressed(key, modifiers);
            return self.update_plugin(msg);
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

    fn handle_keymap_candidate(&mut self, context: String, keystroke: Keystroke) -> bool {
        if self.key_chord.context() != Some(context.as_str()) {
            self.key_chord.reset_for_context(context.clone());
        }

        self.key_chord.push(keystroke.clone());
        match self.dispatch_current_key_chord(&context) {
            KeymapDispatchOutcome::Dispatched { .. } | KeymapDispatchOutcome::Pending => true,
            KeymapDispatchOutcome::Unhandled => {
                let should_retry_current = self.key_chord.buffer().len() > 1;
                self.key_chord.clear();
                if !should_retry_current {
                    return false;
                }

                self.key_chord.replace_with(context.clone(), keystroke);
                match self.dispatch_current_key_chord(&context) {
                    KeymapDispatchOutcome::Dispatched { .. } | KeymapDispatchOutcome::Pending => {
                        true
                    }
                    KeymapDispatchOutcome::Unhandled => {
                        self.key_chord.clear();
                        false
                    }
                }
            }
        }
    }

    fn dispatch_current_key_chord(&mut self, context: &str) -> KeymapDispatchOutcome {
        let buffer = self.key_chord.buffer().to_vec();
        let outcome = self.plugin_host.dispatch_key(context, &buffer);
        if matches!(outcome, KeymapDispatchOutcome::Dispatched { .. }) {
            self.key_chord.clear();
        }
        outcome
    }

    fn keymap_context(&self) -> String {
        if let Some(screen) = self.tabs.active_plugin_screen() {
            format!("plugin_screen:{}", screen.screen_id())
        } else if self.no_git_screen.is_none() {
            self.tabs
                .active_screen()
                .map(|screen| screen.keymap_context().to_string())
                .unwrap_or_else(|| "global".to_string())
        } else {
            "global".to_string()
        }
    }

    pub(super) fn focus_plugin_overlay_input(&self) -> Task<Message> {
        let Some(id) = self.plugin_host.autofocus_overlay_text_input_id() else {
            return Task::none();
        };
        Task::batch([
            iced::widget::operation::focus(id.clone()),
            iced::widget::operation::move_cursor_to_end(id),
        ])
    }
}

struct OverlayKeyEvent {
    plugin_id: String,
    overlay_id: String,
    payload: serde_json::Value,
}

fn overlay_key_event(
    overlays: &[OverlayRecord],
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
) -> Option<OverlayKeyEvent> {
    let key_name = overlay_key_name_from_iced_key(key)?;
    let top_overlay = overlays.first()?;
    if !top_overlay.listens_for_key(key_name.as_ref()) {
        return None;
    }

    Some(OverlayKeyEvent {
        plugin_id: top_overlay.plugin_id.clone(),
        overlay_id: top_overlay.id.clone(),
        payload: serde_json::json!({
            "key": key_name.as_ref(),
            "ctrl": modifiers.control(),
            "shift": modifiers.shift(),
            "alt": modifiers.alt(),
            "logo": modifiers.logo(),
            "command": modifiers.command(),
        }),
    })
}

fn is_escape_key(key: &keyboard::Key) -> bool {
    matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape))
}

fn overlay_key_name_from_iced_key(key: &keyboard::Key) -> Option<Cow<'static, str>> {
    match key.as_ref() {
        keyboard::Key::Named(named) => named_key_name(named).map(Cow::Borrowed),
        keyboard::Key::Character(character) => {
            let mut chars = character.chars();
            let ch = chars.next()?;
            if chars.next().is_some() || ch.is_control() {
                return None;
            }
            Some(Cow::Owned(ch.to_ascii_lowercase().to_string()))
        }
        keyboard::Key::Unidentified => None,
    }
}

fn keystroke_from_iced_key(
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
) -> Option<Keystroke> {
    let mut keystroke = match key.as_ref() {
        keyboard::Key::Character(character) => keystroke_from_character(character, modifiers)?,
        keyboard::Key::Named(named) => Keystroke {
            key: named_key_name(named)?.to_string(),
            ctrl: modifiers.control(),
            shift: modifiers.shift(),
            alt: modifiers.alt(),
        },
        keyboard::Key::Unidentified => return None,
    };

    if keystroke.key.len() == 1 {
        let c = keystroke.key.as_bytes()[0] as char;
        if c.is_ascii_uppercase() {
            keystroke.key = c.to_ascii_lowercase().to_string();
            keystroke.shift = true;
        }
    }

    Some(keystroke)
}

fn keystroke_from_character(character: &str, modifiers: keyboard::Modifiers) -> Option<Keystroke> {
    if character == " " {
        return Some(Keystroke {
            key: "space".into(),
            ctrl: modifiers.control(),
            shift: modifiers.shift(),
            alt: modifiers.alt(),
        });
    }

    let mut chars = character.chars();
    let c = chars.next()?;
    if chars.next().is_some() || !c.is_ascii() || c.is_ascii_control() {
        return None;
    }

    Some(Keystroke {
        key: c.to_string(),
        ctrl: modifiers.control(),
        shift: c.is_ascii_alphabetic() && modifiers.shift(),
        alt: modifiers.alt(),
    })
}

fn named_key_name(named: keyboard::key::Named) -> Option<&'static str> {
    Some(match named {
        keyboard::key::Named::Enter => "enter",
        keyboard::key::Named::Escape => "esc",
        keyboard::key::Named::Tab => "tab",
        keyboard::key::Named::Space => "space",
        keyboard::key::Named::Backspace => "backspace",
        keyboard::key::Named::Delete => "delete",
        keyboard::key::Named::ArrowUp => "up",
        keyboard::key::Named::ArrowDown => "down",
        keyboard::key::Named::ArrowLeft => "left",
        keyboard::key::Named::ArrowRight => "right",
        keyboard::key::Named::Home => "home",
        keyboard::key::Named::End => "end",
        keyboard::key::Named::PageUp => "pageup",
        keyboard::key::Named::PageDown => "pagedown",
        keyboard::key::Named::F1 => "f1",
        keyboard::key::Named::F2 => "f2",
        keyboard::key::Named::F3 => "f3",
        keyboard::key::Named::F4 => "f4",
        keyboard::key::Named::F5 => "f5",
        keyboard::key::Named::F6 => "f6",
        keyboard::key::Named::F7 => "f7",
        keyboard::key::Named::F8 => "f8",
        keyboard::key::Named::F9 => "f9",
        keyboard::key::Named::F10 => "f10",
        keyboard::key::Named::F11 => "f11",
        keyboard::key::Named::F12 => "f12",
        keyboard::key::Named::F13 => "f13",
        keyboard::key::Named::F14 => "f14",
        keyboard::key::Named::F15 => "f15",
        keyboard::key::Named::F16 => "f16",
        keyboard::key::Named::F17 => "f17",
        keyboard::key::Named::F18 => "f18",
        keyboard::key::Named::F19 => "f19",
        keyboard::key::Named::F20 => "f20",
        keyboard::key::Named::F21 => "f21",
        keyboard::key::Named::F22 => "f22",
        keyboard::key::Named::F23 => "f23",
        keyboard::key::Named::F24 => "f24",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_shifted_colon_to_bare_punctuation() {
        let stroke = keystroke_from_iced_key(
            &keyboard::Key::Character(":".into()),
            keyboard::Modifiers::SHIFT,
        )
        .expect("colon should convert");

        assert_eq!(stroke, Keystroke::plain(":"));
    }

    #[test]
    fn converts_shifted_letter_with_shift_modifier() {
        let stroke = keystroke_from_iced_key(
            &keyboard::Key::Character("A".into()),
            keyboard::Modifiers::SHIFT,
        )
        .expect("letter should convert");

        assert_eq!(stroke, Keystroke::plain("a").with_shift(true));
    }

    #[test]
    fn converts_escape_to_keymap_name() {
        let stroke = keystroke_from_iced_key(
            &keyboard::Key::Named(keyboard::key::Named::Escape),
            keyboard::Modifiers::default(),
        )
        .expect("escape should convert");

        assert_eq!(stroke, Keystroke::plain("esc"));
    }

    fn overlay(id: &str, priority: i32, key_events: &[&str]) -> OverlayRecord {
        OverlayRecord {
            plugin_id: format!("plugin_{id}"),
            id: id.to_string(),
            priority,
            dismissible: true,
            key_events: key_events.iter().map(|key| key.to_string()).collect(),
            widget: crate::plugin::ui::widget_ast::decode(&serde_json::json!({
                "kind": "text",
                "value": "hi",
            }))
            .expect("decode widget"),
            source_location: None,
        }
    }

    #[test]
    fn overlay_key_event_routes_to_top_opted_in_overlay() {
        let overlays = vec![
            overlay("top", 100, &["tab"]),
            overlay("bottom", 0, &["down"]),
        ];

        let event = overlay_key_event(
            &overlays,
            &keyboard::Key::Named(keyboard::key::Named::Tab),
            keyboard::Modifiers::SHIFT,
        )
        .expect("tab should be captured");

        assert_eq!(event.plugin_id, "plugin_top");
        assert_eq!(event.overlay_id, "top");
        assert_eq!(event.payload["key"], "tab");
        assert_eq!(event.payload["shift"], true);
    }

    #[test]
    fn overlay_key_event_does_not_route_to_lower_overlay() {
        let overlays = vec![
            overlay("top", 100, &["tab"]),
            overlay("bottom", 0, &["down"]),
        ];

        let event = overlay_key_event(
            &overlays,
            &keyboard::Key::Named(keyboard::key::Named::ArrowDown),
            keyboard::Modifiers::default(),
        );

        assert!(event.is_none());
    }

    #[test]
    fn overlay_key_event_routes_opted_in_character_keys() {
        let overlays = vec![overlay("top", 100, &["j"])];
        let event = overlay_key_event(
            &overlays,
            &keyboard::Key::Character("j".into()),
            keyboard::Modifiers::default(),
        )
        .expect("j should be captured");

        assert_eq!(event.plugin_id, "plugin_top");
        assert_eq!(event.overlay_id, "top");
        assert_eq!(event.payload["key"], "j");
    }

    #[test]
    fn overlay_key_event_ignores_unlisted_character_keys() {
        let overlays = vec![overlay("top", 100, &["k"])];
        let event = overlay_key_event(
            &overlays,
            &keyboard::Key::Character("j".into()),
            keyboard::Modifiers::default(),
        );

        assert!(event.is_none());
    }
}

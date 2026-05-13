use crate::plugin::keymap::{self as keymap_mod, render_chord, Keystroke};

use super::App;

#[derive(Debug, Default)]
pub(super) struct KeyChordState {
    context: Option<String>,
    buffer: Vec<Keystroke>,
    published_prefix: bool,
}

impl KeyChordState {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn is_active(&self) -> bool {
        !self.buffer.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.context = None;
        self.buffer.clear();
        self.published_prefix = false;
    }

    pub(super) fn reset_for_context(&mut self, context: String) {
        self.context = Some(context);
        self.buffer.clear();
        self.published_prefix = false;
    }

    pub(super) fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    pub(super) fn buffer(&self) -> &[Keystroke] {
        &self.buffer
    }

    pub(super) fn prefix_published(&self) -> bool {
        self.published_prefix
    }

    pub(super) fn mark_prefix_published(&mut self) {
        self.published_prefix = true;
    }

    pub(super) fn push(&mut self, stroke: Keystroke) {
        self.buffer.push(stroke);
    }

    pub(super) fn replace_with(&mut self, context: String, stroke: Keystroke) {
        self.context = Some(context);
        self.buffer.clear();
        self.buffer.push(stroke);
    }
}

impl App {
    pub(super) fn clear_key_chord(&mut self, reason: &'static str) {
        let was_published = self.key_chord.prefix_published();
        let context = self.key_chord.context().unwrap_or("global").to_string();
        let prefix = render_chord(self.key_chord.buffer());
        self.key_chord.clear();
        if was_published {
            let payload = keymap_mod::keymap_prefix_changed_payload(
                false,
                &context,
                &prefix,
                Vec::new(),
                reason,
            );
            self.plugin_host
                .fire_event_typed("KeymapPrefixChanged", payload);
        }
    }

    pub(super) fn publish_key_chord_pending(&mut self, context: &str, reason: &'static str) {
        if !self.key_chord.is_active() {
            return;
        }
        let buffer = self.key_chord.buffer().to_vec();
        let prefix = render_chord(&buffer);
        let hints = {
            let registry = self.plugin_host.keymap_registry();
            let borrowed = registry.borrow();
            borrowed.prefix_hints(context, &buffer)
        };
        let payload =
            keymap_mod::keymap_prefix_changed_payload(true, context, &prefix, hints, reason);
        self.key_chord.mark_prefix_published();
        self.plugin_host
            .fire_event_typed("KeymapPrefixChanged", payload);
    }
}

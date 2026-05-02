//! Plugin-routed messages. Added to root `Message` as `Message::Plugin(_)`.
//!
//! `Clone + Send + 'static` so they fit the iced/tokio task graph. Payloads are
//! `serde_json::Value` rather than `mlua::Value` because Lua values are `!Send`
//! and not Clone across runtime boundaries — the host serialises on entry and
//! deserialises on dispatch.

#[derive(Debug, Clone)]
pub enum PluginMessage {
    /// A widget inside a plugin-contributed slot fired an event. The
    /// host's slot-handler map is keyed by `(plugin_id, region,
    /// container, slot_id)`. `event` is the widget-supplied string
    /// (e.g. `on_click`, `on_select`); `value` is its payload. Plain
    /// buttons that only set `on_click` produce `event = ""` and
    /// `value = Null` — the slot's Lua handler can ignore the extra
    /// args and stays backward-compatible.
    SlotClicked {
        plugin_id: String,
        region: String,
        container: String,
        slot_id: String,
        event: String,
        value: serde_json::Value,
    },
    /// A widget inside a plugin screen emitted an event (e.g. button press).
    Event {
        plugin_id: String,
        screen_id: String,
        event: String,
        value: serde_json::Value,
    },
    /// User pressed down on a split divider. Screen captures initial state.
    /// `limits` is `(min, max)` per child, extracted at render time from each
    /// child's `container` min/max fields. Length matches `child_count`.
    SplitDragBegin {
        split_key: String,
        divider_index: usize,
        child_count: usize,
        is_vertical: bool,
        limits: Vec<(f32, f32)>,
    },
    /// Pointer moved while a split drag is in progress. Carries both axes;
    /// the host picks the relevant one based on the active drag's direction.
    /// (Needed because iced's `event::listen_with` takes a plain `fn`, so the
    /// subscription can't capture `is_vertical` at build time.)
    SplitDragged { x: f32, y: f32 },
    /// Mouse released during a split drag. Clears drag state.
    SplitDragReleased,
}

//! Animation-tick driver for `App`. The iced runtime fires `AnimationTick`
//! whenever any active component is animating (see `subscription.rs`); this
//! module spreads the tick across the toast manager and the active screen,
//! and resets the idle-clock when nothing is animating.
//! Extracted from `update.rs` in keymaps.

use std::time::Instant;

use iced::Task;

use crate::message::Message;
use crate::screens::Screen;

use super::App;

/// One 60fps frame, used when there's no previous tick to diff against.
const DEFAULT_FRAME_DELTA_MS: f32 = 16.0;
/// Clamp bounds guard against scheduler stalls (a long gap producing one huge
/// delta) and coalesced ticks (sub-frame deltas), keeping animation steps sane.
const MIN_FRAME_DELTA_MS: f32 = 1.0;
const MAX_FRAME_DELTA_MS: f32 = 32.0;

impl App {
    pub(super) fn handle_animation_tick(&mut self, timestamp: Instant) -> Task<Message> {
        let delta_ms = self
            .last_animation_tick
            .map(|last| timestamp.saturating_duration_since(last).as_secs_f32() * 1000.0)
            .unwrap_or(DEFAULT_FRAME_DELTA_MS)
            .clamp(MIN_FRAME_DELTA_MS, MAX_FRAME_DELTA_MS);
        self.last_animation_tick = Some(timestamp);
        self.toasts.tick(delta_ms);
        if self.no_git_screen.is_some() || self.tabs.is_empty() {
            return Task::none();
        }
        self.tabs
            .active_screen_mut()
            .and_then(|screen| screen.tick_animation(delta_ms))
            .unwrap_or(Task::none())
    }

    /// Called at the end of every `App::update` to drop the animation clock
    /// once nothing is animating — otherwise the next tick would compute a
    /// stale `delta_ms` from an unrelated earlier timestamp.
    pub(super) fn reset_animation_clock_if_idle(&mut self) {
        if self.tabs.is_empty() {
            if !self.toasts.is_animating() {
                self.last_animation_tick = None;
            }
            return;
        }
        let screen_animating = self
            .tabs
            .active_screen()
            .map(|s| s.is_animating())
            .unwrap_or(false);
        if !screen_animating && !self.toasts.is_animating() {
            self.last_animation_tick = None;
        }
    }
}

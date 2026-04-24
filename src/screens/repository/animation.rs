//! Screen-level animation tick + animation-active predicate.
//!
//! `tick` drives the per-frame pulls: selection-drag auto-scroll, overlay
//! slide transitions, and deferred focus requests from commit/text search.
//! `is_animating` decides whether the screen should keep requesting frames;
//! returning `true` here keeps `App::tick_animation` rolling each 16ms.

use iced::Task;

use crate::message::Message;

use super::{commit_search::commit_search_input_id, panels, RepositoryScreen};

pub(super) fn tick(screen: &mut RepositoryScreen, delta_ms: f32) -> Option<Task<Message>> {
    if let Some(task) = panels::diff::auto_scroll_tick(
        &mut screen.panels.diff,
        screen.input.last_pointer_position,
        delta_ms,
    ) {
        return Some(task);
    }
    if let Some(task) = screen.overlay_manager.tick_animation(delta_ms) {
        return Some(task);
    }
    if let Some(search) = screen.data.commit_search.as_mut() {
        if search.take_needs_focus() {
            return Some(iced::widget::operation::focus(commit_search_input_id()));
        }
    }
    if let Some(w) = screen.panels.diff.text_search.as_mut() {
        if w.take_needs_focus() {
            return Some(iced::widget::operation::focus(
                crate::widgets::search_widget::input_id(w.id()),
            ));
        }
    }
    None
}

pub(super) fn is_animating(screen: &RepositoryScreen) -> bool {
    screen.overlay_manager.is_animating()
        || screen.data.animation.network_op_in_flight()
        || (screen.panels.diff.is_diff_dragging()
            && screen.panels.diff.diff_viewport_rect.is_some())
        || screen
            .data
            .commit_search
            .as_ref()
            .is_some_and(|s| s.needs_focus())
        || screen
            .panels
            .diff
            .text_search
            .as_ref()
            .is_some_and(|w| w.needs_focus())
}

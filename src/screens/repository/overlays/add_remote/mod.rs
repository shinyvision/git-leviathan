//! "Add remote" side panel — slides in from the left of the main area with
//! its own easing animation (separate from the sliding-toolbar dialogs).

use std::time::Instant;

mod styles;
mod view;

pub(crate) use view::overlay_layers;

pub const PANEL_WIDTH: f32 = 400.0;
pub const ENTER_OFFSET: f32 = 400.0;
const SLIDE_DURATION_MS: f32 = 400.0;

pub(crate) fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("add-remote-name-input")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Opening,
    Closing,
}

pub(crate) struct State {
    pub name: String,
    pub pull_url: String,
    pub push_url: String,
    pub animation_start: Instant,
    pub direction: Direction,
    pub needs_focus: bool,
    pub submitting: bool,
}

impl State {
    pub(crate) fn new() -> Self {
        Self {
            name: String::new(),
            pull_url: String::new(),
            push_url: String::new(),
            animation_start: Instant::now(),
            direction: Direction::Opening,
            needs_focus: true,
            submitting: false,
        }
    }

    pub(crate) fn can_submit(&self) -> bool {
        !self.submitting && !self.name.trim().is_empty() && !self.pull_url.trim().is_empty()
    }

    pub(crate) fn slide_offset(&self) -> f32 {
        let elapsed_ms = self.animation_start.elapsed().as_millis() as f32;
        let t = (elapsed_ms / SLIDE_DURATION_MS).min(1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        match self.direction {
            Direction::Opening => ENTER_OFFSET * (1.0 - eased),
            Direction::Closing => ENTER_OFFSET * eased,
        }
    }

    pub(crate) fn is_animation_done(&self) -> bool {
        self.animation_start.elapsed().as_millis() >= SLIDE_DURATION_MS as u128
    }

    pub(crate) fn start_close(&mut self) {
        self.direction = Direction::Closing;
        self.animation_start = Instant::now();
    }
}

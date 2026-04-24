//! Shared button styles for main-bar slots.
//!
//! DRY: three variants previously copy-pasted inline in `main_bar.rs` are
//! collapsed here. Each function is a `button::StyleFn`-compatible
//! `Fn(&Theme, button::Status) -> button::Style`.

use iced::{
    widget::button::{Status, Style},
    Border, Theme,
};

use crate::theme;

/// Flat toolbar action button (Pull, Push, Branch, Stash, Pop).
pub fn action_btn(_: &Theme, status: Status) -> Style {
    Style {
        background: hover_background(status),
        text_color: theme::TEXT_PRIMARY,
        border: Border::default(),
        shadow: Default::default(),
        snap: false,
    }
}

/// Square search button on the right edge — 6px corner radius, no border.
pub fn search_btn(_: &Theme, status: Status) -> Style {
    Style {
        background: hover_background(status),
        text_color: theme::TEXT_PRIMARY,
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        shadow: Default::default(),
        snap: false,
    }
}

fn hover_background(status: Status) -> Option<iced::Background> {
    match status {
        Status::Hovered => Some(theme::BG_HOVER.into()),
        _ => None,
    }
}

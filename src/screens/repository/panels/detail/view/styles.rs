//! Shared button / editor styles for the detail panel.
//!
//! Extracted so dirty-files, reword, and single-commit views can reuse the
//! same palette without duplicating the style builders. The generic
//! palette→style mapping lives in `overlays::widgets::palette_button_style`;
//! these wrappers pick the right palette for each tone.

use iced::{
    widget::{button, text_editor},
    Border, Theme,
};

use crate::{
    screens::repository::overlays::widgets::{
        palette_button_style, CREATE_BUTTON, DANGER_BUTTON, RESOLVE_BUTTON,
    },
    theme,
};

pub(super) fn detail_text_editor_style(_: &Theme, _: text_editor::Status) -> text_editor::Style {
    text_editor::Style {
        background: theme::BG_BASE.into(),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 3.0.into(),
        },
        placeholder: theme::TEXT_DIM,
        value: theme::TEXT_PRIMARY,
        selection: theme::ACCENT_BLUE,
    }
}

pub(super) fn green_button_style(
    enabled: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    palette_button_style(CREATE_BUTTON, enabled)
}

pub(super) fn red_button_style(enabled: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    palette_button_style(DANGER_BUTTON, enabled)
}

pub(super) fn resolve_button_style(
    enabled: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    palette_button_style(RESOLVE_BUTTON, enabled)
}

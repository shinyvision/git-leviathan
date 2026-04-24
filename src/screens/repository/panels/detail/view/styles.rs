//! Shared button / editor styles for the detail panel. The generic
//! palette→style mapping lives in `overlays::widgets::palette_button_style`;
//! these wrappers pick the right palette for each tone.

use iced::{
    widget::{button, text_editor},
    Border, Color, Theme,
};

use crate::{
    screens::repository::overlays::widgets::{
        palette_button_style, OverlayButtonPalette, CREATE_BUTTON, DANGER_BUTTON,
    },
    theme,
};

const RESOLVE_BUTTON: OverlayButtonPalette = OverlayButtonPalette {
    background: Color {
        r: 0.549,
        g: 0.392,
        b: 0.118,
        a: 1.0,
    },
    hover_background: Color {
        r: 0.890,
        g: 0.658,
        b: 0.184,
        a: 1.0,
    },
    border: Color {
        r: 0.890,
        g: 0.658,
        b: 0.184,
        a: 1.0,
    },
    text: theme::TEXT_PRIMARY,
    hover_text: Color::WHITE,
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

pub(super) fn green_button_style(enabled: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    palette_button_style(CREATE_BUTTON, enabled)
}

pub(super) fn red_button_style() -> impl Fn(&Theme, button::Status) -> button::Style {
    palette_button_style(DANGER_BUTTON, true)
}

pub(super) fn resolve_button_style() -> impl Fn(&Theme, button::Status) -> button::Style {
    palette_button_style(RESOLVE_BUTTON, true)
}

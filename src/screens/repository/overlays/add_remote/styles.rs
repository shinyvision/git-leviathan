//! Style helpers extracted from the AddRemote view so the view stays under
//! the per-dialog LOC budget.

use iced::{
    widget::{button, text_input},
    Border, Theme,
};

use crate::theme;

use super::super::widgets::{palette_button_style, CREATE_BUTTON};

pub(super) fn input_style(_: &Theme, _: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: theme::BG_BASE.into(),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 3.0.into(),
        },
        icon: theme::TEXT_DIM,
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

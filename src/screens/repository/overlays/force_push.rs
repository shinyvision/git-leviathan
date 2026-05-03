//! Final destructive confirmation before force pushing a branch.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_cancel_button, overlay_row, sliding_main_bar_overlay, DANGER_BUTTON,
};

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub remote_name: String,
}

pub(crate) fn view(state: &State, slide_offset: f32) -> Element<'static, Message> {
    let label = text(format!(
        "Force push {} to {}. This cannot be undone.",
        state.branch_name, state.remote_name
    ))
    .size(theme::FONT_SM)
    .style(style::primary_text);

    let force_btn = overlay_button(
        "Force Push",
        DANGER_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::ForcePushConfirmed),
    );
    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::ForcePushCanceled,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![label.into(), force_btn, cancel_btn]),
        slide_offset,
    )
}

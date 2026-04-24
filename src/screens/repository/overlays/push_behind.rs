//! Bar shown when a push is rejected because the branch is behind its remote.
//! Offers pull (fast-forward), force push (with another confirmation), or cancel.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_cancel_button, overlay_row, sliding_main_bar_overlay, CREATE_BUTTON,
    DANGER_BUTTON,
};

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub remote_name: String,
}

pub(crate) fn view<'a>(state: &'a State, slide_offset: f32) -> Element<'a, Message> {
    let label = text(format!(
        "'{}' is behind '{}/{}' Update your branch by doing a Pull.",
        state.branch_name, state.remote_name, state.branch_name
    ))
    .size(theme::FONT_SM)
    .style(style::primary_text);

    let pull_btn = overlay_button(
        "Pull (fast-forward if possible)",
        CREATE_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::PushBehindPullRequested),
    );
    let force_btn = overlay_button(
        "Force Push",
        DANGER_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::PushBehindForcePushRequested),
    );
    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::PushBehindCanceled,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![label.into(), pull_btn, force_btn, cancel_btn]),
        slide_offset,
    )
}

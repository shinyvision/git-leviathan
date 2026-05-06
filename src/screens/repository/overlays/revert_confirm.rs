//! Revert-mode picker: commit immediately, apply in-place, or cancel.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_neutral_button, overlay_row, sliding_main_bar_overlay, CREATE_BUTTON,
    DANGER_BUTTON,
};

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub commit_hash: String,
}

pub(crate) fn view(slide_offset: f32) -> Element<'static, Message> {
    let label = text("Do you want to immediately commit the reverted changes?")
        .size(theme::FONT_SM)
        .style(style::primary_text);

    let yes_btn = overlay_button(
        "Yes",
        CREATE_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::RevertImmediateConfirmed),
    );
    let no_btn = overlay_neutral_button(
        "No",
        RepositoryMessage::OverlayPanel(OverlayPanelAction::RevertInPlaceConfirmed),
    );
    let cancel_btn = overlay_button(
        "Cancel",
        DANGER_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::RevertCanceled),
    );

    sliding_main_bar_overlay(
        overlay_row(vec![label.into(), yes_btn, no_btn, cancel_btn]),
        slide_offset,
    )
}

//! Final destructive confirmation before force pushing a branch.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_cancel_button, overlay_row, sliding_main_bar_overlay, DANGER_BUTTON,
};

#[derive(Debug, Clone)]
pub(crate) struct State {
    #[allow(dead_code)]
    pub branch_name: String,
    #[allow(dead_code)]
    pub remote_name: String,
}

pub(crate) fn view(slide_offset: f32) -> Element<'static, Message> {
    let label = text("Force push is a destructive action and cannot be undone.")
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

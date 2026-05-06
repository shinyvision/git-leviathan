//! Conflict choice for a file modified on one side and deleted on the other.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_cancel_button, overlay_row, sliding_main_bar_overlay, BROWSE_BUTTON,
    DANGER_BUTTON, RESOLVE_BUTTON,
};

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub path: String,
}

pub(crate) fn view(slide_offset: f32) -> Element<'static, Message> {
    let label = text("This file has been modified on one branch, but deleted on the other.")
        .size(theme::FONT_SM)
        .style(style::primary_text);

    let keep_modified = overlay_button(
        "Keep Modified Version",
        RESOLVE_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::ModifyDeleteKeepModified),
    );
    let delete_file = overlay_button(
        "Delete The File",
        DANGER_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::ModifyDeleteDeleteFile),
    );
    let keep_base = overlay_button(
        "Keep Base Version",
        BROWSE_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::ModifyDeleteKeepBase),
    );
    let cancel = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::ModifyDeleteCancel,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![
            label.into(),
            keep_modified,
            delete_file,
            keep_base,
            cancel,
        ]),
        slide_offset,
    )
}

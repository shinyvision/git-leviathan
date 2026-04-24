//! Destructive confirmation before deleting a local tag (and any remote copies).

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_cancel_button, overlay_row, sliding_main_bar_overlay, DANGER_BUTTON,
};

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub tag_name: String,
    pub tag_remote_names: Vec<String>,
}

pub(crate) fn view<'a>(state: &'a State, slide_offset: f32) -> Element<'a, Message> {
    let remote_suffix = if state.tag_remote_names.is_empty() {
        String::new()
    } else {
        format!(
            " (will also be deleted from {})",
            state.tag_remote_names.join(", ")
        )
    };
    let label = text(format!(
        "This is a destructive operation, are you sure you want to delete tag '{}'{}?",
        state.tag_name, remote_suffix
    ))
    .size(theme::FONT_SM)
    .style(style::primary_text);

    let delete_btn = overlay_button(
        "Delete",
        DANGER_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::DeleteTagConfirmed),
    );
    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::DeleteTagCanceled,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![label.into(), delete_btn, cancel_btn]),
        slide_offset,
    )
}

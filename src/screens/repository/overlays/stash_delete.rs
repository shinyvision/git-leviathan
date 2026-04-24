//! Destructive confirmation before dropping a stash entry.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_cancel_button, overlay_row, sliding_main_bar_overlay, DANGER_BUTTON,
};

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub stash_index: usize,
    pub display_name: String,
}

pub(crate) fn view<'a>(state: &'a State, slide_offset: f32) -> Element<'a, Message> {
    let label = text(format!(
        "This is a destructive operation, are you sure you want to delete stash '{}'?",
        state.display_name
    ))
    .size(theme::FONT_SM)
    .style(style::primary_text);

    let delete_btn = overlay_button(
        "Delete",
        DANGER_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::StashDeleteConfirmed),
    );
    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::StashDeleteCanceled,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![label.into(), delete_btn, cancel_btn]),
        slide_offset,
    )
}

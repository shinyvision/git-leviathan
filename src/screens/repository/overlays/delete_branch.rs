//! Destructive confirmation before deleting a local and/or remote branch.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_cancel_button, overlay_row, sliding_main_bar_overlay, DANGER_BUTTON,
};

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub is_remote: bool,
    pub has_remote: bool,
    /// Remote name for display (e.g. "origin").
    pub remote_name: Option<String>,
    /// Exact remote ref for remote-side deletion, e.g. "origin/feature".
    pub remote_ref: Option<String>,
}

pub(crate) fn view<'a>(state: &'a State, slide_offset: f32) -> Element<'a, Message> {
    let display_name = match (state.is_remote, state.remote_name.as_deref()) {
        (true, Some(remote)) => format!("{}/{}", remote, state.branch_name),
        _ => state.branch_name.clone(),
    };
    let label = text(format!(
        "This is a destructive operation, are you sure you want to delete '{}'?",
        display_name
    ))
    .size(theme::FONT_SM)
    .style(style::primary_text);

    let delete_btn = overlay_button(
        "Delete",
        DANGER_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::BranchDeleteConfirmed),
    );
    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::BranchDeleteCanceled,
    ));

    let content = if !state.is_remote && state.has_remote {
        let delete_all_btn = overlay_button(
            "Delete local and remote",
            DANGER_BUTTON,
            RepositoryMessage::OverlayPanel(OverlayPanelAction::BranchDeleteAllConfirmed),
        );
        overlay_row(vec![label.into(), delete_btn, delete_all_btn, cancel_btn])
    } else {
        overlay_row(vec![label.into(), delete_btn, cancel_btn])
    };

    sliding_main_bar_overlay(content, slide_offset)
}

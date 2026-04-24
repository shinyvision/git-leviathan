//! Remove worktree confirmation dialog — toolbar overlay, same pattern as
//! delete_branch. Blocks confirmation when the worktree is the active focus.

use std::path::PathBuf;

use iced::Element;

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_cancel_button, overlay_row, sliding_main_bar_overlay, DANGER_BUTTON,
};

pub(crate) struct State {
    pub path: PathBuf,
    pub branch_name: String,
    pub is_active: bool,
}

pub(crate) fn view<'a>(state: &'a State, slide_offset: f32) -> Element<'a, Message> {
    let prompt = if state.is_active {
        format!(
            "Cannot remove '{}': it is the focused worktree. Switch away first.",
            state.branch_name,
        )
    } else {
        format!(
            "Remove worktree '{}' at {}?",
            state.branch_name,
            state.path.display()
        )
    };

    let text_el = iced::widget::text(prompt)
        .size(theme::FONT_SM)
        .style(style::primary_text);

    let items: Vec<Element<Message>> = if state.is_active {
        vec![
            text_el.into(),
            overlay_cancel_button(RepositoryMessage::OverlayPanel(
                OverlayPanelAction::WorktreeRemoveCanceled,
            )),
        ]
    } else {
        vec![
            text_el.into(),
            overlay_button(
                "Remove",
                DANGER_BUTTON,
                RepositoryMessage::OverlayPanel(OverlayPanelAction::WorktreeRemoveConfirmed),
            ),
            overlay_cancel_button(RepositoryMessage::OverlayPanel(
                OverlayPanelAction::WorktreeRemoveCanceled,
            )),
        ]
    };

    sliding_main_bar_overlay(overlay_row(items), slide_offset)
}

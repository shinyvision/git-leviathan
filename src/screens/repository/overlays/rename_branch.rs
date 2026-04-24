//! Rename a local or remote branch via inline text input.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_button_disabled, overlay_cancel_button, overlay_row,
    overlay_text_input_with_submit, sliding_main_bar_overlay, CREATE_BUTTON,
};

pub(crate) fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("rename-branch-input")
}

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub is_remote: bool,
    pub new_branch_input: String,
    pub needs_focus: bool,
    /// Remote name for display (e.g. "origin"). Not used for the git op.
    pub remote_name: Option<String>,
}

pub(crate) fn view<'a>(state: &'a State, slide_offset: f32) -> Element<'a, Message> {
    let has_name = !state.new_branch_input.trim().is_empty();

    let display_name = match (state.is_remote, state.remote_name.as_deref()) {
        (true, Some(remote)) => format!("{}/{}", remote, state.branch_name),
        _ => state.branch_name.clone(),
    };
    let label = text(format!("Rename '{}' to:", display_name))
        .size(theme::FONT_SM)
        .style(style::primary_text);

    let name_input = overlay_text_input_with_submit(
        "branch name",
        &state.new_branch_input,
        |s| RepositoryMessage::OverlayPanel(OverlayPanelAction::RenameNewBranchInput(s)),
        Message::repo(RepositoryMessage::OverlayPanel(
            OverlayPanelAction::BranchRenameConfirmed,
        )),
        Some(input_id()),
    );

    let submit_btn = if has_name {
        overlay_button(
            "Submit",
            CREATE_BUTTON,
            RepositoryMessage::OverlayPanel(OverlayPanelAction::BranchRenameConfirmed),
        )
    } else {
        overlay_button_disabled("Submit", CREATE_BUTTON)
    };
    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::BranchRenameCanceled,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![label.into(), name_input, submit_btn, cancel_btn]),
        slide_offset,
    )
}

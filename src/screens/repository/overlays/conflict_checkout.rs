//! Conflict bar shown when double-clicking a remote branch whose local
//! counterpart already exists. Offers create-new-branch or reset-local.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_button_disabled, overlay_cancel_button, overlay_row,
    overlay_text_input, sliding_main_bar_overlay, CREATE_BUTTON, DANGER_BUTTON,
};

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub remote_ref: String,
    pub new_branch_input: String,
}

pub(crate) fn view<'a>(state: &'a State, slide_offset: f32) -> Element<'a, Message> {
    let has_name = !state.new_branch_input.trim().is_empty();

    let label = text(format!("A local '{}' already exists.", state.branch_name))
        .size(theme::FONT_SM)
        .style(style::primary_text);

    let name_input = overlay_text_input("branch name", &state.new_branch_input, |s| {
        RepositoryMessage::OverlayPanel(OverlayPanelAction::ConflictNewBranchInput(s))
    });

    let create_btn = if has_name {
        overlay_button(
            "Create Branch Here",
            CREATE_BUTTON,
            RepositoryMessage::OverlayPanel(OverlayPanelAction::ConflictCreateBranch),
        )
    } else {
        overlay_button_disabled("Create Branch Here", CREATE_BUTTON)
    };
    let reset_btn = overlay_button(
        "Reset Local to Here",
        DANGER_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::ConflictResetLocal),
    );
    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::ConflictCancel,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![
            label.into(),
            name_input,
            create_btn,
            reset_btn,
            cancel_btn,
        ]),
        slide_offset,
    )
}

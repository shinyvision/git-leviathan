//! First-push-for-a-new-branch dialog: pick the upstream name on the remote.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_button_disabled, overlay_cancel_button, overlay_row,
    overlay_text_input_with_submit, sliding_main_bar_overlay, CREATE_BUTTON,
};

pub(crate) fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("set-upstream-input")
}

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub remote_name: String,
    pub new_branch_input: String,
    pub needs_focus: bool,
    pub submitting: bool,
}

pub(crate) fn view<'a>(state: &'a State, slide_offset: f32) -> Element<'a, Message> {
    let has_name = !state.new_branch_input.trim().is_empty();

    let label = text(format!(
        "Push '{}' to {}/",
        state.branch_name, state.remote_name
    ))
    .size(theme::FONT_SM)
    .style(style::primary_text);

    let name_input = overlay_text_input_with_submit(
        "remote branch name",
        &state.new_branch_input,
        |s| RepositoryMessage::OverlayPanel(OverlayPanelAction::SetUpstreamInput(s)),
        Message::repo(RepositoryMessage::OverlayPanel(
            OverlayPanelAction::SetUpstreamConfirmed,
        )),
        Some(input_id()),
    );

    let submit_btn = if has_name && !state.submitting {
        overlay_button(
            "Confirm",
            CREATE_BUTTON,
            RepositoryMessage::OverlayPanel(OverlayPanelAction::SetUpstreamConfirmed),
        )
    } else {
        overlay_button_disabled("Confirm", CREATE_BUTTON)
    };
    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::SetUpstreamCanceled,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![label.into(), name_input, submit_btn, cancel_btn]),
        slide_offset,
    )
}

//! "Create branch here" dialog triggered from a commit's right-click menu.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::validation::validate_branch_name;
use super::widgets::{
    overlay_button, overlay_button_disabled, overlay_cancel_button, overlay_row,
    overlay_text_input_with_submit, sliding_main_bar_overlay, CREATE_BUTTON,
};

pub(crate) fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("create-branch-here-input")
}

#[derive(Debug, Clone)]
pub(crate) struct State {
    #[allow(dead_code)]
    pub commit_idx: usize,
    pub commit_hash: String,
    pub branch_name_input: String,
    /// Whether the input field needs focus after animation completes.
    pub needs_focus: bool,
}

pub(crate) fn view<'a>(
    state: &'a State,
    slide_offset: f32,
    existing_branches: &[String],
) -> Element<'a, Message> {
    let label = text("Enter branch name")
        .size(theme::FONT_SM)
        .style(style::primary_text);

    let name_input = overlay_text_input_with_submit(
        "branch name",
        &state.branch_name_input,
        |s| RepositoryMessage::OverlayPanel(OverlayPanelAction::CreateBranchHereInput(s)),
        Message::repo(RepositoryMessage::OverlayPanel(
            OverlayPanelAction::CreateBranchHereConfirmed,
        )),
        Some(input_id()),
    );

    let validation_ok = validate_branch_name(&state.branch_name_input, existing_branches).is_ok();

    let create_btn: Element<Message> = if validation_ok {
        overlay_button(
            "Create Branch",
            CREATE_BUTTON,
            RepositoryMessage::OverlayPanel(OverlayPanelAction::CreateBranchHereConfirmed),
        )
    } else {
        overlay_button_disabled("Create Branch", CREATE_BUTTON)
    };

    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::CreateBranchHereCanceled,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![label.into(), name_input, create_btn, cancel_btn]),
        slide_offset,
    )
}

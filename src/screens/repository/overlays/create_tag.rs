//! "Create tag here" dialog triggered from a commit's right-click menu.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_button_disabled, overlay_cancel_button, overlay_row,
    overlay_text_input_with_submit, sliding_main_bar_overlay, CREATE_BUTTON,
};

pub(crate) fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("create-tag-here-input")
}

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub commit_hash: String,
    pub tag_name_input: String,
    pub needs_focus: bool,
}

pub(crate) fn view<'a>(state: &'a State, slide_offset: f32) -> Element<'a, Message> {
    let has_name = !state.tag_name_input.trim().is_empty();

    let label = text("Enter tag name")
        .size(theme::FONT_SM)
        .style(style::primary_text);

    let name_input = overlay_text_input_with_submit(
        "tag name",
        &state.tag_name_input,
        |s| RepositoryMessage::OverlayPanel(OverlayPanelAction::CreateTagHereInput(s)),
        Message::repo(RepositoryMessage::OverlayPanel(
            OverlayPanelAction::CreateTagHereConfirmed,
        )),
        Some(input_id()),
    );

    let create_btn = if has_name {
        overlay_button(
            "Create Tag",
            CREATE_BUTTON,
            RepositoryMessage::OverlayPanel(OverlayPanelAction::CreateTagHereConfirmed),
        )
    } else {
        overlay_button_disabled("Create Tag", CREATE_BUTTON)
    };

    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::CreateTagHereCanceled,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![label.into(), name_input, create_btn, cancel_btn]),
        slide_offset,
    )
}

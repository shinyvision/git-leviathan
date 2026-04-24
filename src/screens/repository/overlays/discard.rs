//! Confirmation before discarding dirty changes — either a single file or the
//! entire working tree.

use iced::{widget::text, Element};

use crate::{message::Message, style, theme};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_cancel_button, overlay_row, sliding_main_bar_overlay, DANGER_BUTTON,
};

#[derive(Debug, Clone)]
pub(crate) enum Target {
    All,
    File(String),
}

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub target: Target,
}

pub(crate) fn view_all(slide_offset: f32) -> Element<'static, Message> {
    let label = text("Are you sure you want to discard all changes?")
        .size(theme::FONT_SM)
        .style(style::primary_text);

    let discard_btn = overlay_button(
        "Discard All Changes",
        DANGER_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::DiscardConfirmed),
    );
    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::DiscardCanceled,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![label.into(), discard_btn, cancel_btn]),
        slide_offset,
    )
}

pub(crate) fn view_file(file_name: String, slide_offset: f32) -> Element<'static, Message> {
    let label = text(format!(
        "Are you sure you want to discard all changes to '{}'?",
        file_name
    ))
    .size(theme::FONT_SM)
    .style(style::primary_text);

    let reset_btn = overlay_button(
        "Reset File",
        DANGER_BUTTON,
        RepositoryMessage::OverlayPanel(OverlayPanelAction::DiscardConfirmed),
    );
    let cancel_btn = overlay_cancel_button(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::DiscardCanceled,
    ));

    sliding_main_bar_overlay(
        overlay_row(vec![label.into(), reset_btn, cancel_btn]),
        slide_offset,
    )
}

pub(crate) fn view<'a>(state: &'a State, slide_offset: f32) -> Element<'a, Message> {
    match &state.target {
        Target::All => view_all(slide_offset),
        Target::File(path) => {
            let file_name = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path.as_str())
                .to_string();
            view_file(file_name, slide_offset)
        }
    }
}

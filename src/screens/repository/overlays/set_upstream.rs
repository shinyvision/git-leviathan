//! First-push-for-a-new-branch dialog: pick the remote and upstream name.

use iced::{
    widget::{container, text},
    Color, Element, Length, Theme,
};

use crate::{
    assets,
    message::Message,
    style, theme,
    widgets::dropdown::{dropdown_item, dropdown_menu, dropdown_trigger, icon_label, Dropdown},
};

use super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::widgets::{
    overlay_button, overlay_button_disabled, overlay_cancel_button, overlay_row,
    overlay_text_input_with_submit, sliding_main_bar_overlay, CREATE_BUTTON,
};

const REMOTE_DROPDOWN_WIDTH: f32 = 150.0;

pub(crate) fn input_id() -> iced::widget::Id {
    iced::widget::Id::new("set-upstream-input")
}

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub branch_name: String,
    pub selected_remote_name: String,
    pub available_remotes: Vec<String>,
    pub remote_dropdown_open: bool,
    pub new_branch_input: String,
    pub needs_focus: bool,
    pub submitting: bool,
}

impl State {
    pub(crate) fn new(
        branch_name: String,
        proposed_remote_name: String,
        available_remotes: Vec<String>,
    ) -> Self {
        let available_remotes = normalized_remotes(&proposed_remote_name, available_remotes);
        let proposed_remote_name = proposed_remote_name.trim();
        let selected_remote_name = available_remotes
            .iter()
            .find(|remote| remote.as_str() == proposed_remote_name)
            .cloned()
            .or_else(|| available_remotes.first().cloned())
            .unwrap_or_default();

        Self {
            new_branch_input: branch_name.clone(),
            branch_name,
            selected_remote_name,
            available_remotes,
            remote_dropdown_open: false,
            needs_focus: true,
            submitting: false,
        }
    }

    pub(crate) fn select_remote(&mut self, remote_name: String) {
        let remote_name = remote_name.trim();
        if remote_name.is_empty() {
            return;
        }
        if self
            .available_remotes
            .iter()
            .any(|remote| remote == remote_name)
        {
            self.selected_remote_name = remote_name.to_string();
        }
    }

    pub(crate) fn can_submit(&self) -> bool {
        !self.submitting
            && !self.selected_remote_name.trim().is_empty()
            && !self.new_branch_input.trim().is_empty()
    }
}

pub(crate) fn view<'a>(state: &'a State, slide_offset: f32) -> Element<'a, Message> {
    let label = text(format!("Push '{}' to", state.branch_name))
        .size(theme::FONT_SM)
        .style(style::primary_text);

    let remote_dropdown = container(remote_dropdown_stack(state))
        .width(Length::Fixed(REMOTE_DROPDOWN_WIDTH))
        .into();

    let slash = text("/").size(theme::FONT_SM).style(style::secondary_text);

    let name_input = overlay_text_input_with_submit(
        "remote branch name",
        &state.new_branch_input,
        |s| RepositoryMessage::OverlayPanel(OverlayPanelAction::SetUpstreamInput(s)),
        Message::repo(RepositoryMessage::OverlayPanel(
            OverlayPanelAction::SetUpstreamConfirmed,
        )),
        Some(input_id()),
    );

    let submit_btn = if state.can_submit() {
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
        overlay_row(vec![
            label.into(),
            remote_dropdown,
            slash.into(),
            name_input,
            submit_btn,
            cancel_btn,
        ]),
        slide_offset,
    )
}

fn remote_dropdown_stack<'a>(state: &'a State) -> Element<'a, Message> {
    let toggle_msg = Message::repo(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::SetUpstreamRemoteDropdownToggled,
    ));

    let label = (!state.selected_remote_name.is_empty())
        .then(|| remote_label(&state.selected_remote_name, theme::TEXT_PRIMARY));
    let trigger = dropdown_trigger(label, "Select remote", toggle_msg.clone());

    let menu = if state.remote_dropdown_open {
        let items: Vec<Element<Message>> = state
            .available_remotes
            .iter()
            .map(|remote| {
                dropdown_item(
                    remote_label(remote, theme::TEXT_PRIMARY),
                    Message::repo(RepositoryMessage::OverlayPanel(
                        OverlayPanelAction::SetUpstreamRemoteChanged(remote.clone()),
                    )),
                )
            })
            .collect();
        Some(dropdown_menu(items))
    } else {
        None
    };

    Dropdown::new(trigger, menu, toggle_msg)
        .menu_width(REMOTE_DROPDOWN_WIDTH)
        .into()
}

fn remote_label<'a>(remote_name: &'a str, text_color: Color) -> Element<'a, Message> {
    icon_label(
        assets::CLOUD,
        theme::TEXT_SECONDARY,
        text(remote_name.to_string())
            .size(theme::FONT_SM)
            .style(move |_: &Theme| text::Style {
                color: Some(text_color),
            })
            .into(),
    )
}

fn normalized_remotes(proposed_remote_name: &str, available_remotes: Vec<String>) -> Vec<String> {
    let mut remotes = Vec::new();
    push_unique_remote(&mut remotes, proposed_remote_name);
    for remote in available_remotes {
        push_unique_remote(&mut remotes, &remote);
    }
    remotes
}

fn push_unique_remote(remotes: &mut Vec<String>, remote_name: &str) {
    let remote_name = remote_name.trim();
    if remote_name.is_empty() || remotes.iter().any(|remote| remote == remote_name) {
        return;
    }
    remotes.push(remote_name.to_string());
}

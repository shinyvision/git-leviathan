//! Reword editor panel for the detail view.
//!
//! Renders either (a) the read-only commit message with optional "click to
//! reword" interaction, or (b) the active reword editor with update/cancel
//! buttons and a descendant-count warning.

use iced::{
    keyboard,
    widget::{button, column, container, text, text_editor, MouseArea},
    Border, Element, Length, Padding, Theme,
};

use crate::{
    core::Commit,
    message::Message,
    screens::repository::{panel_messages::DetailAction, RepositoryMessage},
    style, theme,
    widgets::primitives::hoverable::{HoverStatus, Hoverable},
};

use super::super::state::RewordViewModel;
use super::styles::{detail_text_editor_style, green_button_style, red_button_style};

pub(super) fn reword_message_view<'a>(
    commit: &'a Commit,
    reword: Option<RewordViewModel<'a>>,
    allowed: bool,
    descendant_count: usize,
) -> Element<'a, Message> {
    if let Some(reword) = reword {
        let editor = text_editor(reword.content)
            .id(super::super::reword_commit_message_editor_id())
            .on_action(|action| {
                Message::repo(RepositoryMessage::Detail(
                    DetailAction::RewordMessageAction(action),
                ))
            })
            .key_binding(reword_message_key_binding)
            .size(theme::FONT_MD)
            .padding(Padding::from([8, 10]))
            .height(Length::Fixed(180.0))
            .style(detail_text_editor_style);

        let info = text(format!(
            "Rewording this commit message will cause {} {} to be rebased.",
            descendant_count,
            if descendant_count == 1 {
                "commit"
            } else {
                "commits"
            }
        ))
        .size(theme::FONT_SM)
        .style(style::dim_text);

        let update_btn = button(
            container(text("Update Message").size(theme::FONT_MD))
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .padding(Padding::from([10, 16]))
        .width(Length::Fill)
        .style(green_button_style(true))
        .on_press(Message::repo(RepositoryMessage::Detail(
            DetailAction::RewordConfirmed,
        )));

        let cancel_btn = button(
            container(text("Cancel Reword").size(theme::FONT_MD))
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .padding(Padding::from([10, 16]))
        .width(Length::Fill)
        .style(red_button_style(true))
        .on_press(Message::repo(RepositoryMessage::Detail(
            DetailAction::RewordCanceled,
        )));

        let buttons = iced::widget::row![update_btn, cancel_btn].spacing(8);

        return column![editor, info, buttons]
            .spacing(10)
            .padding(Padding::from([10, 10]))
            .into();
    }

    let (summary, body) =
        crate::screens::repository::panels::detail::split_commit_message(&commit.message);

    let mut content_col = column![text(summary)
        .size(theme::FONT_LG)
        .width(Length::Fill)
        .style(style::primary_text)]
    .spacing(10);
    if !body.is_empty() {
        let body_lines: Vec<Element<Message>> = body
            .lines()
            .map(|line| {
                text(line.to_string())
                    .size(theme::FONT_MD)
                    .width(Length::Fill)
                    .style(style::primary_text)
                    .into()
            })
            .collect();
        content_col = content_col.push(column(body_lines).spacing(2));
    }

    let inner = container(content_col)
        .padding(Padding::from([12, 14]))
        .width(Length::Fill)
        .height(Length::Fixed(180.0));

    let bordered = Hoverable::new(inner, move |_: &Theme, status: HoverStatus| {
        container::Style {
            background: Some(theme::BG_BASE.into()),
            border: Border {
                color: if allowed && status.is_hovered() {
                    theme::ACCENT_BLUE
                } else {
                    theme::BORDER
                },
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    });

    if allowed {
        let hash = commit.hash.clone();
        let original = commit.message.clone();
        return container(
            MouseArea::new(bordered)
                .interaction(iced::mouse::Interaction::Pointer)
                .on_press(Message::repo(RepositoryMessage::Detail(
                    DetailAction::RewordStarted {
                        hash,
                        original_message: original,
                    },
                ))),
        )
        .padding(Padding::from([10, 10]))
        .into();
    }

    container(bordered).padding(Padding::from([10, 10])).into()
}

fn reword_message_key_binding(
    key_press: text_editor::KeyPress,
) -> Option<text_editor::Binding<Message>> {
    let focused = matches!(key_press.status, text_editor::Status::Focused { .. });
    let key = key_press.key.as_ref();

    if focused
        && key_press.modifiers.control()
        && matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter))
    {
        return Some(text_editor::Binding::Custom(Message::repo(
            RepositoryMessage::Detail(DetailAction::RewordConfirmed),
        )));
    }

    if focused && matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
        return Some(text_editor::Binding::Custom(Message::repo(
            RepositoryMessage::Detail(DetailAction::RewordCanceled),
        )));
    }

    text_editor::Binding::from_key_press(key_press)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Message, ScreenMessage, ScreenRouted};

    fn key_press(
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
        status: text_editor::Status,
    ) -> text_editor::KeyPress {
        text_editor::KeyPress {
            modified_key: key.clone(),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Enter),
            key,
            modifiers,
            text: None,
            status,
        }
    }

    fn custom_detail_action(binding: Option<text_editor::Binding<Message>>) -> DetailAction {
        let Some(text_editor::Binding::Custom(Message::Screen(ScreenRouted::Active(
            ScreenMessage::Repository(message),
        )))) = binding
        else {
            panic!("expected custom repository message");
        };
        let RepositoryMessage::Detail(action) = *message else {
            panic!("expected detail action");
        };
        action
    }

    #[test]
    fn reword_message_ctrl_enter_confirms_when_focused() {
        let action = custom_detail_action(reword_message_key_binding(key_press(
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::Modifiers::CTRL,
            text_editor::Status::Focused { is_hovered: false },
        )));

        assert!(matches!(action, DetailAction::RewordConfirmed));
    }

    #[test]
    fn reword_message_escape_cancels_when_focused() {
        let action = custom_detail_action(reword_message_key_binding(key_press(
            keyboard::Key::Named(keyboard::key::Named::Escape),
            keyboard::Modifiers::default(),
            text_editor::Status::Focused { is_hovered: false },
        )));

        assert!(matches!(action, DetailAction::RewordCanceled));
    }

    #[test]
    fn reword_message_enter_keeps_default_editor_newline() {
        let binding = reword_message_key_binding(key_press(
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::Modifiers::default(),
            text_editor::Status::Focused { is_hovered: false },
        ));

        assert!(matches!(binding, Some(text_editor::Binding::Enter)));
    }
}

//! Reword editor panel for the detail view.
//!
//! Renders either (a) the read-only commit message with optional "click to
//! reword" interaction, or (b) the active reword editor with update/cancel
//! buttons and a descendant-count warning.

use iced::{
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
            .on_action(|action| {
                Message::repo(RepositoryMessage::Detail(
                    DetailAction::RewordMessageAction(action),
                ))
            })
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
        .style(red_button_style())
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

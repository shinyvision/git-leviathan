//! Dirty-changes detail panel: working tree staging UI.
//!
//! Renders the three file sections (Conflicts / Unstaged / Staged) with
//! per-file and per-section action buttons, plus the commit-message editor
//! and Commit-All / Abort-Merge action row.

use iced::{
    widget::{button, column, container, row, scrollable, text, text_editor},
    Element, Length, Padding,
};

use crate::{
    assets,
    core::{ChangedFile, Commit},
    message::Message,
    screens::repository::{panel_messages::DetailAction, RepositoryMessage},
    style, theme,
    widgets::shared::{h_divider, horizontal_space, scrollbar_style},
};

use super::super::state::DetailViewModel;
use super::file_row_view;
use super::styles::{detail_text_editor_style, green_button_style, red_button_style};

const DIRTY_COMMIT_ACTION_BUTTON_HEIGHT: f32 = 40.0;

#[derive(Debug, Clone, Copy)]
pub(super) enum DirtyFileSection {
    Conflicted,
    Unstaged,
    Staged,
}

impl DirtyFileSection {
    fn all_action_label(self) -> &'static str {
        match self {
            Self::Conflicted => "Mark All Resolved",
            Self::Unstaged => "Stage All Changes",
            Self::Staged => "Unstage All Changes",
        }
    }

    pub(super) fn file_action_label(self) -> &'static str {
        match self {
            Self::Conflicted => "Mark Resolved",
            Self::Unstaged => "Stage File",
            Self::Staged => "Unstage File",
        }
    }

    fn all_action_message(self) -> RepositoryMessage {
        match self {
            Self::Conflicted => RepositoryMessage::Detail(DetailAction::MarkAllConflictsResolved),
            Self::Unstaged => RepositoryMessage::Detail(DetailAction::StageAll),
            Self::Staged => RepositoryMessage::Detail(DetailAction::UnstageAll),
        }
    }

    pub(super) fn file_action_message(self, path: String) -> RepositoryMessage {
        match self {
            Self::Conflicted => RepositoryMessage::Detail(DetailAction::MarkConflictResolved(path)),
            Self::Unstaged => RepositoryMessage::Detail(DetailAction::StageFile(path)),
            Self::Staged => RepositoryMessage::Detail(DetailAction::UnstageFile(path)),
        }
    }

    pub(super) fn action_tone(self) -> DirtyActionTone {
        match self {
            Self::Conflicted => DirtyActionTone::Resolve,
            Self::Unstaged => DirtyActionTone::Safe,
            Self::Staged => DirtyActionTone::Danger,
        }
    }

    pub(super) fn is_staged_for_diff(self) -> bool {
        matches!(self, Self::Staged)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum DirtyActionTone {
    Safe,
    Danger,
    Resolve,
}

pub(super) fn dirty_detail_panel_content<'a>(
    screen: DetailViewModel<'a>,
    commit: &'a Commit,
) -> Element<'a, Message> {
    let width = screen.width;
    let dirty_file_count = dirty_change_count(commit);
    let dirty_summary = if dirty_file_count == 1 {
        "1 file change".to_string()
    } else {
        format!("{dirty_file_count} file changes")
    };

    let has_any_dirty = dirty_file_count > 0;
    let hash_bar = if commit.is_merge_in_progress {
        row![
            text(dirty_summary)
                .size(theme::FONT_SM)
                .style(style::dim_text),
            text(" on ").size(theme::FONT_SM).style(style::dim_text),
            text(screen.current_branch)
                .size(theme::FONT_SM)
                .style(|_: &iced::Theme| text::Style {
                    color: Some(theme::ACCENT_BLUE)
                }),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8)
        .padding(Padding::from([6, 10]))
    } else {
        let mut discard_button = button(text("Discard All Changes").size(theme::FONT_XS))
        .style(red_button_style())
        .padding(Padding::from([4, 8]));
        if has_any_dirty {
            discard_button = discard_button.on_press(Message::repo(RepositoryMessage::Detail(
                DetailAction::DiscardAllRequested,
            )));
        }

        row![
            text(dirty_summary)
                .size(theme::FONT_SM)
                .style(style::dim_text),
            text(" on ").size(theme::FONT_SM).style(style::dim_text),
            text(screen.current_branch)
                .size(theme::FONT_SM)
                .style(|_: &iced::Theme| text::Style {
                    color: Some(theme::ACCENT_BLUE)
                }),
            horizontal_space(),
            discard_button,
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8)
        .padding(Padding::from([6, 10]))
    };

    let commit_msg = container(
        text("Uncommitted changes")
            .size(theme::FONT_LG)
            .style(style::primary_text),
    )
    .padding(Padding {
        top: 10.0,
        right: 10.0,
        bottom: 6.0,
        left: 10.0,
    });

    let dirty_modified = screen
        .commit_diff_state
        .map(|d| d.modified_count)
        .unwrap_or(0);
    let dirty_added = screen.commit_diff_state.map(|d| d.added_count).unwrap_or(0);
    let stats_row = row![
        assets::sidebar_icon(assets::PENCIL, theme::TEXT_SECONDARY),
        text(format!("{} modified", dirty_modified))
            .size(theme::FONT_SM)
            .style(style::secondary_text),
        text(format!("  + {} added", dirty_added))
            .size(theme::FONT_SM)
            .style(|_: &iced::Theme| text::Style {
                color: Some(theme::ACCENT_GREEN)
            }),
        horizontal_space(),
    ]
    .padding(Padding::from([6, 10]))
    .spacing(4)
    .align_y(iced::Alignment::Center);

    let file_list = scrollable(
        column![
            dirty_file_section(
                "Conflicts",
                &commit.conflicted_files,
                DirtyFileSection::Conflicted,
                None,
                screen.active_diff_file_path,
                width,
            ),
            dirty_file_section(
                "Unstaged Files",
                &commit.unstaged_files,
                DirtyFileSection::Unstaged,
                None,
                screen.active_diff_file_path,
                width,
            ),
            dirty_file_section(
                "Staged Files",
                &commit.staged_files,
                DirtyFileSection::Staged,
                None,
                screen.active_diff_file_path,
                width,
            ),
        ]
        .spacing(0)
        .width(Length::Fill),
    )
    .height(Length::Fill)
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::new().width(5).scroller_width(5),
    ))
    .style(scrollbar_style);

    let can_commit = !commit.staged_files.is_empty()
        && dirty_commit_message_has_summary(screen.dirty_commit_message);

    let commit_message_label = container(
        text("Commit Message")
            .size(theme::FONT_SM)
            .style(style::secondary_text),
    )
    .padding(Padding {
        top: 0.0,
        right: 0.0,
        bottom: 2.0,
        left: 0.0,
    });

    let editor_width = width - 20.0;
    let commit_message_editor = text_editor(screen.dirty_commit_message)
        .placeholder("Commit Message")
        .on_action(|action| {
            Message::repo(RepositoryMessage::Detail(
                DetailAction::CommitMessageAction(action),
            ))
        })
        .size(theme::FONT_MD)
        .padding(Padding::from([8, 10]))
        .height(Length::Fixed(104.0))
        .width(editor_width)
        .style(detail_text_editor_style);

    let mut commit_button = button(dirty_commit_action_label("Commit All Changes"))
        .style(green_button_style(can_commit))
        .padding(Padding::from([0, 16]))
        .height(Length::Fixed(DIRTY_COMMIT_ACTION_BUTTON_HEIGHT))
        .width(Length::Fill);

    if can_commit {
        commit_button = commit_button.on_press(Message::repo(RepositoryMessage::Detail(
            DetailAction::CommitConfirmed,
        )));
    }

    let action_row: Element<Message> = if commit.is_merge_in_progress {
        let abort_button = button(dirty_commit_action_label("Abort Merge"))
            .style(red_button_style())
            .padding(Padding::from([0, 16]))
            .height(Length::Fixed(DIRTY_COMMIT_ACTION_BUTTON_HEIGHT))
            .width(Length::Fill)
            .on_press(Message::repo(RepositoryMessage::Detail(
                DetailAction::AbortMergeConfirmed,
            )));

        row![commit_button, abort_button]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill)
            .into()
    } else {
        commit_button.into()
    };

    let commit_form = column![commit_message_label, commit_message_editor, action_row,]
        .padding(Padding::from([10, 10]))
        .spacing(8);

    let detail_col = column![
        hash_bar,
        h_divider(),
        commit_msg,
        stats_row,
        h_divider(),
        file_list,
        h_divider(),
        commit_form,
    ]
    .spacing(0)
    .height(Length::Fill);

    container(detail_col)
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .style(style::panel_container)
        .into()
}

fn dirty_file_section<'a>(
    title: &'static str,
    files: &'a [ChangedFile],
    section: DirtyFileSection,
    _hovered_path: Option<&'a str>,
    active_diff_path: Option<&'a str>,
    available_width: f32,
) -> Element<'a, Message> {
    let mut header = row![
        text(format!("{} ({})", title, files.len()))
            .size(theme::FONT_SM)
            .style(style::secondary_text),
        horizontal_space(),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .width(Length::Fill);

    if !files.is_empty() {
        header = header.push(dirty_action_button(
            section.all_action_label(),
            section.all_action_message(),
            section.action_tone(),
        ));
    }

    let mut rows: Vec<Element<Message>> = vec![container(header)
        .padding(Padding::from([8, 10]))
        .width(Length::Fill)
        .into()];

    if files.is_empty() {
        rows.push(
            container(text("No files").size(theme::FONT_SM).style(style::dim_text))
                .padding(Padding::from([3, 10]))
                .width(Length::Fill)
                .into(),
        );
    } else {
        rows.extend(files.iter().map(|file| {
            file_row_view(
                file,
                Some(section),
                None,
                false,
                active_diff_path,
                available_width,
            )
        }));
    }

    column(rows).spacing(0).width(Length::Fill).into()
}

pub(super) fn dirty_action_button<'a>(
    label: &'static str,
    on_press: RepositoryMessage,
    tone: DirtyActionTone,
) -> Element<'a, Message> {
    let btn = button(text(label).size(theme::FONT_XS))
        .padding(Padding::from([4, 8]))
        .on_press(Message::repo(on_press));
    match tone {
        DirtyActionTone::Safe => btn.style(green_button_style(true)).into(),
        DirtyActionTone::Danger => btn.style(red_button_style()).into(),
        DirtyActionTone::Resolve => btn.style(super::styles::resolve_button_style()).into(),
    }
}

fn dirty_commit_action_label(label: &'static str) -> Element<'static, Message> {
    container(text(label).size(theme::FONT_SM))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

fn dirty_commit_message_has_summary(content: &text_editor::Content) -> bool {
    content
        .text()
        .lines()
        .next()
        .is_some_and(|line| !line.trim().is_empty())
}

fn dirty_change_count(commit: &Commit) -> usize {
    use std::collections::HashSet;

    commit
        .unstaged_files
        .iter()
        .chain(commit.conflicted_files.iter())
        .chain(commit.staged_files.iter())
        .map(|file| file.path.as_str())
        .collect::<HashSet<_>>()
        .len()
}

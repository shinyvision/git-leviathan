//! Dirty-changes detail panel: working tree staging UI.
//!
//! Renders the three file sections (Conflicts / Unstaged / Staged) with
//! per-file and per-section action buttons, plus the commit-message editor
//! and Commit-All / Abort-Merge action row.

use iced::{
    widget::{button, column, container, responsive, row, scrollable, text, text_editor},
    Element, Length, Padding,
};

use crate::{
    assets,
    core::{ChangedFile, Commit},
    message::Message,
    screens::repository::{panel_messages::DetailAction, RepositoryMessage},
    style, theme,
    widgets::shared::{h_divider, horizontal_space, scrollbar_style, v_divider},
};

use super::super::state::DetailViewModel;
use super::file_row_view;
use super::styles::{detail_text_editor_style, green_button_style, red_button_style};

const DIRTY_COMMIT_ACTION_BUTTON_HEIGHT: f32 = 40.0;

#[derive(Debug, Clone, Copy)]
struct DirtyActionBusy {
    general: bool,
    fast: bool,
}

impl DirtyActionBusy {
    fn for_section(self, section: DirtyFileSection) -> bool {
        match section {
            DirtyFileSection::Unstaged | DirtyFileSection::Staged => self.fast,
            DirtyFileSection::Conflicted => self.general,
        }
    }
}

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
    let dirty_summary = dirty_summary_text(commit);
    let has_any_dirty = dirty_change_count(commit) > 0;
    let busy = DirtyActionBusy {
        general: screen.dirty_actions_busy,
        fast: screen.dirty_fast_actions_busy,
    };

    let hash_bar = dirty_hash_bar(
        commit,
        &dirty_summary,
        screen.current_branch,
        has_any_dirty,
        busy.general,
    );
    let stats_row = dirty_stats_row(screen.commit_diff_state, screen.dirty_operation_label);
    let file_list = dirty_file_list(commit, screen.active_diff_file_path, width, busy);

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

    let can_commit = !commit.staged_files.is_empty()
        && dirty_commit_message_has_summary(screen.dirty_commit_message);

    let commit_form = dirty_commit_form_inline(
        screen.dirty_commit_message,
        can_commit,
        commit.is_merge_in_progress,
        width - 20.0,
        busy,
        screen.dirty_operation_label,
    );

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

pub(super) fn dirty_detail_panel_content_horizontal<'a>(
    screen: DetailViewModel<'a>,
    commit: &'a Commit,
) -> Element<'a, Message> {
    let width = screen.width;
    let dirty_summary = dirty_summary_text(commit);
    let has_any_dirty = dirty_change_count(commit) > 0;
    let busy = DirtyActionBusy {
        general: screen.dirty_actions_busy,
        fast: screen.dirty_fast_actions_busy,
    };

    let hash_bar = dirty_hash_bar(
        commit,
        &dirty_summary,
        screen.current_branch,
        has_any_dirty,
        busy.general,
    );
    let stats_row = dirty_stats_row(screen.commit_diff_state, screen.dirty_operation_label);
    let file_list = dirty_file_list(commit, screen.active_diff_file_path, width, busy);

    let can_commit = !commit.staged_files.is_empty()
        && dirty_commit_message_has_summary(screen.dirty_commit_message);
    let commit_form = dirty_commit_form_responsive(
        screen.dirty_commit_message,
        can_commit,
        commit.is_merge_in_progress,
        busy,
        screen.dirty_operation_label,
    );

    let left_col = column![
        container(
            text("Uncommitted changes")
                .size(theme::FONT_LG)
                .style(style::primary_text),
        )
        .padding(Padding {
            top: 10.0,
            right: 10.0,
            bottom: 6.0,
            left: 10.0,
        }),
        commit_form,
    ]
    .spacing(0)
    .height(Length::Fill)
    .width(Length::FillPortion(3));

    let right_col = column![hash_bar, h_divider(), stats_row, h_divider(), file_list,]
        .spacing(0)
        .height(Length::Fill)
        .width(Length::FillPortion(2));

    container(row![left_col, v_divider(), right_col].spacing(0))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::panel_container)
        .into()
}

fn dirty_summary_text(commit: &Commit) -> String {
    let count = dirty_change_count(commit);
    if count == 1 {
        "1 file change".to_string()
    } else {
        format!("{count} file changes")
    }
}

fn dirty_hash_bar<'a>(
    commit: &'a Commit,
    dirty_summary: &str,
    current_branch: &'a str,
    has_any_dirty: bool,
    actions_busy: bool,
) -> Element<'a, Message> {
    let summary = dirty_summary.to_string();
    if commit.is_merge_in_progress {
        row![
            text(summary).size(theme::FONT_SM).style(style::dim_text),
            text(" on ").size(theme::FONT_SM).style(style::dim_text),
            text(current_branch)
                .size(theme::FONT_SM)
                .style(|_: &iced::Theme| text::Style {
                    color: Some(theme::ACCENT_BLUE),
                }),
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8)
        .padding(Padding::from([6, 10]))
        .into()
    } else {
        let mut discard_button = button(text("Discard All Changes").size(theme::FONT_XS))
            .style(red_button_style(has_any_dirty && !actions_busy))
            .padding(Padding::from([4, 8]));
        if has_any_dirty && !actions_busy {
            discard_button = discard_button.on_press(Message::repo(RepositoryMessage::Detail(
                DetailAction::DiscardAllRequested,
            )));
        }
        row![
            text(summary).size(theme::FONT_SM).style(style::dim_text),
            text(" on ").size(theme::FONT_SM).style(style::dim_text),
            text(current_branch)
                .size(theme::FONT_SM)
                .style(|_: &iced::Theme| text::Style {
                    color: Some(theme::ACCENT_BLUE),
                }),
            horizontal_space(),
            discard_button,
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8)
        .padding(Padding::from([6, 10]))
        .into()
    }
}

fn dirty_stats_row<'a>(
    commit_diff_state: Option<&'a crate::view_model::diff_view::CommitDiffState>,
    operation_label: Option<&'static str>,
) -> Element<'a, Message> {
    let modified = commit_diff_state.map(|d| d.modified_count).unwrap_or(0);
    let added = commit_diff_state.map(|d| d.added_count).unwrap_or(0);
    row![
        assets::sidebar_icon(assets::PENCIL, theme::TEXT_SECONDARY),
        text(format!("{} modified", modified))
            .size(theme::FONT_SM)
            .style(style::secondary_text),
        text(format!("  + {} added", added))
            .size(theme::FONT_SM)
            .style(|_: &iced::Theme| text::Style {
                color: Some(theme::ACCENT_GREEN),
            }),
        horizontal_space(),
        text(operation_label.unwrap_or(""))
            .size(theme::FONT_SM)
            .style(style::dim_text),
    ]
    .padding(Padding::from([6, 10]))
    .spacing(4)
    .align_y(iced::Alignment::Center)
    .into()
}

fn dirty_file_list<'a>(
    commit: &'a Commit,
    active_diff_file_path: Option<&'a str>,
    width: f32,
    busy: DirtyActionBusy,
) -> Element<'a, Message> {
    scrollable(
        column![
            dirty_file_section(
                "Conflicts",
                &commit.conflicted_files,
                DirtyFileSection::Conflicted,
                None,
                active_diff_file_path,
                width,
                busy,
            ),
            dirty_file_section(
                "Unstaged Files",
                &commit.unstaged_files,
                DirtyFileSection::Unstaged,
                None,
                active_diff_file_path,
                width,
                busy,
            ),
            dirty_file_section(
                "Staged Files",
                &commit.staged_files,
                DirtyFileSection::Staged,
                None,
                active_diff_file_path,
                width,
                busy,
            ),
        ]
        .spacing(0)
        .width(Length::Fill),
    )
    .height(Length::Fill)
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::new().width(5).scroller_width(5),
    ))
    .style(scrollbar_style)
    .into()
}

fn dirty_commit_form_inline<'a>(
    dirty_commit_message: &'a text_editor::Content,
    can_commit: bool,
    is_merge_in_progress: bool,
    editor_width: f32,
    busy: DirtyActionBusy,
    operation_label: Option<&'static str>,
) -> Element<'a, Message> {
    let editor = text_editor(dirty_commit_message)
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

    column![
        commit_message_label(),
        editor,
        commit_action_row(can_commit, is_merge_in_progress, busy, operation_label),
    ]
    .padding(Padding::from([10, 10]))
    .spacing(8)
    .into()
}

fn dirty_commit_form_responsive<'a>(
    dirty_commit_message: &'a text_editor::Content,
    can_commit: bool,
    is_merge_in_progress: bool,
    busy: DirtyActionBusy,
    operation_label: Option<&'static str>,
) -> Element<'a, Message> {
    responsive(move |size| {
        let editor_width = (size.width - 20.0).max(100.0);
        let editor = text_editor(dirty_commit_message)
            .placeholder("Commit Message")
            .on_action(|action| {
                Message::repo(RepositoryMessage::Detail(
                    DetailAction::CommitMessageAction(action),
                ))
            })
            .size(theme::FONT_MD)
            .padding(Padding::from([8, 10]))
            .height(Length::Fill)
            .width(editor_width)
            .style(detail_text_editor_style);

        column![
            commit_message_label(),
            editor,
            commit_action_row(can_commit, is_merge_in_progress, busy, operation_label),
        ]
        .padding(Padding::from([10, 10]))
        .spacing(8)
        .height(Length::Fill)
        .into()
    })
    .into()
}

fn commit_message_label<'a>() -> Element<'a, Message> {
    container(
        text("Commit Message")
            .size(theme::FONT_SM)
            .style(style::secondary_text),
    )
    .padding(Padding {
        top: 0.0,
        right: 0.0,
        bottom: 2.0,
        left: 0.0,
    })
    .into()
}

fn commit_action_row<'a>(
    can_commit: bool,
    is_merge_in_progress: bool,
    busy: DirtyActionBusy,
    operation_label: Option<&'static str>,
) -> Element<'a, Message> {
    let commit_label = if busy.fast {
        operation_label.unwrap_or("Working...")
    } else {
        "Commit All Changes"
    };
    let commit_enabled = can_commit && !busy.fast;
    let mut commit_button = button(dirty_commit_action_label(commit_label))
        .style(green_button_style(commit_enabled))
        .padding(Padding::from([0, 16]))
        .height(Length::Fixed(DIRTY_COMMIT_ACTION_BUTTON_HEIGHT))
        .width(Length::Fill);

    if commit_enabled {
        commit_button = commit_button.on_press(Message::repo(RepositoryMessage::Detail(
            DetailAction::CommitConfirmed,
        )));
    }

    if is_merge_in_progress {
        let mut abort_button = button(dirty_commit_action_label("Abort Merge"))
            .style(red_button_style(!busy.general))
            .padding(Padding::from([0, 16]))
            .height(Length::Fixed(DIRTY_COMMIT_ACTION_BUTTON_HEIGHT))
            .width(Length::Fill);
        if !busy.general {
            abort_button = abort_button.on_press(Message::repo(RepositoryMessage::Detail(
                DetailAction::AbortMergeConfirmed,
            )));
        }

        row![commit_button, abort_button]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill)
            .into()
    } else {
        commit_button.into()
    }
}

fn dirty_file_section<'a>(
    title: &'static str,
    files: &'a [ChangedFile],
    section: DirtyFileSection,
    _hovered_path: Option<&'a str>,
    active_diff_path: Option<&'a str>,
    available_width: f32,
    busy: DirtyActionBusy,
) -> Element<'a, Message> {
    let action_busy = busy.for_section(section);
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
            !action_busy,
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
                action_busy,
            )
        }));
    }

    column(rows).spacing(0).width(Length::Fill).into()
}

pub(super) fn dirty_action_button<'a>(
    label: &'static str,
    on_press: RepositoryMessage,
    tone: DirtyActionTone,
    enabled: bool,
) -> Element<'a, Message> {
    let btn = button(text(label).size(theme::FONT_XS)).padding(Padding::from([4, 8]));
    let btn = if enabled {
        btn.on_press(Message::repo(on_press))
    } else {
        btn
    };
    match tone {
        DirtyActionTone::Safe => btn.style(green_button_style(enabled)).into(),
        DirtyActionTone::Danger => btn.style(red_button_style(enabled)).into(),
        DirtyActionTone::Resolve => btn
            .style(super::styles::resolve_button_style(enabled))
            .into(),
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

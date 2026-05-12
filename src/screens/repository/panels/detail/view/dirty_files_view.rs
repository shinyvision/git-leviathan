//! Dirty-changes detail panel: working tree staging UI.
//!
//! Renders the three file sections (Conflicts / Unstaged / Staged) with
//! per-file and per-section action buttons, plus the commit-message editor
//! and Commit-All / Abort-Merge action row.

use iced::{
    keyboard,
    widget::{button, column, container, responsive, row, text, text_editor, Space},
    Element, Length, Padding,
};

use crate::{
    assets,
    core::{ChangedFile, Commit},
    message::Message,
    screens::repository::{
        panel_messages::{DetailAction, DetailFileListKind},
        RepositoryMessage,
    },
    style, theme,
    widgets::shared::{h_divider, horizontal_space, v_divider},
};

use super::super::state::{
    dirty_commit_message_editor_id, DetailViewModel, DirtyFileKey, DirtyFileStatus,
};
use super::styles::{detail_text_editor_style, green_button_style, red_button_style};
use super::{file_row_view, virtualized_file_list_view, FileRowContext, FILE_ROW_HEIGHT};

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
            Self::Unstaged => "Stage All Files",
            Self::Staged => "Unstage All Files",
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

    fn status(self) -> DirtyFileStatus {
        match self {
            Self::Conflicted => DirtyFileStatus::Conflicted,
            Self::Unstaged => DirtyFileStatus::Unstaged,
            Self::Staged => DirtyFileStatus::Staged,
        }
    }

    fn selected_action(
        self,
        counts: super::super::state::DirtySelectionCounts,
    ) -> Option<(String, RepositoryMessage)> {
        match self {
            Self::Unstaged if counts.total > 1 && counts.unstaged > 0 => Some((
                plural_action_label("Stage", counts.unstaged, "File", "Files"),
                RepositoryMessage::Detail(DetailAction::StageSelectedFiles),
            )),
            Self::Staged if counts.total > 1 && counts.staged > 0 => Some((
                plural_action_label("Unstage", counts.staged, "File", "Files"),
                RepositoryMessage::Detail(DetailAction::UnstageSelectedFiles),
            )),
            Self::Conflicted | Self::Unstaged | Self::Staged => None,
        }
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
        screen.dirty_selection_counts.total,
    );
    let stats_row = dirty_stats_row(screen.commit_diff_state, screen.dirty_operation_label);
    let file_list = dirty_file_list(
        commit,
        screen.selected_dirty_files.clone(),
        screen.dirty_selection_counts,
        screen.dirty_file_list_scroll_y,
        busy,
    );

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
        screen.dirty_selection_counts.total,
    );
    let stats_row = dirty_stats_row(screen.commit_diff_state, screen.dirty_operation_label);
    let file_list = dirty_file_list(
        commit,
        screen.selected_dirty_files.clone(),
        screen.dirty_selection_counts,
        screen.dirty_file_list_scroll_y,
        busy,
    );

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
    selected_count: usize,
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
        let discard_label = if selected_count > 1 {
            plural_action_label("Discard", selected_count, "Change", "Changes")
        } else {
            "Discard All Changes".to_string()
        };
        let discard_message = if selected_count > 1 {
            DetailAction::DiscardSelectedFilesRequested
        } else {
            DetailAction::DiscardAllRequested
        };
        let mut discard_button = button(text(discard_label).size(theme::FONT_XS))
            .style(red_button_style(has_any_dirty && !actions_busy))
            .padding(Padding::from([4, 8]));
        if has_any_dirty && !actions_busy {
            discard_button =
                discard_button.on_press(Message::repo(RepositoryMessage::Detail(discard_message)));
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
    let deleted = commit_diff_state.map(|d| d.deleted_count).unwrap_or(0);

    let mut stats_items: Vec<Element<Message>> = Vec::new();
    if modified > 0 {
        stats_items.push(
            row![
                assets::sidebar_icon(assets::PENCIL, theme::ACCENT_ORANGE),
                text(format!("{} modified", modified)).size(theme::FONT_SM),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center)
            .into(),
        );
    }
    if added > 0 {
        stats_items.push(
            row![
                assets::sidebar_icon(assets::PLUS, theme::ACCENT_GREEN),
                text(format!("{} added", added)).size(theme::FONT_SM),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center)
            .into(),
        );
    }
    if deleted > 0 {
        stats_items.push(
            row![
                assets::sidebar_icon(assets::MINUS, theme::ACCENT_RED),
                text(format!("{} removed", deleted)).size(theme::FONT_SM),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center)
            .into(),
        );
    }
    stats_items.push(horizontal_space().into());
    stats_items.push(
        text(operation_label.unwrap_or(""))
            .size(theme::FONT_SM)
            .style(style::dim_text)
            .into(),
    );

    row(stats_items)
        .padding(Padding::from([6, 10]))
        .spacing(10)
        .align_y(iced::Alignment::Center)
        .into()
}

fn dirty_file_list<'a>(
    commit: &'a Commit,
    selected_files: Vec<DirtyFileKey>,
    selection_counts: super::super::state::DirtySelectionCounts,
    scroll_y: f32,
    busy: DirtyActionBusy,
) -> Element<'a, Message> {
    let sections = [
        DirtySectionSpec {
            title: "Conflicts",
            files: &commit.conflicted_files,
            section: DirtyFileSection::Conflicted,
        },
        DirtySectionSpec {
            title: "Unstaged Files",
            files: &commit.unstaged_files,
            section: DirtyFileSection::Unstaged,
        },
        DirtySectionSpec {
            title: "Staged Files",
            files: &commit.staged_files,
            section: DirtyFileSection::Staged,
        },
    ];
    let row_count = dirty_file_row_count(&sections);

    virtualized_file_list_view(
        row_count,
        DetailFileListKind::Dirty,
        scroll_y,
        move |row_idx, available_width| {
            let Some((spec, row)) = dirty_file_row_at(&sections, row_idx) else {
                return Space::new()
                    .width(Length::Fill)
                    .height(Length::Fixed(FILE_ROW_HEIGHT))
                    .into();
            };

            match row {
                DirtyFileRow::Header => dirty_file_section_header(spec, selection_counts, busy),
                DirtyFileRow::Empty => dirty_empty_file_row(),
                DirtyFileRow::File(file_idx) => {
                    let action_busy = busy.for_section(spec.section);
                    let is_selected = is_dirty_file_selected(
                        &selected_files,
                        &spec.files[file_idx].path,
                        spec.section.status(),
                    );
                    file_row_view(
                        &spec.files[file_idx],
                        FileRowContext {
                            dirty_section: Some(spec.section),
                            commit_idx: None,
                            is_merged: false,
                            active_diff_path: None,
                            dirty_actions_busy: action_busy,
                            is_selected,
                        },
                        available_width,
                    )
                }
            }
        },
    )
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
        .id(dirty_commit_message_editor_id())
        .placeholder("Commit Message")
        .on_action(|action| {
            Message::repo(RepositoryMessage::Detail(
                DetailAction::CommitMessageAction(action),
            ))
        })
        .key_binding(commit_message_key_binding)
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
            .id(dirty_commit_message_editor_id())
            .placeholder("Commit Message")
            .on_action(|action| {
                Message::repo(RepositoryMessage::Detail(
                    DetailAction::CommitMessageAction(action),
                ))
            })
            .key_binding(commit_message_key_binding)
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

fn commit_message_key_binding(
    key_press: text_editor::KeyPress,
) -> Option<text_editor::Binding<Message>> {
    let is_ctrl_enter = matches!(key_press.status, text_editor::Status::Focused { .. })
        && key_press.modifiers.control()
        && matches!(
            key_press.key.as_ref(),
            keyboard::Key::Named(keyboard::key::Named::Enter)
        );

    if is_ctrl_enter {
        Some(text_editor::Binding::Custom(Message::repo(
            RepositoryMessage::Detail(DetailAction::CommitConfirmed),
        )))
    } else {
        text_editor::Binding::from_key_press(key_press)
    }
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

#[derive(Clone, Copy)]
struct DirtySectionSpec<'a> {
    title: &'static str,
    files: &'a [ChangedFile],
    section: DirtyFileSection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirtyFileRow {
    Header,
    Empty,
    File(usize),
}

fn dirty_file_row_count(sections: &[DirtySectionSpec<'_>; 3]) -> usize {
    sections
        .iter()
        .map(|section| 1 + section.files.len().max(1))
        .sum()
}

fn dirty_file_row_at<'a>(
    sections: &[DirtySectionSpec<'a>; 3],
    mut row_idx: usize,
) -> Option<(DirtySectionSpec<'a>, DirtyFileRow)> {
    for section in sections {
        if row_idx == 0 {
            return Some((*section, DirtyFileRow::Header));
        }
        row_idx -= 1;

        if section.files.is_empty() {
            if row_idx == 0 {
                return Some((*section, DirtyFileRow::Empty));
            }
            row_idx -= 1;
            continue;
        }

        if row_idx < section.files.len() {
            return Some((*section, DirtyFileRow::File(row_idx)));
        }
        row_idx -= section.files.len();
    }

    None
}

fn dirty_file_section_header<'a>(
    spec: DirtySectionSpec<'a>,
    selection_counts: super::super::state::DirtySelectionCounts,
    busy: DirtyActionBusy,
) -> Element<'a, Message> {
    let action_busy = busy.for_section(spec.section);
    let mut header = row![
        text(format!("{} ({})", spec.title, spec.files.len()))
            .size(theme::FONT_SM)
            .style(style::secondary_text),
        horizontal_space(),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .width(Length::Fill);

    if !spec.files.is_empty() {
        let (label, message) = spec
            .section
            .selected_action(selection_counts)
            .unwrap_or_else(|| {
                (
                    spec.section.all_action_label().to_string(),
                    spec.section.all_action_message(),
                )
            });
        header = header.push(dirty_action_button(
            label,
            message,
            spec.section.action_tone(),
            !action_busy,
        ));
    }

    container(header)
        .padding(Padding::from([0, 10]))
        .width(Length::Fill)
        .height(Length::Fixed(FILE_ROW_HEIGHT))
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

fn dirty_empty_file_row<'a>() -> Element<'a, Message> {
    container(text("No files").size(theme::FONT_SM).style(style::dim_text))
        .padding(Padding::from([0, 10]))
        .width(Length::Fill)
        .height(Length::Fixed(FILE_ROW_HEIGHT))
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

pub(super) fn dirty_action_button<'a>(
    label: impl Into<String>,
    on_press: RepositoryMessage,
    tone: DirtyActionTone,
    enabled: bool,
) -> Element<'a, Message> {
    let btn = button(text(label.into()).size(theme::FONT_XS)).padding(Padding::from([4, 8]));
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

fn is_dirty_file_selected(
    selected_files: &[DirtyFileKey],
    path: &str,
    status: DirtyFileStatus,
) -> bool {
    selected_files
        .iter()
        .any(|selected| selected.path == path && selected.status == status)
}

fn plural_action_label(action: &str, count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{action} 1 {singular}")
    } else {
        format!("{action} {count} {plural}")
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::state::DirtySelectionCounts;
    use super::*;
    use crate::core::ChangeKind;
    use crate::message::{ScreenMessage, ScreenRouted};

    fn changed(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.to_string(),
            kind: ChangeKind::Modified,
        }
    }

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

    #[test]
    fn commit_message_ctrl_enter_commits_when_focused() {
        let binding = commit_message_key_binding(key_press(
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::Modifiers::CTRL,
            text_editor::Status::Focused { is_hovered: false },
        ));

        let Some(text_editor::Binding::Custom(Message::Screen(ScreenRouted::Active(
            ScreenMessage::Repository(message),
        )))) = binding
        else {
            panic!("expected custom repository message");
        };

        assert!(matches!(
            *message,
            RepositoryMessage::Detail(DetailAction::CommitConfirmed)
        ));
    }

    #[test]
    fn commit_message_enter_keeps_default_editor_newline() {
        let binding = commit_message_key_binding(key_press(
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::Modifiers::default(),
            text_editor::Status::Focused { is_hovered: false },
        ));

        assert!(matches!(binding, Some(text_editor::Binding::Enter)));
    }

    #[test]
    fn commit_message_ctrl_enter_ignored_when_not_focused() {
        let binding = commit_message_key_binding(key_press(
            keyboard::Key::Named(keyboard::key::Named::Enter),
            keyboard::Modifiers::CTRL,
            text_editor::Status::Active,
        ));

        assert!(binding.is_none());
    }

    #[test]
    fn dirty_file_row_count_keeps_headers_and_empty_rows() {
        let conflicted = vec![changed("conflict.txt")];
        let unstaged = vec![changed("one.txt"), changed("two.txt")];
        let staged: Vec<ChangedFile> = Vec::new();
        let sections = [
            DirtySectionSpec {
                title: "Conflicts",
                files: &conflicted,
                section: DirtyFileSection::Conflicted,
            },
            DirtySectionSpec {
                title: "Unstaged Files",
                files: &unstaged,
                section: DirtyFileSection::Unstaged,
            },
            DirtySectionSpec {
                title: "Staged Files",
                files: &staged,
                section: DirtyFileSection::Staged,
            },
        ];

        assert_eq!(dirty_file_row_count(&sections), 7);
        assert_eq!(
            dirty_file_row_at(&sections, 0).map(|(_, row)| row),
            Some(DirtyFileRow::Header)
        );
        assert_eq!(
            dirty_file_row_at(&sections, 4).map(|(_, row)| row),
            Some(DirtyFileRow::File(1))
        );
        assert_eq!(
            dirty_file_row_at(&sections, 6).map(|(_, row)| row),
            Some(DirtyFileRow::Empty)
        );
    }

    #[test]
    fn dirty_section_header_uses_all_action_when_only_one_file_is_selected() {
        let counts = DirtySelectionCounts {
            total: 1,
            unstaged: 1,
            staged: 0,
        };

        assert!(DirtyFileSection::Unstaged.selected_action(counts).is_none());
        assert_eq!(
            DirtyFileSection::Unstaged.all_action_label(),
            "Stage All Files"
        );
    }

    #[test]
    fn dirty_section_header_uses_selected_action_for_multiple_selected_files() {
        let counts = DirtySelectionCounts {
            total: 2,
            unstaged: 1,
            staged: 1,
        };

        let (unstaged_label, _) = DirtyFileSection::Unstaged
            .selected_action(counts)
            .expect("unstaged selected action");
        let (staged_label, _) = DirtyFileSection::Staged
            .selected_action(counts)
            .expect("staged selected action");

        assert_eq!(unstaged_label, "Stage 1 File");
        assert_eq!(staged_label, "Unstage 1 File");
    }
}

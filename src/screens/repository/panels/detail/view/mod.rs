//! Detail panel views.
//!
//! The detail panel has three mutually-exclusive modes, each split into its
//! own sibling module:
//!
//! - single-commit view (this file) — normal commit / stash metadata + files.
//! - [`dirty_files_view`]   — working-tree uncommitted-changes staging UI.
//! - [`multi_commit_view`]  — merged-diff view when 2+ commits are selected.
//! - [`reword_view`]        — commit-message editor, embedded in single-commit.
//! - [`styles`]             — shared button/editor palettes.
//!
//! Shared primitives (`file_row_view`, `truncate_path_for_width`) live here
//! because every mode lists files.

mod dirty_files_view;
mod multi_commit_view;
mod reword_view;
mod styles;

use iced::{
    widget::{button, column, container, mouse_area, row, scrollable, text, MouseArea},
    Border, Color, Element, Length, Padding, Theme,
};

use crate::{
    assets,
    core::{ChangeKind, ChangedFile, Commit, CommitKind},
    message::Message,
    services::text_measurement::{cached_measure_width, FontFamily},
    style, theme,
    utils::{initials, split_path},
    widgets::{
        primitives::hoverable::{hoverable_swap, HoverStatus, Hoverable},
        shared::{h_divider, horizontal_space, scrollbar_style},
    },
};

use super::super::super::{
    panel_messages::{CenterAction, DetailAction},
    FileView, RepositoryMessage,
};
use super::state::DetailViewModel;

use dirty_files_view::{dirty_detail_panel_content, DirtyFileSection};

const DIRTY_FILE_ROW_HEIGHT: f32 = theme::ROW_H;

pub fn dirty_file_context_menu(
    state: &super::super::super::state::DirtyFileContextMenuState,
) -> Element<'static, Message> {
    use crate::widgets::context_menu::{context_menu_item, ContextMenu, ContextMenuItem};

    let path = state.path.clone();
    let items: Vec<ContextMenuItem> = vec![context_menu_item(
        "Discard Changes",
        Some(Message::repo(RepositoryMessage::Detail(
            DetailAction::DiscardFileRequested(path),
        ))),
    )];

    ContextMenu::new(items).into()
}

pub fn detail_panel_view(screen: DetailViewModel<'_>) -> Element<'_, Message> {
    let resize_handle = resize_handle_view(screen.is_resizing);
    let panel_content = detail_panel_content(screen);

    MouseArea::new(row![resize_handle, panel_content])
        .on_press(Message::repo(RepositoryMessage::Center(
            CenterAction::PanelFocused(super::super::super::state::FocusedPanel::Detail),
        )))
        .into()
}

fn resize_handle_view(is_resizing: bool) -> Element<'static, Message> {
    let handle = container(horizontal_space())
        .width(Length::Fixed(5.0))
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: if is_resizing {
                Some(theme::ACCENT_BLUE.into())
            } else {
                None
            },
            ..Default::default()
        });

    mouse_area(handle)
        .on_press(Message::repo(RepositoryMessage::Center(
            CenterAction::DetailResizeStarted,
        )))
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}

fn detail_panel_content(screen: DetailViewModel<'_>) -> Element<'_, Message> {
    let width = screen.width;
    if !screen.multi_commits.is_empty() {
        return multi_commit_view::multi_commit_detail_panel_content(screen);
    }
    let Some(commit) = screen.commit else {
        return container(
            text("Loading repository…")
                .size(theme::FONT_LG)
                .style(style::dim_text),
        )
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .style(style::panel_container)
        .into();
    };

    if commit.kind == CommitKind::Dirty {
        return dirty_detail_panel_content(screen, commit);
    }

    let hash_label = if commit.kind == CommitKind::Stash {
        "stash: "
    } else {
        "commit: "
    };
    let sha_text = text(&commit.short_hash)
        .size(theme::FONT_SM)
        .style(|_: &Theme| text::Style {
            color: Some(theme::ACCENT_BLUE),
        });
    let sha_clickable = mouse_area(sha_text)
        .interaction(iced::mouse::Interaction::Pointer)
        .on_press(Message::repo(RepositoryMessage::Detail(
            DetailAction::CopyCommitShaRequested(commit.hash.clone()),
        )));

    // Always render the tooltip container with identical dimensions; only
    // swap colors based on the flash state. This keeps the hash_bar's width
    // AND height constant so nothing shifts when the flash toggles.
    let flash = screen.copied_sha_flash;
    let tooltip_slot =
        container(
            text("Copied")
                .size(theme::FONT_SM)
                .style(move |_: &Theme| text::Style {
                    color: Some(if flash {
                        Color::WHITE
                    } else {
                        Color::TRANSPARENT
                    }),
                }),
        )
        .padding(Padding::from([3, 8]))
        .style(move |_: &Theme| container::Style {
            background: Some(
                if flash {
                    Color::BLACK
                } else {
                    Color::TRANSPARENT
                }
                .into(),
            ),
            border: Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let hash_bar = row![
        text(hash_label).size(theme::FONT_SM).style(style::dim_text),
        sha_clickable,
        iced::widget::Space::new().width(Length::Fixed(10.0)),
        tooltip_slot,
    ]
    .align_y(iced::Alignment::Center)
    .spacing(2)
    .padding(Padding::from([6, 10]));

    let commit_msg: Element<Message> = reword_view::reword_message_view(
        commit,
        screen.reword.clone(),
        screen.reword_allowed,
        screen.reword_descendant_count,
    );

    let author_row = row![
        container(
            text(initials(&commit.author))
                .size(theme::FONT_SM)
                .style(style::white_text)
        )
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(36.0))
        .style(|_: &Theme| container::Style {
            background: Some(Color::from_rgb(0.4, 0.3, 0.7).into()),
            border: Border {
                radius: 18.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center),
        column![
            text(&commit.author)
                .size(theme::FONT_MD)
                .style(style::primary_text),
            text(format!("authored {}", &commit.date))
                .size(theme::FONT_SM)
                .style(style::secondary_text),
        ]
        .spacing(2),
        horizontal_space(),
        parent_hash_column(commit),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .padding(Padding::from([6, 10]));

    let diff_modified = screen
        .commit_diff_state
        .map(|d| d.modified_count)
        .unwrap_or(0);
    let diff_added = screen.commit_diff_state.map(|d| d.added_count).unwrap_or(0);
    let stats_row = row![
        assets::sidebar_icon(assets::PENCIL, theme::TEXT_SECONDARY),
        text(format!("{} modified", diff_modified))
            .size(theme::FONT_SM)
            .style(style::secondary_text),
        text(format!("  + {} added", diff_added))
            .size(theme::FONT_SM)
            .style(|_: &Theme| text::Style {
                color: Some(theme::ACCENT_GREEN)
            }),
        horizontal_space(),
    ]
    .padding(Padding::from([6, 10]))
    .spacing(4)
    .align_y(iced::Alignment::Center);

    let is_path = screen.file_view == FileView::Path;

    let path_btn = button(text("Path").size(theme::FONT_SM))
        .style(move |_: &Theme, _: button::Status| button::Style {
            background: Some(
                if is_path {
                    theme::BG_HOVER
                } else {
                    theme::BG_HEADER
                }
                .into(),
            ),
            text_color: if is_path {
                Color::WHITE
            } else {
                theme::TEXT_DIM
            },
            border: Border {
                radius: 3.0.into(),
                color: theme::BORDER,
                width: 1.0,
            },
            shadow: Default::default(),
            snap: false,
        })
        .padding(Padding::from([3, 8]))
        .on_press(Message::repo(RepositoryMessage::Detail(
            DetailAction::FileViewChanged(FileView::Path),
        )));

    let tree_btn = button(text("Tree").size(theme::FONT_SM))
        .style(move |_: &Theme, _: button::Status| button::Style {
            background: Some(
                if !is_path {
                    theme::BG_HOVER
                } else {
                    theme::BG_HEADER
                }
                .into(),
            ),
            text_color: if !is_path {
                Color::WHITE
            } else {
                theme::TEXT_DIM
            },
            border: Border {
                radius: 3.0.into(),
                color: theme::BORDER,
                width: 1.0,
            },
            shadow: Default::default(),
            snap: false,
        })
        .padding(Padding::from([3, 8]))
        .on_press(Message::repo(RepositoryMessage::Detail(
            DetailAction::FileViewChanged(FileView::Tree),
        )));

    let view_toggle_row = row![
        assets::sidebar_icon(assets::ARROWS_SORT, theme::TEXT_DIM),
        path_btn,
        tree_btn,
        horizontal_space(),
        text("View all files")
            .size(theme::FONT_SM)
            .style(style::dim_text),
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center)
    .padding(Padding::from([4, 10]));

    let diff_loaded = screen
        .commit_diff_state
        .map(|d| d.diff_loaded)
        .unwrap_or(false);
    let commit_files: Option<&'_ [ChangedFile]> =
        screen.commit_diff_state.map(|d| d.files.as_slice());

    let file_rows: Vec<Element<Message>> = if !diff_loaded {
        vec![container(
            text("Loading files…")
                .size(theme::FONT_SM)
                .style(style::dim_text),
        )
        .padding(Padding::from([6, 10]))
        .into()]
    } else if commit_files.map(|f| f.is_empty()).unwrap_or(true) {
        // Genuinely zero-diff commit (e.g. a merge whose first-parent tree
        // matches the merge tree). Render an explicit empty state instead of
        // looping on "Loading files…".
        vec![container(
            text("No file changes")
                .size(theme::FONT_SM)
                .style(style::dim_text),
        )
        .padding(Padding::from([6, 10]))
        .into()]
    } else {
        commit_files
            .unwrap_or(&[])
            .iter()
            .map(|file| {
                file_row_view(
                    file,
                    None,
                    Some(screen.commit_idx),
                    false,
                    screen.active_diff_file_path,
                    width,
                )
            })
            .collect()
    };

    let file_list = scrollable(column(file_rows).spacing(0).width(Length::Fill))
        .height(Length::Fill)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(5).scroller_width(5),
        ))
        .style(scrollbar_style);

    let detail_col = column![
        hash_bar,
        h_divider(),
        commit_msg,
        author_row,
        h_divider(),
        stats_row,
        view_toggle_row,
        h_divider(),
        file_list,
    ]
    .spacing(0)
    .height(Length::Fill);

    container(detail_col)
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .style(style::panel_container)
        .into()
}

fn parent_hash_column<'a>(commit: &'a Commit) -> Element<'a, Message> {
    let label_text = if commit.parent_hashes.len() > 1 {
        "parents:"
    } else {
        "parent:"
    };
    let label = text(label_text).size(theme::FONT_XS).style(style::dim_text);

    let hashes_row: Element<'a, Message> = if commit.parent_hashes.is_empty() {
        text(&commit.parent_hash)
            .size(theme::FONT_SM)
            .style(|_: &Theme| text::Style {
                color: Some(theme::ACCENT_BLUE),
            })
            .into()
    } else {
        let mut items: Vec<Element<'a, Message>> = Vec::new();
        for (i, parent_hash) in commit.parent_hashes.iter().enumerate() {
            if i > 0 {
                items.push(
                    text(", ")
                        .size(theme::FONT_SM)
                        .style(style::dim_text)
                        .into(),
                );
            }
            let short = parent_hash.chars().take(7).collect::<String>();
            let hash_text = text(short)
                .size(theme::FONT_SM)
                .style(|_: &Theme| text::Style {
                    color: Some(theme::ACCENT_BLUE),
                });
            items.push(
                mouse_area(hash_text)
                    .interaction(iced::mouse::Interaction::Pointer)
                    .on_press(Message::repo(RepositoryMessage::Detail(
                        DetailAction::ParentCommitPressed(parent_hash.clone()),
                    )))
                    .into(),
            );
        }
        row(items).spacing(0).into()
    };

    column![label, hashes_row].spacing(2).into()
}

struct TruncatedPath {
    dir_display: String,
    file_display: String,
}

fn truncate_path_for_width(path: &str, max_width: f32) -> TruncatedPath {
    let font_size = theme::FONT_SM;
    let (dir_part, file_part) = split_path(path);

    // Measure full combined width
    let full_width = cached_measure_width(path, FontFamily::Default, font_size);
    if full_width <= max_width {
        return TruncatedPath {
            dir_display: dir_part.to_string(),
            file_display: file_part.to_string(),
        };
    }

    let file_width = cached_measure_width(file_part, FontFamily::Default, font_size);
    let separator_width = cached_measure_width("/", FontFamily::Default, font_size);
    let ellipsis_width = cached_measure_width("…", FontFamily::Default, font_size);

    // If even filename barely fits, show only filename
    let min_needed = file_width + separator_width + ellipsis_width;
    if max_width <= min_needed {
        if file_width <= max_width {
            return TruncatedPath {
                dir_display: String::new(),
                file_display: file_part.to_string(),
            };
        }
        let avail = (max_width - ellipsis_width).max(0.0);
        let chars: Vec<char> = file_part.chars().collect();
        let mut best = 0;
        let mut lo = 0;
        let mut hi = chars.len();
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let w = cached_measure_width(
                &file_part[..chars[..mid].iter().collect::<String>().len()],
                FontFamily::Default,
                font_size,
            );
            if w <= avail {
                best = mid;
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        if best == 0 {
            return TruncatedPath {
                dir_display: String::new(),
                file_display: "…".to_string(),
            };
        }
        let byte_end = chars[..best].iter().collect::<String>().len();
        return TruncatedPath {
            dir_display: String::new(),
            file_display: format!("{}…", &file_part[..byte_end]),
        };
    }

    let dir_avail = max_width - file_width - separator_width - ellipsis_width;
    if dir_part.is_empty() || dir_avail <= 0.0 {
        return TruncatedPath {
            dir_display: "…/".to_string(),
            file_display: file_part.to_string(),
        };
    }

    let dir_chars: Vec<char> = dir_part.chars().collect();
    let mut best = 0;
    let mut lo = 0;
    let mut hi = dir_chars.len();
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let substr: String = dir_chars[..mid].iter().collect();
        let w = cached_measure_width(&substr, FontFamily::Default, font_size);
        if w <= dir_avail {
            best = mid;
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }

    if best == 0 {
        TruncatedPath {
            dir_display: "…/".to_string(),
            file_display: file_part.to_string(),
        }
    } else {
        let byte_end = dir_chars[..best].iter().collect::<String>().len();
        TruncatedPath {
            dir_display: format!("{}…/", &dir_part[..byte_end]),
            file_display: file_part.to_string(),
        }
    }
}

fn file_row_view<'a>(
    file: &'a ChangedFile,
    dirty_section: Option<DirtyFileSection>,
    commit_idx: Option<usize>,
    is_merged: bool,
    active_diff_path: Option<&'a str>,
    available_width: f32,
) -> Element<'a, Message> {
    fn make_indicator(kind: &ChangeKind) -> Element<'static, Message> {
        match kind {
            ChangeKind::Modified => assets::sidebar_icon(assets::PENCIL, theme::ACCENT_ORANGE),
            ChangeKind::Added => assets::sidebar_icon(assets::PLUS, theme::ACCENT_GREEN),
            ChangeKind::Deleted => assets::sidebar_icon(assets::MINUS, theme::ACCENT_RED),
        }
    }

    let is_active_diff = active_diff_path == Some(file.path.as_str());

    let row_padding = if dirty_section.is_some() {
        Padding::from([0, 10])
    } else {
        Padding::from([3, 10])
    };

    if let Some(section) = dirty_section {
        let indicator_width = 14.0;
        let row_padding_h = 10.0;
        let row_h_padding = Padding {
            top: 0.0,
            right: row_padding_h,
            bottom: 0.0,
            left: row_padding_h,
        };
        let text_max_width =
            (available_width - indicator_width - row_padding_h * 2.0 - 9.0).max(0.0);
        let truncated = truncate_path_for_width(&file.path, text_max_width);
        let trunc_dir = truncated.dir_display;
        let trunc_file = truncated.file_display;
        let trunc_dir_hover = trunc_dir.clone();
        let trunc_file_hover = trunc_file.clone();

        let action_button = dirty_files_view::dirty_action_button(
            section.file_action_label(),
            section.file_action_message(file.path.clone()),
            section.action_tone(),
        );

        let path_row = row![
            container(make_indicator(&file.kind))
                .width(Length::Fixed(indicator_width))
                .align_x(iced::alignment::Horizontal::Center),
            text(trunc_dir).size(theme::FONT_SM).style(style::path_text),
            text(trunc_file)
                .size(theme::FONT_SM)
                .style(style::primary_text),
            horizontal_space(),
        ]
        .spacing(0)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill);

        let idle_content = container(path_row)
            .padding(row_h_padding)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(iced::alignment::Vertical::Center);

        let button_overlay = container(action_button)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(row_h_padding)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Center);

        let hover_underlay = container(
            row![
                container(make_indicator(&file.kind))
                    .width(Length::Fixed(indicator_width))
                    .align_x(iced::alignment::Horizontal::Center),
                text(trunc_dir_hover)
                    .size(theme::FONT_SM)
                    .style(style::path_text),
                text(trunc_file_hover)
                    .size(theme::FONT_SM)
                    .style(style::primary_text),
                horizontal_space(),
            ]
            .spacing(0)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill),
        )
        .padding(row_h_padding)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Center);

        use iced::widget::Stack;
        let hover_content =
            Stack::with_children(vec![hover_underlay.into(), button_overlay.into()])
                .width(Length::Fill)
                .height(Length::Fill);

        let idle_style = move |_: &Theme| container::Style {
            background: if is_active_diff {
                Some(theme::BG_SELECTED.into())
            } else {
                None
            },
            ..Default::default()
        };

        let hover_style = move |_: &Theme| container::Style {
            background: Some(
                if is_active_diff {
                    theme::BG_SELECTED
                } else {
                    theme::BG_HOVER
                }
                .into(),
            ),
            ..Default::default()
        };

        let on_click_msg =
            Message::repo(RepositoryMessage::Detail(DetailAction::DirtyFileClicked {
                path: file.path.clone(),
                is_staged: section.is_staged_for_diff(),
            }));

        let idle_row = container(idle_content)
            .width(Length::Fill)
            .height(Length::Fixed(DIRTY_FILE_ROW_HEIGHT))
            .align_y(iced::alignment::Vertical::Center);

        let hover_row = container(hover_content)
            .width(Length::Fill)
            .height(Length::Fixed(DIRTY_FILE_ROW_HEIGHT))
            .align_y(iced::alignment::Vertical::Center);

        let right_click_msg = Message::repo(RepositoryMessage::Detail(
            DetailAction::DirtyFileRightClicked(file.path.clone()),
        ));
        MouseArea::new(hoverable_swap(idle_row, hover_row, idle_style, hover_style))
            .on_press(on_click_msg)
            .on_right_press(right_click_msg)
            .into()
    } else {
        let indicator_width = 14.0;
        let row_padding_h = 10.0;
        let text_max_width =
            (available_width - indicator_width - row_padding_h * 2.0 - 9.0).max(0.0);
        let truncated = truncate_path_for_width(&file.path, text_max_width);

        let row_content = row![
            container(make_indicator(&file.kind))
                .width(Length::Fixed(indicator_width))
                .align_x(iced::alignment::Horizontal::Center),
            text(truncated.dir_display)
                .size(theme::FONT_SM)
                .style(style::path_text),
            text(truncated.file_display)
                .size(theme::FONT_SM)
                .style(style::primary_text),
            horizontal_space(),
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill);

        let idle_style = move |_: &Theme| container::Style {
            background: if is_active_diff {
                Some(theme::BG_SELECTED.into())
            } else {
                None
            },
            ..Default::default()
        };

        let hover_style = move |_: &Theme| container::Style {
            background: Some(
                if is_active_diff {
                    theme::BG_SELECTED
                } else {
                    theme::BG_HOVER
                }
                .into(),
            ),
            ..Default::default()
        };

        let idle_row = container(row_content)
            .padding(row_padding)
            .width(Length::Fill)
            .height(Length::Fixed(DIRTY_FILE_ROW_HEIGHT))
            .align_y(iced::alignment::Vertical::Center);

        let interactive = Hoverable::new(idle_row, move |theme, status: HoverStatus| {
            if status.is_hovered() {
                hover_style(theme)
            } else {
                idle_style(theme)
            }
        });

        if let Some(idx) = commit_idx {
            let on_click_msg =
                Message::repo(RepositoryMessage::Detail(DetailAction::CommitFileClicked {
                    commit_idx: idx,
                    path: file.path.clone(),
                }));
            MouseArea::new(interactive).on_press(on_click_msg).into()
        } else if is_merged {
            let on_click_msg =
                Message::repo(RepositoryMessage::Detail(DetailAction::MergedFileClicked {
                    path: file.path.clone(),
                }));
            MouseArea::new(interactive).on_press(on_click_msg).into()
        } else {
            interactive.into()
        }
    }
}

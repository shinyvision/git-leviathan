use iced::{
    advanced::text::Shaping,
    widget::{
        button, column, container, responsive, row, scrollable, text, text_editor, MouseArea, Stack,
    },
    Border, Color, Element, Length, Padding, Theme,
};

use crate::assets;

use crate::{
    core::{Commit, CommitKind},
    message::Message,
    style, theme,
    view_model::{CommitDiffState, CommitPresentation},
    widgets::{
        branch_label::{
            any_name_truncated, branch_label_cell, branch_popout_content_id, branch_popout_panel,
            branch_popout_panel_id, branch_stack_background,
        },
        full_graph_widget,
        shared::{horizontal_space, list_vertical_spacer, scrollbar_style},
        ROW_H,
    },
};

use super::super::super::{
    commit_search, panel_messages::CenterAction, state::center_list_scroll_id, RepositoryMessage,
};
use super::state::CenterViewModel;

const COMMIT_LIST_OVERSCAN_ROWS: usize = 10;

pub(crate) fn center_panel_view(screen: CenterViewModel<'_>) -> Element<'_, Message> {
    let graph_col_width = crate::widgets::graph::graph_column_width(screen.num_lanes);

    let header = row![
        container(
            text("BRANCH / TAG")
                .size(theme::FONT_XS)
                .style(style::dim_text)
        )
        .width(Length::Fixed(theme::BRANCH_COL_WIDTH as f32))
        .padding(Padding::from([4, 8])),
        container(text("GRAPH").size(theme::FONT_XS).style(style::dim_text))
            .width(Length::Fixed(graph_col_width))
            .padding(Padding::from([4, 4])),
        container(
            text("COMMIT MESSAGE")
                .size(theme::FONT_XS)
                .style(style::dim_text)
        )
        .padding(Padding::from([4, 8])),
        horizontal_space(),
    ]
    .height(Length::Fixed(22.0));

    let header_container = container(header)
        .width(Length::Fill)
        .style(style::header_container);

    let commit_search_ref = screen.commit_search;

    let list_scroll = scrollable(responsive(move |size| {
        build_list_contents(&screen, graph_col_width, size.width)
    }))
    .id(center_list_scroll_id())
    .height(Length::Fill)
    .width(Length::Fill)
    .on_scroll(|viewport| {
        Message::repo(RepositoryMessage::Center(CenterAction::CenterListScrolled(
            viewport,
        )))
    })
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::new().width(5).scroller_width(5),
    ))
    .style(scrollbar_style);

    let panel_content: Element<Message> = column![header_container, list_scroll]
        .spacing(0)
        .height(Length::Fill)
        .into();

    // Overlay the floating commit-search bar on top of the panel content so
    // it sits in the top-right corner of the graph area.
    let panel_with_search = commit_search::overlay_if_active(panel_content, commit_search_ref);

    MouseArea::new(
        container(panel_with_search)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::panel_container),
    )
    .on_press(Message::repo(RepositoryMessage::Center(
        CenterAction::PanelFocused(super::super::super::state::FocusedPanel::Center),
    )))
    .into()
}

fn build_list_contents<'a>(
    screen: &CenterViewModel<'a>,
    graph_col_width: f32,
    viewport_w: f32,
) -> Element<'a, Message> {
    let total_h = screen.commits.len() as f32 * ROW_H;
    let visible_range = visible_commit_range(
        screen.commits.len(),
        screen.center_list_offset_y,
        screen.center_list_viewport_h,
    );
    let visible_start = visible_range.start;
    let visible_end = visible_range.end;
    let top_spacer_h = visible_start as f32 * ROW_H;
    let bottom_spacer_h = (screen.commits.len().saturating_sub(visible_end)) as f32 * ROW_H;

    let active_popout = screen.branch_popout.as_ref();
    let active_popout_commit = active_popout.map(|state| state.commit_idx);

    let mut branch_col: Vec<Element<Message>> = vec![list_vertical_spacer(top_spacer_h)];
    branch_col.extend(
        screen.commits[visible_start..visible_end]
            .iter()
            .enumerate()
            .map(|(i, _commit)| {
                let commit_idx = visible_start + i;
                branch_label_cell(
                    &screen.commit_presentations[commit_idx].branch_display_rows,
                    screen.selected_indices.binary_search(&commit_idx).is_ok(),
                    commit_idx,
                    active_popout_commit,
                )
            }),
    );
    branch_col.push(list_vertical_spacer(bottom_spacer_h));

    let graph_col = column![full_graph_widget(
        screen.graph_rows,
        screen.selected_indices.clone(),
        screen.num_lanes,
        screen.graph_revision,
        visible_start,
        visible_end,
    )]
    .spacing(0)
    .width(Length::Fixed(graph_col_width));

    let msg_col_w = (viewport_w - theme::BRANCH_COL_WIDTH as f32 - graph_col_width).max(80.0);

    let search_dimming = screen.commit_search.is_some_and(|s| s.is_dimming_active());

    let mut msg_col: Vec<Element<Message>> = vec![list_vertical_spacer(top_spacer_h)];
    msg_col.extend(
        screen.commits[visible_start..visible_end]
            .iter()
            .enumerate()
            .map(|(i, commit)| {
                let commit_idx = visible_start + i;
                let is_muted = search_dimming
                    && screen
                        .commit_search
                        .is_some_and(|s| !s.is_match(commit_idx));
                message_cell(MessageCellData {
                    commit,
                    presentation: &screen.commit_presentations[commit_idx],
                    diff_state: screen.commit_diff_states.get(commit_idx),
                    is_selected: screen.selected_indices.binary_search(&commit_idx).is_ok(),
                    idx: commit_idx,
                    available_width: msg_col_w - 5.0,
                    dirty_commit_message: screen.dirty_commit_message,
                    is_muted,
                })
            }),
    );
    msg_col.push(list_vertical_spacer(bottom_spacer_h));

    let list_row = row![
        column(branch_col)
            .spacing(0)
            .width(Length::Fixed(theme::BRANCH_COL_WIDTH as f32)),
        graph_col,
        column(msg_col).spacing(0).width(Length::Fill),
    ]
    .height(Length::Fixed(total_h));

    // Floating dropdown overlay for the active popout commit.
    // Two cases share the same Stack mechanism:
    //   • Overflow:      extra labels exist  → show all rows (trigger + extras)
    //   • Name reveal:   no extra labels but the displayed name is truncated
    //                    → show the same label(s) with the full name
    let dropdown: Option<Element<Message>> = active_popout.and_then(|state| {
        let idx = state.commit_idx;
        let commit_presentation = screen.commit_presentations.get(idx)?;
        let rows = &commit_presentation.branch_display_rows;

        let row_count = rows.len();
        let has_truncated = any_name_truncated(rows);
        let (panel_rows, max_name_len) = if row_count <= 1 {
            // No overflow — only open if a name is actually truncated.
            if !has_truncated {
                return None;
            }
            // Show the trigger row(s) with the full name (usize::MAX → no clip).
            (rows, usize::MAX)
        } else {
            // Overflow popout: all rows, generous name limit.
            (rows, usize::MAX)
        };

        let mut panel_rows_owned: Vec<_> = panel_rows.to_vec();
        if let Some(current_idx) = panel_rows_owned.iter().position(|r| r.is_current) {
            if current_idx != 0 {
                let current = panel_rows_owned.remove(current_idx);
                panel_rows_owned.insert(0, current);
            }
        }
        let panel_bg = branch_stack_background(panel_rows_owned.first()?);
        let panel_len = panel_rows_owned.len();
        let lazy_panel = iced::widget::lazy(
            (idx, screen.graph_revision, max_name_len, panel_len),
            move |_| branch_popout_panel(&panel_rows_owned, panel_bg, max_name_len),
        );
        let popout_panel = container(lazy_panel).id(branch_popout_panel_id());

        // Only render once the real trigger bounds are known so the popout
        // appears in its final position instead of jumping into place.
        let (Some(trigger_bounds), Some(content_bounds)) =
            (state.trigger_bounds, state.content_bounds)
        else {
            return None;
        };

        let x_left = (trigger_bounds.x - content_bounds.x).max(0.0);
        let y_top = (trigger_bounds.y - content_bounds.y + screen.center_list_offset_y).max(0.0);
        let positioned = container(popout_panel).padding(Padding {
            top: y_top,
            right: 0.0,
            bottom: 0.0,
            left: x_left,
        });
        Some(positioned.into())
    });

    let scrollable_content: Element<Message> = match dropdown {
        Some(dd) => Stack::with_children(vec![container(list_row).width(Length::Fill).into(), dd])
            .width(Length::Fill)
            .into(),
        None => container(list_row).width(Length::Fill).into(),
    };
    container(scrollable_content)
        .id(branch_popout_content_id())
        .into()
}

fn visible_commit_range(
    total_commits: usize,
    offset_y: f32,
    viewport_h: f32,
) -> std::ops::Range<usize> {
    if total_commits == 0 {
        return 0..0;
    }

    let first_visible = (offset_y.max(0.0) / ROW_H).floor() as usize;
    let last_visible = ((offset_y.max(0.0) + viewport_h.max(ROW_H)) / ROW_H).ceil() as usize;

    let start = first_visible.saturating_sub(COMMIT_LIST_OVERSCAN_ROWS);
    let end = (last_visible + COMMIT_LIST_OVERSCAN_ROWS).min(total_commits);

    start.min(end)..end
}
fn dirty_message_row<'a>(
    _commit: &'a Commit,
    diff_state: Option<&'a CommitDiffState>,
    editor_content: &'a text_editor::Content,
) -> iced::widget::Row<'a, Message> {
    let modified_count = diff_state.map(|d| d.modified_count).unwrap_or(0);
    let added_count = diff_state.map(|d| d.added_count).unwrap_or(0);
    let deleted_count = diff_state.map(|d| d.deleted_count).unwrap_or(0);
    // First line of typed message, or "// WIP" if nothing typed yet.
    let badge_label = {
        let typed = editor_content.text();
        typed
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("// WIP")
            .to_string()
    };

    let badge_bg = theme::BG_HEADER;
    let wip_badge =
        container(
            text(badge_label)
                .size(theme::FONT_SM)
                .style(|_: &Theme| text::Style {
                    color: Some(theme::TEXT_SECONDARY),
                }),
        )
        .padding(Padding {
            top: 2.0,
            right: 7.0,
            bottom: 2.0,
            left: 7.0,
        })
        .style(move |_: &Theme| container::Style {
            background: Some(badge_bg.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });

    let mut items: Vec<Element<Message>> = vec![wip_badge.into()];

    if modified_count > 0 {
        items.push(assets::icon(assets::PENCIL, 11.0, theme::ACCENT_ORANGE));
        items.push(
            text(modified_count.to_string())
                .size(theme::FONT_SM)
                .style(|_: &Theme| text::Style {
                    color: Some(theme::TEXT_PRIMARY),
                })
                .into(),
        );
    }

    if added_count > 0 {
        items.push(
            row![
                assets::icon(assets::PLUS, 12.0, theme::ACCENT_GREEN),
                text(added_count.to_string())
                    .size(theme::FONT_SM)
                    .style(|_: &Theme| text::Style {
                        color: Some(theme::TEXT_PRIMARY),
                    }),
            ]
            .spacing(2)
            .align_y(iced::Alignment::Center)
            .into(),
        );
    }

    if deleted_count > 0 {
        items.push(
            row![
                assets::icon(assets::MINUS, 12.0, theme::ACCENT_RED),
                text(deleted_count.to_string())
                    .size(theme::FONT_SM)
                    .style(|_: &Theme| text::Style {
                        color: Some(theme::TEXT_PRIMARY),
                    }),
            ]
            .spacing(2)
            .align_y(iced::Alignment::Center)
            .into(),
        );
    }

    items.push(horizontal_space().into());

    row(items)
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
}

struct MessageCellData<'a> {
    commit: &'a Commit,
    presentation: &'a CommitPresentation,
    diff_state: Option<&'a CommitDiffState>,
    is_selected: bool,
    idx: usize,
    available_width: f32,
    dirty_commit_message: &'a text_editor::Content,
    is_muted: bool,
}

fn message_cell<'a>(data: MessageCellData<'a>) -> Element<'a, Message> {
    let MessageCellData {
        commit,
        presentation,
        diff_state,
        is_selected,
        idx,
        available_width,
        dirty_commit_message,
        is_muted,
    } = data;

    let bg = if is_selected {
        theme::BG_SELECTED
    } else {
        theme::BG_PANEL
    };
    let msg_color = if is_muted {
        theme::TEXT_MUTED
    } else if is_selected {
        Color::WHITE
    } else {
        theme::TEXT_PRIMARY
    };

    let inner_row = if commit.kind == CommitKind::Dirty {
        dirty_message_row(commit, diff_state, dirty_commit_message)
    } else {
        let summary = commit.message.lines().next().unwrap_or("").to_string();
        let rest: String = commit.message.lines().skip(1).collect::<Vec<_>>().join(" ");
        let rest = rest.trim().to_string();
        let combined = if rest.is_empty() {
            summary.clone()
        } else {
            format!("{} {}", summary, rest)
        };
        use crate::services::{cached_measure_width, FontFamily};

        // Fixed overhead: button padding (8 + 8) + row spacing between 3 children (8 + 8).
        const ROW_OVERHEAD: f32 = 32.0;
        let time_width = presentation
            .relative_time
            .as_deref()
            .map(|t| cached_measure_width(t, FontFamily::Default, theme::FONT_XS))
            .unwrap_or(0.0);
        let msg_avail = (available_width - ROW_OVERHEAD - time_width).max(0.0);

        let truncated = truncate_message_for_width(&combined, msg_avail);
        let summary_prefix = format!("{} ", summary);
        let (summary_part, rest_part) =
            if !rest.is_empty() && truncated.starts_with(&summary_prefix) {
                (
                    summary.clone(),
                    truncated[summary_prefix.len()..].to_string(),
                )
            } else {
                (truncated, String::new())
            };
        let mut message_row_items: Vec<Element<Message>> = vec![text(summary_part)
            .size(theme::FONT_MD)
            .style(move |_: &Theme| text::Style {
                color: Some(msg_color),
            })
            .shaping(Shaping::Advanced)
            .into()];
        if !rest_part.is_empty() {
            message_row_items.push(
                text(format!(" {}", rest_part))
                    .size(theme::FONT_MD)
                    .style(|_: &Theme| text::Style {
                        color: Some(theme::TEXT_MUTED),
                    })
                    .shaping(Shaping::Advanced)
                    .into(),
            );
        }
        let message_row = row(message_row_items)
            .spacing(0)
            .align_y(iced::Alignment::Center);
        let mut row_items: Vec<Element<Message>> =
            vec![message_row.into(), horizontal_space().into()];
        if let Some(rel_time) = &presentation.relative_time {
            row_items.push(
                text(rel_time)
                    .size(theme::FONT_XS)
                    .style(style::dim_text)
                    .into(),
            );
        }
        row(row_items)
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill)
    };

    let centered = container(inner_row)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Center);

    // Wrap the button in a MouseArea to capture click events on press (mouse down)
    let commit_button = button(centered)
        .style(move |_: &Theme, _: button::Status| button::Style {
            background: Some(bg.into()),
            text_color: msg_color,
            border: Border::default(),
            shadow: Default::default(),
            snap: false,
        })
        .padding(Padding::from([0, 8]))
        .width(Length::Fill)
        .height(Length::Fixed(ROW_H));

    MouseArea::new(commit_button)
        .on_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CommitSelected(idx),
        )))
        .on_right_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CommitRightClicked { commit_idx: idx },
        )))
        .into()
}

fn truncate_message_for_width(message: &str, available_width: f32) -> String {
    use crate::services::{cached_measure_width, FontFamily};

    let full_width = cached_measure_width(message, FontFamily::Default, theme::FONT_MD);

    if full_width <= available_width {
        return message.to_string();
    }

    let ellipsis_width = cached_measure_width("…", FontFamily::Default, theme::FONT_MD);
    let available_for_text = available_width - ellipsis_width;

    if available_for_text <= 0.0 {
        return "…".to_string();
    }

    let mut low = 0usize;
    let mut high = message.len();
    let mut best_byte = 0usize;

    while low < high {
        let mid = (low + high).div_ceil(2);
        let boundary = floor_char_boundary(message, mid.min(message.len()));
        let substr = &message[..boundary];
        let w = cached_measure_width(substr, FontFamily::Default, theme::FONT_MD);
        if w <= available_for_text {
            best_byte = substr.len();
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    if best_byte == 0 {
        "…".to_string()
    } else {
        let boundary = floor_char_boundary(message, best_byte.min(message.len()));
        format!("{}…", &message[..boundary])
    }
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

pub fn commit_context_menu(
    state: &crate::screens::repository::state::CommitContextMenuState,
    current_branch: &str,
    head_hash: Option<&str>,
) -> Element<'static, Message> {
    use crate::widgets::context_menu::{context_menu_item, ContextMenu};

    let commit_idx = state.commit_idx;
    let commit_hash = &state.commit_hash;

    let items: Vec<crate::widgets::context_menu::ContextMenuItem> =
        if let Some(stash_index) = state.stash_index {
            let display_name = state
                .stash_display_name
                .clone()
                .unwrap_or_else(|| format!("stash@{{{}}}", stash_index));
            vec![
                context_menu_item(
                    "Apply stash",
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::StashApplyRequested { stash_index },
                    ))),
                ),
                context_menu_item(
                    "Pop stash",
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::StashPopRequested { stash_index },
                    ))),
                ),
                context_menu_item(
                    "Delete stash",
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::StashDeleteRequested {
                            stash_index,
                            display_name,
                        },
                    ))),
                ),
            ]
        } else {
            let is_head_commit = head_hash == Some(commit_hash.as_str());
            let mut items = vec![
                context_menu_item(
                    "Copy commit hash",
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::CopyBranchName(commit_hash.clone()),
                    ))),
                ),
                context_menu_item(
                    "Create branch here",
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::CreateBranchHereRequested {
                            commit_idx,
                            commit_hash: commit_hash.clone(),
                        },
                    ))),
                ),
                context_menu_item(
                    "Create tag here",
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::CreateTagHereRequested {
                            commit_hash: commit_hash.clone(),
                        },
                    ))),
                ),
            ];
            if !is_head_commit {
                items.push(context_menu_item(
                    "Cherry pick commit",
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::CherryPickRequested {
                            commit_hash: commit_hash.clone(),
                        },
                    ))),
                ));
            }
            if state.selected_indices.len() > 1 {
                items.push(context_menu_item(
                    format!("Squash {} commits", state.selected_indices.len()),
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::SquashCommitsRequested {
                            indices: state.selected_indices.clone(),
                            hashes: state.selected_hashes.clone(),
                        },
                    ))),
                ));
            }
            if !current_branch.is_empty() && !is_head_commit {
                use crate::widgets::context_menu::context_menu_item_with_hover;
                let enter_hash = commit_hash.clone();
                let exit_hash = commit_hash.clone();
                items.push(context_menu_item_with_hover(
                    format!("Reset {} to this commit  ›", current_branch),
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::ResetSubmenuRequested {
                            commit_idx,
                            commit_hash: commit_hash.clone(),
                        },
                    ))),
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::ResetParentHoverChanged {
                            commit_hash: enter_hash,
                            hovered: true,
                        },
                    ))),
                    Some(Message::repo(RepositoryMessage::Center(
                        CenterAction::ResetParentHoverChanged {
                            commit_hash: exit_hash,
                            hovered: false,
                        },
                    ))),
                ));
            }
            items
        };

    ContextMenu::new(items).into()
}

/// Approximate width (px) of the commit context menu given current contents.
/// Used to place the reset submenu at the far right of the parent menu.
pub fn commit_context_menu_width(
    state: &crate::screens::repository::state::CommitContextMenuState,
    current_branch: &str,
    head_hash: Option<&str>,
) -> f32 {
    use crate::services::{cached_measure_width, FontFamily};

    let mut labels: Vec<String> = if state.stash_index.is_some() {
        vec![
            "Apply stash".into(),
            "Pop stash".into(),
            "Delete stash".into(),
        ]
    } else {
        let is_head_commit = head_hash == Some(state.commit_hash.as_str());
        let mut v = vec![
            "Copy commit hash".into(),
            "Create branch here".into(),
            "Create tag here".into(),
        ];
        if !is_head_commit {
            v.push("Cherry pick commit".into());
        }
        if state.selected_indices.len() > 1 {
            v.push(format!("Squash {} commits", state.selected_indices.len()));
        }
        if !current_branch.is_empty() && !is_head_commit {
            v.push(format!("Reset {} to this commit  ›", current_branch));
        }
        v
    };
    labels.sort_by_key(|b| std::cmp::Reverse(b.len()));
    let max_w = labels
        .iter()
        .map(|l| cached_measure_width(l, FontFamily::Default, theme::FONT_SM))
        .fold(0.0_f32, f32::max);
    // PADDING (4) + BUTTON_PADDING_X (10) on each side = 28
    max_w + 28.0
}

pub fn reset_submenu(
    state: &crate::screens::repository::state::ResetSubmenuState,
) -> Element<'static, Message> {
    use crate::services::ResetMode;
    use crate::widgets::context_menu::{context_menu_item, ContextMenu};

    let commit_hash = state.commit_hash.clone();
    let hash_mixed = commit_hash.clone();
    let hash_hard = commit_hash.clone();

    let items = vec![
        context_menu_item(
            "Soft - keep all changes",
            Some(Message::repo(RepositoryMessage::Center(
                CenterAction::ResetToCommitRequested {
                    commit_hash,
                    mode: ResetMode::Soft,
                },
            ))),
        ),
        context_menu_item(
            "Mixed - keep working copy but reset index",
            Some(Message::repo(RepositoryMessage::Center(
                CenterAction::ResetToCommitRequested {
                    commit_hash: hash_mixed,
                    mode: ResetMode::Mixed,
                },
            ))),
        ),
        context_menu_item(
            "Hard - discard all changes",
            Some(Message::repo(RepositoryMessage::Center(
                CenterAction::ResetToCommitRequested {
                    commit_hash: hash_hard,
                    mode: ResetMode::Hard,
                },
            ))),
        ),
    ];

    ContextMenu::new(items).into()
}

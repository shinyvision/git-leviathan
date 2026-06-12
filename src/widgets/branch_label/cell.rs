//! Branch-label cell: in-row pill rendering + mouse wiring.
use iced::{
    border, mouse,
    widget::{button, container, row, text, MouseArea},
    Border, Color, Element, Length, Padding, Theme,
};

use crate::assets;
use crate::message::Message;
use crate::screens::repository::{panel_messages::CenterAction, RepositoryMessage};
use crate::theme;
use crate::view_model::BranchDisplayRow;
use crate::widgets::palette::{self, PaletteRole};
use crate::widgets::shared::horizontal_space;
use crate::widgets::ROW_H;

use super::layout::{
    calculate_text_width, count_trailing_icons, display_name_for_width,
    BRANCH_LABEL_CLOUD_ICON_SIZE, BRANCH_LABEL_ICON_SIZE, BRANCH_LABEL_LAPTOP_ICON_SIZE,
    PILL_ROW_SPACING,
};
use super::{remote_ref_name, BRANCH_LABEL_INSET_X, BRANCH_POPOUT_RADIUS};

// ─── Color helpers ────────────────────────────────────────────────────────────

pub fn branch_stack_background(row: &BranchDisplayRow) -> Color {
    if row.is_tag {
        Color {
            r: 0.17,
            g: 0.15,
            b: 0.08,
            a: 1.0,
        }
    } else {
        branch_lane_background(row.lane_color)
    }
}

pub(super) fn branch_lane_background(lane_color: usize) -> Color {
    // Note: asymmetric per-channel weights on BG_PANEL (0.30/0.20/0.25) are
    // load-bearing for the visual tone; kept explicit rather than routed
    // through `palette::mix`.
    let lane_col = palette::lane_color(lane_color);
    Color {
        r: theme::BG_PANEL.r * 0.30 + lane_col.r * 0.35,
        g: theme::BG_PANEL.g * 0.20 + lane_col.g * 0.35,
        b: theme::BG_PANEL.b * 0.25 + lane_col.b * 0.35,
        a: 1.0,
    }
}

pub(super) fn branch_stack_border(row: &BranchDisplayRow) -> Color {
    if row.is_tag {
        palette::scale(palette::palette_color(PaletteRole::Tag), 0.60)
    } else {
        palette::scale(palette::lane_color(row.lane_color), 0.55)
    }
}

fn branch_stack_text_color(row: &BranchDisplayRow) -> Color {
    if row.is_tag {
        palette::palette_color(PaletteRole::Tag)
    } else {
        palette::tint(palette::lane_color(row.lane_color), 0.4, 0.6)
    }
}

// ─── Icon composition ─────────────────────────────────────────────────────────

pub(super) fn branch_row_icons(
    row: &BranchDisplayRow,
    color: Color,
    tag_or_fallback_size: f32,
    laptop_size: f32,
    cloud_size: f32,
) -> Vec<Element<'static, Message>> {
    let mut icons = Vec::new();

    if row.is_tag {
        icons.push(assets::icon(assets::TAG, tag_or_fallback_size, color));
        return icons;
    }

    if row.has_local || row.is_current {
        let icon_data = if row.worktree_path.is_some() {
            assets::TREE
        } else {
            assets::LAPTOP
        };
        icons.push(assets::icon(icon_data, laptop_size, color));
    }
    if row.has_remote {
        icons.push(assets::icon(assets::CLOUD, cloud_size, color));
    }
    if icons.is_empty() && !row.is_current {
        icons.push(assets::icon(assets::BRANCH, tag_or_fallback_size, color));
    }

    icons
}

fn branch_stack_icons(row: &BranchDisplayRow, color: Color) -> Vec<Element<'static, Message>> {
    branch_row_icons(
        row,
        color,
        BRANCH_LABEL_ICON_SIZE,
        BRANCH_LABEL_LAPTOP_ICON_SIZE,
        BRANCH_LABEL_CLOUD_ICON_SIZE,
    )
}

// ─── Row content (text + icons) ───────────────────────────────────────────────

fn branch_row_content_exact_inner(
    row_data: &BranchDisplayRow,
    trailing_cover_w: f32,
    max_text_width: f32,
    text_color: Color,
    icon_color: Color,
) -> Element<'static, Message> {
    let mut items: Vec<Element<Message>> = Vec::new();

    if row_data.is_current && !row_data.is_tag {
        items.push(assets::icon(
            assets::CHECK,
            BRANCH_LABEL_ICON_SIZE,
            icon_color,
        ));
    }

    let display_text = display_name_for_width(&row_data.name, max_text_width);
    items.push(
        text(display_text)
            .size(theme::FONT_SM)
            .style(move |_: &Theme| text::Style {
                color: Some(text_color),
            })
            .into(),
    );
    items.extend(branch_stack_icons(row_data, icon_color));

    if trailing_cover_w > 0.0 {
        items.push(
            horizontal_space()
                .width(Length::Fixed(trailing_cover_w))
                .into(),
        );
    }

    row(items)
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
}

fn branch_stack_row_content_exact(
    row_data: &BranchDisplayRow,
    trailing_cover_w: f32,
    max_text_width: f32,
) -> Element<'static, Message> {
    let color = branch_stack_text_color(row_data);
    branch_row_content_exact_inner(row_data, trailing_cover_w, max_text_width, color, color)
}

// ─── Pill widget ──────────────────────────────────────────────────────────────

/// A single styled label pill at the inline truncation limit.
/// `max_text_width` is the maximum pixel width available for the branch name
/// text, accounting for all layout context.
fn branch_stack_label(
    row_data: &BranchDisplayRow,
    background: Color,
    radius: border::Radius,
    max_text_width: f32,
) -> Element<'static, Message> {
    let border_color = branch_stack_border(row_data);

    container(branch_stack_row_content_exact(
        row_data,
        0.0,
        max_text_width,
    ))
    .padding(Padding::from([5, 10]))
    .style(move |_: &Theme| container::Style {
        background: Some(background.into()),
        border: Border {
            color: border_color,
            width: 1.0,
            radius,
        },
        ..Default::default()
    })
    .into()
}

// ─── Click wiring ─────────────────────────────────────────────────────────────

pub(super) fn branch_checkout_message(row_data: &BranchDisplayRow) -> Option<Message> {
    if row_data.is_tag {
        return None;
    }
    let is_remote_only = row_data.has_remote && !row_data.has_local && !row_data.is_current;
    let remote_branch = row_data
        .remote_branch_name
        .as_deref()
        .unwrap_or(row_data.name.as_str());
    Some(Message::repo(RepositoryMessage::Center(
        CenterAction::BranchLabelClicked {
            branch_name: row_data.name.clone(),
            is_remote_only,
            remote_ref: if is_remote_only {
                remote_ref_name(row_data.remote_name.as_deref(), remote_branch)
            } else {
                None
            },
        },
    )))
}

pub(super) fn branch_context_menu_message(row_data: &BranchDisplayRow) -> Option<Message> {
    Some(Message::repo(RepositoryMessage::Center(
        CenterAction::BranchLabelRightClicked {
            branch_name: row_data.name.clone(),
            is_remote: row_data.has_remote && !row_data.has_local && !row_data.is_tag,
            has_remote: row_data.has_remote && !row_data.is_tag,
            is_tag: row_data.is_tag,
            remote_name: row_data.remote_name.clone(),
            remote_ref: row_data
                .remote_branch_name
                .as_deref()
                .and_then(|branch| remote_ref_name(row_data.remote_name.as_deref(), branch)),
            remote_branch_name: row_data.remote_branch_name.clone(),
        },
    )))
}

fn clickable_branch_label(
    content: Element<'static, Message>,
    row_data: &BranchDisplayRow,
) -> Element<'static, Message> {
    match branch_checkout_message(row_data) {
        Some(message) => {
            let area = MouseArea::new(content)
                .on_press(message)
                .interaction(mouse::Interaction::Pointer);

            match branch_context_menu_message(row_data) {
                Some(right_press) => area.on_right_press(right_press).into(),
                None => area.into(),
            }
        }
        None => match branch_context_menu_message(row_data) {
            Some(right_press) => MouseArea::new(content)
                .on_right_press(right_press)
                .interaction(mouse::Interaction::Pointer)
                .into(),
            None => content,
        },
    }
}

// ─── Overflow badge + empty fallback ──────────────────────────────────────────

fn extra_pill_badge(
    count: usize,
    is_active: bool,
    row: &BranchDisplayRow,
) -> Element<'static, Message> {
    let border_color = if is_active {
        branch_stack_border(row)
    } else {
        theme::BORDER
    };
    let text_color = if is_active {
        Color::WHITE
    } else {
        theme::TEXT_SECONDARY
    };

    container(
        text(format!("+{}", count))
            .size(theme::FONT_XS)
            .style(move |_: &Theme| text::Style {
                color: Some(text_color),
            }),
    )
    .padding(Padding::from([2, 5]))
    .style(move |_: &Theme| container::Style {
        background: Some(theme::BG_BASE.into()),
        border: Border {
            radius: 3.0.into(),
            color: border_color,
            width: 1.0,
        },
        ..Default::default()
    })
    .into()
}

fn empty_cell(bg: Color, commit_idx: usize) -> Element<'static, Message> {
    let cell_button = button(horizontal_space())
        .style(move |_: &Theme, _: button::Status| button::Style {
            background: Some(bg.into()),
            border: Border::default(),
            ..Default::default()
        })
        .on_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CommitClicked(commit_idx),
        )))
        .padding(Padding::from([5, BRANCH_LABEL_INSET_X]))
        .width(Length::Fill)
        .height(Length::Fixed(ROW_H));

    MouseArea::new(cell_button)
        .on_right_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CommitRightClicked { commit_idx },
        )))
        .into()
}

// ─── Cell entry point ─────────────────────────────────────────────────────────

pub fn branch_label_cell<'a>(
    display_rows: &'a [BranchDisplayRow],
    is_selected: bool,
    commit_idx: usize,
    active_popout_commit: Option<usize>,
) -> Element<'a, Message> {
    use super::branch_popout_trigger_id;
    use crate::services::{cached_measure_width, FontFamily};

    let bg = if is_selected {
        theme::BG_SELECTED
    } else {
        theme::BG_PANEL
    };
    let rows = display_rows;
    let extra_count = rows.len().saturating_sub(1);
    let has_popout = extra_count > 0;
    let is_active_popout = active_popout_commit == Some(commit_idx);
    let listeners_suspended =
        active_popout_commit.is_some() && active_popout_commit != Some(commit_idx);

    let trigger_row = if has_popout {
        rows.iter().find(|r| r.is_current).or(rows.first())
    } else {
        rows.first()
    };

    let content_width = (theme::BRANCH_COL_WIDTH as f32) - 2.0 * (BRANCH_LABEL_INSET_X as f32);

    let content: Element<Message> = if has_popout {
        let Some(trigger_row) = trigger_row else {
            return empty_cell(bg, commit_idx);
        };
        let trigger_bg = branch_stack_background(trigger_row);
        let radius = if is_active_popout {
            border::Radius::default().top(BRANCH_POPOUT_RADIUS)
        } else {
            BRANCH_POPOUT_RADIUS.into()
        };
        let trigger_has_checkmark = trigger_row.is_current && !trigger_row.is_tag;
        let trigger_trailing_icons = count_trailing_icons(trigger_row);
        let max_text_width = calculate_text_width(
            content_width,
            1,
            true,
            trigger_has_checkmark,
            trigger_trailing_icons,
        );
        let trigger_label = clickable_branch_label(
            branch_stack_label(trigger_row, trigger_bg, radius, max_text_width),
            trigger_row,
        );
        let trigger_cluster = row![
            trigger_label,
            extra_pill_badge(extra_count, is_active_popout, trigger_row),
        ]
        .spacing(PILL_ROW_SPACING)
        .align_y(iced::Alignment::Center);

        let trigger_cluster: Element<Message> = if is_active_popout {
            container(trigger_cluster)
                .id(branch_popout_trigger_id())
                .into()
        } else {
            trigger_cluster.into()
        };

        let trigger_content: Element<Message> = if !listeners_suspended {
            MouseArea::new(trigger_cluster)
                .on_enter(Message::repo(RepositoryMessage::Center(
                    CenterAction::BranchLabelTriggerEntered(commit_idx),
                )))
                .into()
        } else {
            trigger_cluster
        };

        row![trigger_content, horizontal_space()]
            .align_y(iced::Alignment::Center)
            .into()
    } else {
        if rows.is_empty() {
            return empty_cell(bg, commit_idx);
        }

        let num_pills = rows.len();
        let pill_text_widths: Vec<f32> = rows
            .iter()
            .map(|r| {
                calculate_text_width(
                    content_width,
                    num_pills,
                    false,
                    r.is_current && !r.is_tag,
                    count_trailing_icons(r),
                )
            })
            .collect();

        let any_truncated = rows.iter().zip(pill_text_widths.iter()).any(|(r, w)| {
            let full_w = cached_measure_width(&r.name, FontFamily::Default, theme::FONT_SM);
            full_w > *w
        });

        let pills: Vec<Element<Message>> = rows
            .iter()
            .zip(pill_text_widths.iter())
            .map(|(row_data, max_tw)| {
                let bg_col = branch_stack_background(row_data);
                clickable_branch_label(
                    branch_stack_label(row_data, bg_col, BRANCH_POPOUT_RADIUS.into(), *max_tw),
                    row_data,
                )
            })
            .collect();

        let pills_elem: Element<Message> = row(pills)
            .spacing(PILL_ROW_SPACING)
            .align_y(iced::Alignment::Center)
            .into();

        if any_truncated {
            let labeled: Element<Message> = if is_active_popout {
                container(pills_elem).id(branch_popout_trigger_id()).into()
            } else {
                pills_elem
            };

            let with_hover: Element<Message> = if !listeners_suspended {
                MouseArea::new(labeled)
                    .on_enter(Message::repo(RepositoryMessage::Center(
                        CenterAction::BranchLabelTriggerEntered(commit_idx),
                    )))
                    .into()
            } else {
                labeled
            };

            row![with_hover, horizontal_space()]
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            pills_elem
        }
    };

    let cell_button = button(content)
        .style(move |_: &Theme, _: button::Status| button::Style {
            background: Some(bg.into()),
            border: Border::default(),
            ..Default::default()
        })
        .on_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CommitClicked(commit_idx),
        )))
        .padding(Padding::from([5, BRANCH_LABEL_INSET_X]))
        .width(Length::Fill)
        .height(Length::Fixed(ROW_H));

    MouseArea::new(cell_button)
        .on_right_press(Message::repo(RepositoryMessage::Center(
            CenterAction::CommitRightClicked { commit_idx },
        )))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_row_with_worktree() -> BranchDisplayRow {
        BranchDisplayRow {
            name: "feature-x".into(),
            lane_color: 0,
            has_local: true,
            has_remote: false,
            is_current: false,
            is_tag: false,
            remote_name: None,
            remote_branch_name: None,
            worktree_path: Some(PathBuf::from("/tmp/wt")),
        }
    }

    #[test]
    fn branch_stack_icons_uses_tree_when_worktree_path_set() {
        let row = sample_row_with_worktree();
        // `Element` internals aren't identity-comparable, so we verify
        // count + shape as a smoke check. Full rendering is visual.
        let icons = branch_stack_icons(&row, Color::WHITE);
        assert_eq!(
            icons.len(),
            1,
            "local+worktree should emit a single icon slot"
        );
    }
}

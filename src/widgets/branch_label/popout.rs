//! Floating popout panel listing every `BranchDisplayRow` for a commit.
//! Thin wrapper over `widgets::list::HoverableList`.
use iced::{
    border,
    widget::{row, text},
    Color, Element, Length, Theme,
};

use crate::assets;
use crate::message::Message;
use crate::theme;
use crate::view_model::BranchDisplayRow;
use crate::widgets::list::{HoverableList, HoverableListItem, HoverableListStyle};
use crate::widgets::palette;
use crate::widgets::shared::horizontal_space;

use super::cell::{branch_checkout_message, branch_context_menu_message};
use super::layout::{
    display_name, BRANCH_POPOUT_ICON_SIZE, BRANCH_POPOUT_ROW_PADDING_X,
    BRANCH_STACK_BADGE_COVER_W,
};
use super::BRANCH_POPOUT_RADIUS;

/// Height of popout rows — matches inline label pill height (5px + 11px + 5px + 1px + 1px).
const BRANCH_POPOUT_ROW_H: f32 = 24.0;

// ─── Row colors ───────────────────────────────────────────────────────────────

fn branch_popout_text_color(row: &BranchDisplayRow) -> Color {
    palette::tint(palette::lane_color(row.lane_color), 0.4, 0.6)
}

fn branch_popout_base_background(row: &BranchDisplayRow) -> Color {
    if row.is_tag {
        super::cell::branch_lane_background(row.lane_color)
    } else {
        super::cell::branch_stack_background(row)
    }
}

fn inactive_popout_row_background(background: Color) -> Color {
    Color {
        r: (background.r * 0.86 + theme::BG_PANEL.r * 0.14) * 0.75,
        g: (background.g * 0.86 + theme::BG_PANEL.g * 0.14) * 0.75,
        b: (background.b * 0.86 + theme::BG_PANEL.b * 0.14) * 0.75,
        a: 1.0,
    }
}

fn branch_popout_row_radius(index: usize, row_count: usize) -> border::Radius {
    if row_count <= 1 {
        BRANCH_POPOUT_RADIUS.into()
    } else if index == 0 {
        border::Radius::default().top(BRANCH_POPOUT_RADIUS)
    } else if index + 1 == row_count {
        border::Radius::default().bottom(BRANCH_POPOUT_RADIUS)
    } else {
        border::Radius::default()
    }
}

// ─── Row content ──────────────────────────────────────────────────────────────

fn branch_popout_icons(row: &BranchDisplayRow, color: Color) -> Vec<Element<'static, Message>> {
    let mut icons = Vec::new();

    if row.is_tag {
        icons.push(assets::icon(assets::TAG, BRANCH_POPOUT_ICON_SIZE, color));
        return icons;
    }

    if row.has_local || row.is_current {
        icons.push(assets::icon(assets::LAPTOP, BRANCH_POPOUT_ICON_SIZE, color));
    }
    if row.has_remote {
        icons.push(assets::icon(assets::CLOUD, BRANCH_POPOUT_ICON_SIZE, color));
    }
    if icons.is_empty() && !row.is_current {
        icons.push(assets::icon(assets::BRANCH, BRANCH_POPOUT_ICON_SIZE, color));
    }

    icons
}

fn branch_popout_row_content(
    row_data: &BranchDisplayRow,
    trailing_cover_w: f32,
    max_name_len: usize,
) -> Element<'static, Message> {
    let color = branch_popout_text_color(row_data);
    let mut items: Vec<Element<Message>> = Vec::new();

    if row_data.is_current && !row_data.is_tag {
        items.push(assets::icon(assets::CHECK, BRANCH_POPOUT_ICON_SIZE, color));
    }

    items.push(
        text(display_name(&row_data.name, max_name_len))
            .size(theme::FONT_SM)
            .style(move |_: &Theme| text::Style { color: Some(color) })
            .into(),
    );
    items.extend(branch_popout_icons(row_data, color));

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

// ─── The popout panel ─────────────────────────────────────────────────────────

/// A floating panel containing one row per `BranchDisplayRow`.
/// `max_name_len` controls how many characters are shown before truncation;
/// pass `usize::MAX` to always show the full name.
pub fn branch_popout_panel(
    rows: &[BranchDisplayRow],
    _background: Color,
    max_name_len: usize,
) -> Element<'static, Message> {
    let row_count = rows.len();
    let border_color = rows
        .first()
        .map(super::cell::branch_stack_border)
        .unwrap_or(theme::BORDER);

    let items: Vec<HoverableListItem<Message>> = rows
        .iter()
        .enumerate()
        .map(|(idx, row_data)| {
            let content = branch_popout_row_content(
                row_data,
                if idx == 0 && row_count > 1 {
                    BRANCH_STACK_BADGE_COVER_W
                } else {
                    0.0
                },
                max_name_len,
            );
            let base_bg = branch_popout_base_background(row_data);
            // Current branch always paints at full intensity (matches the pill
            // it visually replaces); everything else dims until hover.
            let idle_bg = if row_data.is_current {
                base_bg
            } else {
                inactive_popout_row_background(base_bg)
            };
            HoverableListItem {
                content,
                on_press: branch_checkout_message(row_data),
                on_right_press: branch_context_menu_message(row_data),
                on_hover_enter: None,
                on_hover_exit: None,
                row_bg_idle: Some(idle_bg),
                row_bg_hover: Some(base_bg),
                row_border_radius: Some(branch_popout_row_radius(idx, row_count)),
            }
        })
        .collect();

    let style = HoverableListStyle {
        row_height: BRANCH_POPOUT_ROW_H,
        row_spacing: 0.0,
        outer_padding: 0.0,
        row_padding_x: BRANCH_POPOUT_ROW_PADDING_X,
        border_color,
        border_radius: BRANCH_POPOUT_RADIUS,
        border_width: 1.0,
        background: theme::BG_PANEL,
        shadow: None,
        row_bg_idle: theme::BG_PANEL,
        row_bg_hover: theme::BG_PANEL,
    };

    HoverableList::new(items, style).sticky_active(true).into()
}

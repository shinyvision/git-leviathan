//! Pill sizing + name truncation math for branch labels.
use crate::services::{cached_measure_width, cached_truncate_name, FontFamily};
use crate::theme;
use crate::utils::truncate;
use crate::view_model::BranchDisplayRow;

/// Horizontal padding inside a pill (left + right: [5, 10] = 20px total).
pub(super) const PILL_PADDING_X: f32 = 20.0;
/// Spacing between elements inside a pill row.
pub(super) const PILL_ELEMENT_SPACING: f32 = 6.0;
/// Spacing between pills in the outer row.
pub(super) const PILL_ROW_SPACING: f32 = 4.0;

pub(super) const BRANCH_STACK_BADGE_COVER_W: f32 = 22.0;
pub(super) const BRANCH_LABEL_ICON_SIZE: f32 = 12.0;
pub(super) const BRANCH_LABEL_LAPTOP_ICON_SIZE: f32 = 12.0;
pub(super) const BRANCH_LABEL_CLOUD_ICON_SIZE: f32 = 12.0;
pub(super) const BRANCH_POPOUT_ICON_SIZE: f32 = 12.0;
pub(super) const BRANCH_POPOUT_ROW_PADDING_X: f32 = 10.0;

/// Width of one icon inside a pill (icon size + spacing to next element).
pub(super) fn icon_slot_width() -> f32 {
    BRANCH_LABEL_ICON_SIZE + PILL_ELEMENT_SPACING
}

/// Calculates the maximum text width for a branch pill's name, accounting for
/// the full layout context: cell padding, sibling pills, overflow badge, and
/// icons within the pill.
///
/// `content_width` — total horizontal space available for all pills + badge
///   (typically BRANCH_COL_WIDTH - 2 × BRANCH_LABEL_INSET_X).
/// `num_pills` — how many branch pills are in the row.
/// `has_overflow_badge` — whether an overflow "[+N]" badge follows the pills.
/// `has_checkmark` — whether this pill shows a checkmark icon.
/// `num_trailing_icons` — number of icon slots (laptop/cloud/branch/tag).
pub(super) fn calculate_text_width(
    content_width: f32,
    num_pills: usize,
    has_overflow_badge: bool,
    has_checkmark: bool,
    num_trailing_icons: usize,
) -> f32 {
    let total_row_spacing = if num_pills > 1 {
        (num_pills - 1) as f32 * PILL_ROW_SPACING
    } else {
        0.0
    };

    let badge_space = if has_overflow_badge {
        PILL_ROW_SPACING + overflow_badge_width()
    } else {
        0.0
    };

    let total_pill_space = content_width - total_row_spacing - badge_space;
    let pill_budget = (total_pill_space / num_pills as f32).max(0.0);

    let icons_width: f32 = if has_checkmark {
        icon_slot_width()
    } else {
        0.0
    } + num_trailing_icons as f32 * icon_slot_width();

    (pill_budget - PILL_PADDING_X - icons_width).max(0.0)
}

/// Returns the pixel width of the overflow badge ("+N" pill).
pub(super) fn overflow_badge_width() -> f32 {
    let text_w = cached_measure_width("+99", FontFamily::Default, theme::FONT_XS);
    text_w + 2.0 * 5.0 + 2.0 * 1.0
}

/// Number of trailing icons (laptop/cloud/branch) for a branch row.
pub(super) fn count_trailing_icons(row: &BranchDisplayRow) -> usize {
    if row.is_tag {
        return 1;
    }

    let mut count = 0;
    if row.has_local || row.is_current {
        count += 1;
    }
    if row.has_remote {
        count += 1;
    }
    if count == 0 && !row.is_current {
        count = 1;
    }
    count
}

/// Returns the display string, clipped to fit within `max_width` pixels.
/// Uses the global text measurement cache to avoid repeated font-lock acquisitions.
pub(super) fn display_name_for_width(name: &str, max_width: f32) -> String {
    cached_truncate_name(name, max_width)
}

/// Returns the display string, clipped to `max` chars with a `…` suffix if needed.
pub(super) fn display_name(name: &str, max: usize) -> String {
    if is_truncated_at(name, max) {
        if max == 0 {
            String::new()
        } else {
            format!("{}…", truncate(name, max.saturating_sub(1)))
        }
    } else {
        name.to_string()
    }
}

fn is_truncated_at(name: &str, max: usize) -> bool {
    name.chars().nth(max).is_some()
}

/// True when the name exceeds its available pixel width in the branch column,
/// accounting for icons, sibling pills, and overflow badges.
pub fn name_exceeds_available_width(
    name: &str,
    num_pills: usize,
    has_overflow_badge: bool,
    has_checkmark: bool,
    num_trailing_icons: usize,
) -> bool {
    let content_width =
        (theme::BRANCH_COL_WIDTH as f32) - 2.0 * (super::BRANCH_LABEL_INSET_X as f32);
    let max_tw = calculate_text_width(
        content_width,
        num_pills,
        has_overflow_badge,
        has_checkmark,
        num_trailing_icons,
    );
    let actual_w = cached_measure_width(name, FontFamily::Default, theme::FONT_SM);
    actual_w > max_tw
}

/// True when any display row's name doesn't fit at its allocated inline width.
pub fn any_name_truncated(rows: &[BranchDisplayRow]) -> bool {
    let num_pills = rows.len();
    let has_overflow = num_pills > 1;
    rows.iter().any(|r| {
        let has_checkmark = r.is_current && !r.is_tag;
        let trailing = count_trailing_icons(r);
        if has_overflow {
            let idx = rows.iter().position(|x| x.is_current).unwrap_or(0);
            let trigger = &rows[idx];
            name_exceeds_available_width(
                &r.name,
                1,
                true,
                trigger.is_current && !trigger.is_tag,
                count_trailing_icons(trigger),
            )
        } else {
            name_exceeds_available_width(&r.name, num_pills, false, has_checkmark, trailing)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{any_name_truncated, display_name};
    use crate::view_model::{BranchLabel, BranchLabelKind};

    fn label(name: &str, kind: BranchLabelKind) -> BranchLabel {
        BranchLabel {
            name: name.to_string(),
            kind,
            lane_color: 0,
            remote_name: None,
            upstream_ref: None,
        }
    }

    #[test]
    fn display_name_truncates_unicode_without_panicking() {
        assert_eq!(display_name("feature/mañana", 10), "feature/m…");
    }

    #[test]
    fn any_name_truncated_detects_long_name() {
        let rows = super::super::branch_display_rows(&[label(
            "über/feature/some-very-long-branch",
            BranchLabelKind::CurrentLocal,
        )]);
        assert!(any_name_truncated(&rows));
    }

    #[test]
    fn any_name_truncated_short_name_fits() {
        let rows =
            super::super::branch_display_rows(&[label("main", BranchLabelKind::CurrentLocal)]);
        assert!(!any_name_truncated(&rows));
    }
}

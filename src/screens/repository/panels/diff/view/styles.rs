//! Style functions for diff/conflict widgets: scrollbars, save button,
//! hunk-pick checkboxes. Kept separate from layout so the view file stays
//! focused on composition.

use iced::{
    widget::{button, checkbox, container, scrollable},
    Border, Color, Theme,
};

use crate::theme;

pub(super) const DIFF_SCROLLBAR_WIDTH: f32 = 5.0;
pub(super) const CONFLICT_SCROLLBAR_WIDTH: f32 = 15.0;

pub(super) fn both_scrollbars() -> scrollable::Direction {
    scrollable::Direction::Both {
        vertical: scrollable::Scrollbar::new()
            .width(CONFLICT_SCROLLBAR_WIDTH)
            .scroller_width(CONFLICT_SCROLLBAR_WIDTH),
        horizontal: scrollable::Scrollbar::new()
            .width(CONFLICT_SCROLLBAR_WIDTH)
            .scroller_width(CONFLICT_SCROLLBAR_WIDTH),
    }
}

pub(super) fn diff_scrollbar_style(
    _theme: &Theme,
    _status: scrollable::Status,
) -> scrollable::Style {
    let rail = scrollable::Rail {
        background: Some(theme::BG_SIDEBAR.into()),
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: theme::BORDER.into(),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Color::TRANSPARENT.into(),
            border: Border::default(),
            shadow: iced::Shadow::default(),
            icon: Color::TRANSPARENT,
        },
    }
}

pub(super) fn conflict_scrollbar_style(
    _theme: &Theme,
    _status: scrollable::Status,
) -> scrollable::Style {
    let rail = scrollable::Rail {
        background: Some(theme::BG_SIDEBAR.into()),
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: theme::BORDER.into(),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
        },
    };

    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: Some(theme::BG_SIDEBAR.into()),
        auto_scroll: scrollable::AutoScroll {
            background: Color::TRANSPARENT.into(),
            border: Border::default(),
            shadow: iced::Shadow::default(),
            icon: Color::TRANSPARENT,
        },
    }
}

pub(super) fn save_button_style(_: &Theme, status: button::Status) -> button::Style {
    let background = if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        Color {
            r: 0.361,
            g: 0.722,
            b: 0.361,
            a: 1.0,
        }
    } else {
        Color {
            r: 0.247,
            g: 0.369,
            b: 0.288,
            a: 1.0,
        }
    };
    let border = Color {
        r: 0.361,
        g: 0.722,
        b: 0.361,
        a: 1.0,
    };
    button::Style {
        background: Some(background.into()),
        text_color: Color::WHITE,
        border: Border {
            color: border,
            width: 1.0,
            radius: 3.0.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}

pub(super) fn conflict_checkbox_style(_: &Theme, status: checkbox::Status) -> checkbox::Style {
    let is_checked = match status {
        checkbox::Status::Active { is_checked }
        | checkbox::Status::Hovered { is_checked }
        | checkbox::Status::Disabled { is_checked } => is_checked,
    };
    let border_color = if is_checked {
        theme::ACCENT_GREEN
    } else {
        theme::TEXT_DIM
    };
    let background = if is_checked {
        theme::BG_SELECTED
    } else {
        theme::BG_BASE
    };

    checkbox::Style {
        background: background.into(),
        icon_color: Color::WHITE,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 3.0.into(),
        },
        text_color: Some(theme::TEXT_PRIMARY),
    }
}

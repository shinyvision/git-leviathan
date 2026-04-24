use iced::{
    widget::{container, text},
    Border, Theme,
};

use crate::theme;

pub fn dim_text(_: &Theme) -> text::Style {
    text::Style {
        color: Some(theme::TEXT_DIM),
    }
}

pub fn secondary_text(_: &Theme) -> text::Style {
    text::Style {
        color: Some(theme::TEXT_SECONDARY),
    }
}

pub fn path_text(_: &Theme) -> text::Style {
    text::Style {
        color: Some(theme::TEXT_PATH),
    }
}

pub fn primary_text(_: &Theme) -> text::Style {
    text::Style {
        color: Some(theme::TEXT_PRIMARY),
    }
}

pub fn white_text(_: &Theme) -> text::Style {
    text::Style {
        color: Some(iced::Color::WHITE),
    }
}

pub fn header_container(_: &Theme) -> container::Style {
    container::Style {
        background: Some(theme::BG_HEADER.into()),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub fn toolbar_container(_: &Theme) -> container::Style {
    container::Style {
        background: Some(theme::BG_TOOLBAR.into()),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub fn panel_container(_: &Theme) -> container::Style {
    container::Style {
        background: Some(theme::BG_PANEL.into()),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub fn sidebar_container(_: &Theme) -> container::Style {
    container::Style {
        background: Some(theme::BG_SIDEBAR.into()),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}


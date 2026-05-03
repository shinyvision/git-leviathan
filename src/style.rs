use iced::{
    widget::{container, text},
    Border, Color, Theme,
};

use crate::theme;

pub fn iced_theme() -> iced::Theme {
    iced::Theme::custom(
        "leviathan".to_string(),
        iced::theme::Palette {
            background: theme::BG_BASE,
            text: theme::TEXT_PRIMARY,
            primary: theme::ACCENT_BLUE,
            success: theme::ACCENT_GREEN,
            danger: Color {
                r: 0.858,
                g: 0.243,
                b: 0.243,
                a: 1.0,
            },
            warning: theme::ACCENT_ORANGE,
        },
    )
}

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

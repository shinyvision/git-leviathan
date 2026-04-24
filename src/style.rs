use iced::{
    widget::{button, container, text},
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

pub fn tab_button(is_active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_: &Theme, status: button::Status| {
        let bg = if is_active {
            theme::BG_HOVER
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => theme::BG_HOVER,
                _ => theme::BG_BASE,
            }
        };
        let tc = if is_active {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_SECONDARY
        };
        let border_color = theme::BORDER;
        button::Style {
            background: Some(bg.into()),
            text_color: tc,
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 0.0.into(),
            },
            shadow: Default::default(),
            snap: false,
        }
    }
}

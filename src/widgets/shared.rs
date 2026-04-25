use iced::{
    widget::{container, scrollable, Space},
    Border, Color, Element, Length, Theme,
};

use crate::message::Message;
use crate::theme;

pub fn horizontal_space() -> Space {
    Space::new().width(Length::Fill)
}

pub fn h_divider<'a>() -> Element<'a, Message> {
    container(horizontal_space())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_: &Theme| container::Style {
            background: Some(theme::DIVIDER.into()),
            ..Default::default()
        })
        .into()
}

pub fn v_divider<'a>() -> Element<'a, Message> {
    container(Space::new().height(Length::Fill))
        .width(Length::Fixed(1.0))
        .height(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(theme::DIVIDER.into()),
            ..Default::default()
        })
        .into()
}

pub fn list_vertical_spacer(height: f32) -> Element<'static, Message> {
    container(Space::new().width(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(height.max(0.0)))
        .into()
}

pub fn scrollbar_style(_theme: &Theme, _status: scrollable::Status) -> scrollable::Style {
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: scrollable::Rail {
            background: Some(theme::BG_SIDEBAR.into()),
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: theme::BORDER.into(),
                border: Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
            },
        },
        horizontal_rail: scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: Color::TRANSPARENT.into(),
                border: Border::default(),
            },
        },
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Color::TRANSPARENT.into(),
            border: Border::default(),
            shadow: iced::Shadow::default(),
            icon: Color::TRANSPARENT,
        },
    }
}

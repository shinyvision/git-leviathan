//! Shared button/input helpers used across overlay dialogs.
//!
//! Each dialog builds its row from these primitives so the visual vocabulary
//! stays consistent (green CREATE button, red DANGER button, neutral CANCEL).

use iced::{
    widget::{button, container, row, text, text_input, MouseArea},
    Border, Color, Element, Length, Padding, Theme,
};

use crate::{message::Message, style, theme};

use super::super::RepositoryMessage;

pub const OVERLAY_ENTER_OFFSET: f32 = 2000.0;

#[derive(Clone, Copy)]
pub struct OverlayButtonPalette {
    pub background: Color,
    pub hover_background: Color,
    pub border: Color,
    pub text: Color,
    pub hover_text: Color,
}

pub fn palette_button_style(
    palette: OverlayButtonPalette,
    enabled: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_: &Theme, status: button::Status| {
        let disabled = !enabled || matches!(status, button::Status::Disabled);
        let dim = |c: Color| Color { a: 0.5, ..c };
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = if disabled {
            dim(palette.background)
        } else if hovered {
            palette.hover_background
        } else {
            palette.background
        };
        let border_color = if disabled {
            dim(palette.border)
        } else {
            palette.border
        };
        let text_color = if disabled {
            dim(palette.text)
        } else if hovered {
            palette.hover_text
        } else {
            palette.text
        };
        button::Style {
            background: Some(background.into()),
            text_color,
            border: Border {
                radius: 3.0.into(),
                color: border_color,
                width: 1.0,
            },
            shadow: Default::default(),
            snap: false,
        }
    }
}

pub const CREATE_BUTTON: OverlayButtonPalette = OverlayButtonPalette {
    background: Color {
        r: 0.189,
        g: 0.275,
        b: 0.231,
        a: 1.0,
    },
    hover_background: Color {
        r: 0.267,
        g: 0.533,
        b: 0.278,
        a: 1.0,
    },
    border: Color {
        r: 0.267,
        g: 0.533,
        b: 0.278,
        a: 1.0,
    },
    text: theme::TEXT_PRIMARY,
    hover_text: Color::WHITE,
};

/// Browse buttons (folder picker). Desaturated base hovers to ACCENT_BLUE,
/// same shape as CREATE_BUTTON (green) / DANGER_BUTTON (red).
pub const BROWSE_BUTTON: OverlayButtonPalette = OverlayButtonPalette {
    background: Color {
        r: 0.170,
        g: 0.240,
        b: 0.345,
        a: 1.0,
    },
    hover_background: Color {
        r: 0.259,
        g: 0.518,
        b: 0.929,
        a: 1.0,
    },
    border: Color {
        r: 0.267,
        g: 0.447,
        b: 0.722,
        a: 1.0,
    },
    text: theme::TEXT_PRIMARY,
    hover_text: Color::WHITE,
};

pub const DANGER_BUTTON: OverlayButtonPalette = OverlayButtonPalette {
    background: Color {
        r: 0.627,
        g: 0.196,
        b: 0.196,
        a: 1.0,
    },
    hover_background: Color {
        r: 0.851,
        g: 0.255,
        b: 0.239,
        a: 1.0,
    },
    border: Color {
        r: 0.851,
        g: 0.255,
        b: 0.239,
        a: 1.0,
    },
    text: theme::TEXT_PRIMARY,
    hover_text: Color::WHITE,
};

const CANCEL_BORDER: Color = Color {
    r: 0.804,
    g: 0.820,
    b: 0.847,
    a: 1.0,
};

pub fn overlay_button(
    label: &'static str,
    palette: OverlayButtonPalette,
    on_press: RepositoryMessage,
) -> Element<'static, Message> {
    button(text(label).size(theme::FONT_SM))
        .on_press(Message::repo(on_press))
        .padding(Padding::from([4, 10]))
        .style(move |_: &Theme, status: button::Status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(if hovered {
                    palette.hover_background.into()
                } else {
                    palette.background.into()
                }),
                border: Border {
                    color: palette.border,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                text_color: if hovered {
                    palette.hover_text
                } else {
                    palette.text
                },
                shadow: Default::default(),
                snap: false,
            }
        })
        .into()
}

pub fn overlay_button_disabled(
    label: &'static str,
    palette: OverlayButtonPalette,
) -> Element<'static, Message> {
    let dimmed_bg = Color {
        a: 0.5,
        ..palette.background
    };
    let dimmed_border = Color {
        a: 0.5,
        ..palette.border
    };
    let dimmed_text = Color {
        a: 0.5,
        ..palette.text
    };

    MouseArea::new(
        button(
            text(label)
                .size(theme::FONT_SM)
                .style(move |_: &Theme| text::Style {
                    color: Some(dimmed_text),
                }),
        )
        .padding(Padding::from([4, 10]))
        .style(move |_: &Theme, _: button::Status| button::Style {
            background: Some(dimmed_bg.into()),
            border: Border {
                color: dimmed_border,
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: dimmed_text,
            shadow: Default::default(),
            snap: false,
        }),
    )
    .interaction(iced::mouse::Interaction::NotAllowed)
    .into()
}

pub fn overlay_neutral_button(
    label: &'static str,
    on_press: RepositoryMessage,
) -> Element<'static, Message> {
    button(text(label).size(theme::FONT_SM).style(style::primary_text))
        .on_press(Message::repo(on_press))
        .padding(Padding::from([4, 10]))
        .style(|_: &Theme, status: button::Status| {
            let (border_color, text_color) = match status {
                button::Status::Hovered | button::Status::Pressed => (Color::WHITE, Color::WHITE),
                _ => (CANCEL_BORDER, theme::TEXT_PRIMARY),
            };
            button::Style {
                background: Some(theme::BG_HEADER.into()),
                border: Border {
                    color: border_color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                text_color,
                shadow: Default::default(),
                snap: false,
            }
        })
        .into()
}

pub fn overlay_cancel_button(on_press: RepositoryMessage) -> Element<'static, Message> {
    button(
        text("Cancel")
            .size(theme::FONT_SM)
            .style(style::primary_text),
    )
    .on_press(Message::repo(on_press))
    .padding(Padding::from([4, 10]))
    .style(|_: &Theme, status: button::Status| {
        let (border_color, text_color) = match status {
            button::Status::Hovered | button::Status::Pressed => (Color::WHITE, Color::WHITE),
            _ => (CANCEL_BORDER, theme::TEXT_PRIMARY),
        };
        button::Style {
            background: Some(theme::BG_HEADER.into()),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color,
            shadow: Default::default(),
            snap: false,
        }
    })
    .into()
}

pub fn overlay_text_input_style(_: &Theme, _: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: theme::BG_BASE.into(),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 3.0.into(),
        },
        icon: theme::TEXT_DIM,
        placeholder: theme::TEXT_DIM,
        value: theme::TEXT_PRIMARY,
        selection: theme::ACCENT_BLUE,
    }
}

pub fn overlay_text_input<'a>(
    placeholder: &'static str,
    value: &'a str,
    on_input: impl Fn(String) -> RepositoryMessage + 'static,
) -> Element<'a, Message> {
    text_input(placeholder, value)
        .on_input(move |s| Message::repo(on_input(s)))
        .size(theme::FONT_SM)
        .padding(theme::INPUT_PADDING)
        .width(Length::Fixed(160.0))
        .style(overlay_text_input_style)
        .into()
}

pub fn overlay_text_input_with_submit<'a>(
    placeholder: &'static str,
    value: &'a str,
    on_input: impl Fn(String) -> RepositoryMessage + 'static,
    on_submit: Message,
    id: Option<iced::widget::Id>,
) -> Element<'a, Message> {
    let mut input = text_input(placeholder, value)
        .on_input(move |s| Message::repo(on_input(s)))
        .on_submit(on_submit)
        .size(theme::FONT_SM)
        .padding(theme::INPUT_PADDING)
        .width(Length::Fixed(160.0))
        .style(overlay_text_input_style);

    if let Some(id) = id {
        input = input.id(id);
    }

    input.into()
}

pub fn sliding_main_bar_overlay<'a>(
    content: Element<'a, Message>,
    slide_offset: f32,
) -> Element<'a, Message> {
    use crate::widgets::SlideOverlay;

    let panel = MouseArea::new(
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(iced::alignment::Vertical::Center)
            .style(style::header_container),
    );

    SlideOverlay::new(panel, -slide_offset.max(0.0), 0.0, 0.0, Length::Fill, 0.0).into()
}

pub fn overlay_row(items: Vec<Element<Message>>) -> Element<Message> {
    use crate::widgets::shared::horizontal_space as hspace;

    let mut all = vec![hspace().into()];
    all.extend(items);
    all.push(hspace().into());

    row(all)
        .spacing(10)
        .align_y(iced::Alignment::Center)
        .padding(Padding::from([0, 16]))
        .into()
}

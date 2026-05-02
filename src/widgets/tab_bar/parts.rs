//! Shared per-tab decorations used by both the native built-in tab list
//! and the Lua-facing `tablist` widget. Keeping them here means the two
//! consumers render identical leading folder icons and trailing
//! hover-swap close buttons without copy-pasting styles.

use iced::{
    widget::{button, container},
    Border, Element, Padding, Theme,
};

use crate::assets;
use crate::message::Message;
use crate::theme;
use crate::widgets::primitives::hoverable::hoverable_swap;

pub fn folder_icon<'a>(is_active: bool) -> Element<'a, Message> {
    let color = if is_active {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_DIM
    };
    container(assets::tab_icon(assets::FOLDER, color))
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

/// Close button that publishes `on_close` when clicked. The idle/hover
/// states swap the icon's text color via the hoverable primitive so the
/// "x" only fully appears under the cursor.
pub fn close_button<'a>(on_close: Message) -> Element<'a, Message> {
    let close_idle: Element<'a, Message> = close_inner(theme::TEXT_DIM, None);
    let close_hover: Element<'a, Message> = close_inner(theme::TEXT_PRIMARY, Some(on_close));
    hoverable_swap(
        close_idle,
        close_hover,
        |_| iced::widget::container::Style::default(),
        |_| iced::widget::container::Style::default(),
    )
}

fn close_inner<'a>(text_color: iced::Color, on_press: Option<Message>) -> Element<'a, Message> {
    let mut btn = button(assets::tab_icon(assets::CLOSE, text_color))
        .style(move |_: &Theme, _: button::Status| button::Style {
            background: None,
            text_color,
            border: Border::default(),
            shadow: Default::default(),
            snap: false,
        })
        .padding(Padding::from([4u16, 4]));
    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }
    btn.into()
}

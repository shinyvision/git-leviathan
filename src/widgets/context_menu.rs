//! Context menu — thin wrapper over `widgets::list::HoverableList` with
//! context-menu styling (bordered panel, drop shadow, BG_SELECTED hover row).
use iced::{widget::text, Color, Element, Shadow, Theme, Vector};

use crate::{
    message::Message,
    theme,
    widgets::list::{HoverableList, HoverableListItem, HoverableListStyle},
};

const ROW_H: f32 = 19.0;
const PADDING: f32 = 4.0;
const BUTTON_PADDING_X: f32 = 10.0;
const SPACING: f32 = 2.0;

pub struct ContextMenuItem {
    pub on_press: Option<Message>,
    pub on_hover_enter: Option<Message>,
    pub on_hover_exit: Option<Message>,
    pub content: Element<'static, Message>,
}

pub struct ContextMenu {
    pub items: Vec<ContextMenuItem>,
}

impl ContextMenu {
    pub fn new(items: Vec<ContextMenuItem>) -> Self {
        Self { items }
    }
}

fn context_menu_style() -> HoverableListStyle {
    HoverableListStyle {
        row_height: ROW_H,
        row_spacing: SPACING,
        outer_padding: PADDING,
        row_padding_x: BUTTON_PADDING_X,
        border_color: theme::BORDER,
        border_radius: 4.0,
        border_width: 1.0,
        background: theme::BG_PANEL,
        shadow: Some(Shadow {
            color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.2,
            },
            offset: Vector::new(4.0, 4.0),
            blur_radius: 8.0,
        }),
        row_bg_idle: theme::BG_PANEL,
        row_bg_hover: theme::BG_SELECTED,
    }
}

impl From<ContextMenu> for Element<'static, Message> {
    fn from(menu: ContextMenu) -> Self {
        let items = menu
            .items
            .into_iter()
            .map(|it| HoverableListItem {
                content: it.content,
                on_press: it.on_press,
                on_right_press: None,
                on_hover_enter: it.on_hover_enter,
                on_hover_exit: it.on_hover_exit,
                row_bg_idle: None,
                row_bg_hover: None,
                row_border_radius: None,
            })
            .collect();
        HoverableList::new(items, context_menu_style())
            .sticky_active(true)
            .into()
    }
}

pub fn context_menu_item(label: impl Into<String>, on_press: Option<Message>) -> ContextMenuItem {
    let content_label = label.into();
    let content: Element<'static, Message> = text(content_label)
        .size(theme::FONT_SM)
        .style(|_: &Theme| iced::widget::text::Style {
            color: Some(Color::WHITE),
        })
        .into();
    ContextMenuItem {
        on_press,
        on_hover_enter: None,
        on_hover_exit: None,
        content,
    }
}

pub fn context_menu_item_with_hover(
    label: impl Into<String>,
    on_press: Option<Message>,
    on_hover_enter: Option<Message>,
    on_hover_exit: Option<Message>,
) -> ContextMenuItem {
    let content_label = label.into();
    let content: Element<'static, Message> = text(content_label)
        .size(theme::FONT_SM)
        .style(|_: &Theme| iced::widget::text::Style {
            color: Some(Color::WHITE),
        })
        .into();
    ContextMenuItem {
        on_press,
        on_hover_enter,
        on_hover_exit,
        content,
    }
}

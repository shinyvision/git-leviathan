//! Repository tab bar — concrete consumer of the generic [`TabBar`] widget.
//! Maps `TabId` keys to messages, supplies the folder icon per tab, the
//! hover-swap close button, the leading "+" button (open repo dialog), and
//! the trailing version label. Drag-reorder mechanics live entirely in the
//! widget; this view only exposes selection and reorder commits.

use iced::{
    widget::{button, container},
    Border, Element, Length, Padding, Theme,
};

use crate::{
    assets,
    core::TabId,
    message::{AppMessage, Message},
    style, theme,
    widgets::primitives::hoverable::hoverable_swap,
    widgets::tab_bar::{TabBar, TabItem},
};

pub fn tab_bar_view(
    tabs: Vec<(TabId, String)>,
    active_tab_id: TabId,
) -> Element<'static, Message> {
    let items: Vec<TabItem<'_, TabId, Message>> = tabs
        .into_iter()
        .map(|(id, name)| {
            let is_active = id == active_tab_id;
            TabItem::new(id, name)
                .leading(folder_icon(is_active))
                .trailing(close_button(id))
        })
        .collect();

    TabBar::new(items, active_tab_id)
        .leading_slot(plus_button())
        .trailing_slot(version_label())
        .on_select(|id| Message::App(AppMessage::TabSelected(id)))
        .on_reorder(|order| Message::App(AppMessage::TabsReordered(order)))
        .into()
}

fn folder_icon<'a>(is_active: bool) -> Element<'a, Message> {
    container(assets::tab_icon(
        assets::FOLDER,
        if is_active {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_DIM
        },
    ))
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

fn close_button<'a>(tab_id: TabId) -> Element<'a, Message> {
    let close_idle: Element<'a, Message> =
        button(assets::tab_icon(assets::CLOSE, theme::TEXT_DIM))
            .style(|_: &Theme, _: button::Status| button::Style {
                background: None,
                text_color: theme::TEXT_DIM,
                border: Border::default(),
                shadow: Default::default(),
                snap: false,
            })
            .padding(Padding::from([4u16, 4]))
            .into();

    let close_hover: Element<'a, Message> =
        button(assets::tab_icon(assets::CLOSE, theme::TEXT_PRIMARY))
            .style(|_: &Theme, _: button::Status| button::Style {
                background: None,
                text_color: theme::TEXT_PRIMARY,
                border: Border::default(),
                shadow: Default::default(),
                snap: false,
            })
            .padding(Padding::from([4u16, 4]))
            .on_press(Message::App(AppMessage::TabClosed(tab_id)))
            .into();

    hoverable_swap(
        close_idle,
        close_hover,
        |_| iced::widget::container::Style::default(),
        |_| iced::widget::container::Style::default(),
    )
}

fn plus_button<'a>() -> Element<'a, Message> {
    button(
        container(assets::tab_icon(assets::PLUS, theme::TEXT_DIM))
            .align_y(iced::alignment::Vertical::Center)
            .height(Length::Fill),
    )
    .style(|_: &Theme, status: button::Status| button::Style {
        background: match status {
            button::Status::Hovered => Some(theme::BG_HOVER.into()),
            _ => None,
        },
        text_color: theme::TEXT_DIM,
        border: Border::default(),
        shadow: Default::default(),
        snap: false,
    })
    .padding(Padding::from([0, 12]))
    .height(Length::Fixed(theme::TAB_HEIGHT as f32))
    .on_press(Message::App(AppMessage::OpenRepoDialog))
    .into()
}

fn version_label<'a>() -> Element<'a, Message> {
    container(
        iced::widget::text(format!("v{}", env!("CARGO_PKG_VERSION")))
            .size(theme::FONT_SM)
            .style(style::secondary_text),
    )
    .padding(Padding::from([0, 12]))
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

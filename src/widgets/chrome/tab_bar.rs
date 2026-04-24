use iced::{
    widget::{button, container, row, text},
    Border, Element, Length, Padding, Point, Theme,
};

use crate::{
    assets,
    core::TabId,
    message::{AppMessage, Message},
    style, theme,
    widgets::primitives::{
        hoverable::{hoverable_swap, Hoverable, HoverStatus},
        DraggableTab,
    },
    widgets::shared::horizontal_space,
};

pub fn tab_bar_view(
    tabs: Vec<(TabId, String)>,
    active_tab_id: TabId,
    press_origin: Option<(TabId, Point)>,
    dragging: Option<TabId>,
) -> Element<'static, Message> {
    let any_drag_active = dragging.is_some();
    let pressed_tab = press_origin.map(|(id, _)| id);

    let mut items: Vec<Element<'static, Message>> = vec![button(
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
    .into()];

    for (tab_id, name) in tabs {
        let is_active = tab_id == active_tab_id;
        let is_dragging_this = dragging == Some(tab_id);
        let is_pressed_this = pressed_tab == Some(tab_id);

        let folder_icon: Element<'static, Message> = container(assets::tab_icon(
            assets::FOLDER,
            if is_active {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_DIM
            },
        ))
        .align_y(iced::alignment::Vertical::Center)
        .into();

        let close_idle: Element<'static, Message> =
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

        let close_hover: Element<'static, Message> =
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

        let close_centered: Element<'static, Message> = hoverable_swap(
            close_idle,
            close_hover,
            |_| iced::widget::container::Style::default(),
            |_| iced::widget::container::Style::default(),
        );

        let inner = container(
            row![
                folder_icon,
                text(name).size(theme::FONT_SM).style(if is_active {
                    style::primary_text
                } else {
                    style::secondary_text
                }),
                close_centered,
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .height(Length::Fill)
        .padding(Padding::from([0, 10]))
        .align_y(iced::alignment::Vertical::Center);

        let visual: Element<'static, Message> = Hoverable::new(inner, move |_: &Theme, status: HoverStatus| {
            let bg = if is_active || status.is_hovered() {
                theme::BG_HOVER
            } else {
                theme::BG_BASE
            };
            iced::widget::container::Style {
                background: Some(bg.into()),
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            }
        })
        .into();

        let tab_container: Element<'static, Message> = container(visual)
            .height(Length::Fixed(theme::TAB_HEIGHT as f32))
            .into();

        let draggable: Element<'static, Message> = DraggableTab::new(
            tab_container,
            tab_id,
            is_pressed_this,
            is_dragging_this,
            any_drag_active,
        )
        .into();

        items.push(draggable);
    }

    items.push(horizontal_space().into());
    items.push(
        container(
            text(format!("v{}", env!("CARGO_PKG_VERSION")))
                .size(theme::FONT_SM)
                .style(style::secondary_text),
        )
        .padding(Padding::from([0, 12]))
        .align_y(iced::alignment::Vertical::Center)
        .into(),
    );

    container(row(items).spacing(0).align_y(iced::Alignment::Center))
        .height(Length::Fixed(theme::TAB_HEIGHT as f32))
        .width(Length::Fill)
        .style(style::toolbar_container)
        .into()
}

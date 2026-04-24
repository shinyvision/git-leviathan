use iced::{
    widget::{button, container, row, text},
    Border, Element, Length, Padding, Theme,
};

use crate::{
    assets,
    core::TabId,
    message::{AppMessage, Message},
    style, theme,
    widgets::primitives::hoverable::hoverable_swap,
    widgets::shared::horizontal_space,
};

pub fn tab_bar_view(tabs: Vec<(TabId, String)>, active_tab_id: TabId) -> Element<'static, Message> {
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

        let tab = button(
            container(
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
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(style::tab_button(is_active))
        .padding(Padding::from([0, 10]))
        .height(Length::Fixed(theme::TAB_HEIGHT as f32))
        .on_press(Message::App(AppMessage::TabSelected(tab_id)));

        items.push(tab.into());
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


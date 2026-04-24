use iced::{
    widget::{button, column, container, row, text},
    Border, Color, Element, Length, Padding, Theme,
};

use std::time::Instant;

use crate::{
    assets,
    message::Message,
    style, theme,
    widgets::primitives::spinner,
    widgets::shared::horizontal_space,
};

pub fn main_bar_view<'a>(
    repo_name: &'a str,
    branch_name: &'a str,
    fetch_started_at: Option<Instant>,
    push_started_at: Option<Instant>,
    pull_started_at: Option<Instant>,
    branch_action: Option<Message>,
) -> Element<'a, Message> {
    let busy = push_started_at.is_some() || pull_started_at.is_some();
    let dim = |c: Color, enabled: bool| -> Color {
        if enabled {
            c
        } else {
            let bg = theme::BG_BASE;
            Color {
                r: c.r * 0.5 + bg.r * 0.5,
                g: c.g * 0.5 + bg.g * 0.5,
                b: c.b * 0.5 + bg.b * 0.5,
                a: c.a,
            }
        }
    };
    let wrap_spinner = |started_at: Instant, color: Color| -> Element<'a, Message> {
        container(spinner::spinner(started_at, color, 16.0))
            .padding(Padding {
                top: 2.0,
                bottom: 2.0,
                left: 2.0,
                right: 2.0,
            })
            .into()
    };

    let pull_enabled = !busy;
    let push_enabled = !busy;
    let branch_enabled = branch_action.is_some();

    let pull_icon_color = dim(theme::TEXT_SECONDARY, pull_enabled);
    let pull_text_color = dim(theme::TEXT_DIM, pull_enabled);
    let pull_icon: Element<'a, Message> = match pull_started_at {
        Some(started_at) => wrap_spinner(started_at, pull_icon_color),
        None => assets::toolbar_icon(assets::PULL, pull_icon_color),
    };
    let push_icon_color = dim(theme::TEXT_SECONDARY, push_enabled);
    let push_text_color = dim(theme::TEXT_DIM, push_enabled);
    let push_icon: Element<'a, Message> = match push_started_at {
        Some(started_at) => wrap_spinner(started_at, push_icon_color),
        None => assets::toolbar_icon(assets::PUSH, push_icon_color),
    };
    let branch_icon_color = dim(theme::TEXT_SECONDARY, branch_enabled);
    let branch_text_color = dim(theme::TEXT_DIM, branch_enabled);

    let action_button_style = |_: &Theme, status: button::Status| button::Style {
        background: match status {
            button::Status::Hovered => Some(theme::BG_HOVER.into()),
            _ => None,
        },
        text_color: theme::TEXT_PRIMARY,
        border: Border::default(),
        shadow: Default::default(),
        snap: false,
    };

    let mut pull_btn = button(
        column![
            pull_icon,
            text("Pull")
                .size(theme::FONT_XS)
                .style(move |_: &Theme| iced::widget::text::Style {
                    color: Some(pull_text_color)
                }),
        ]
        .spacing(3)
        .align_x(iced::Alignment::Center),
    )
    .style(action_button_style)
    .padding(Padding::from([6, 12]))
    .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32));
    if pull_enabled {
        pull_btn = pull_btn.on_press(Message::repo(
            crate::screens::repository::RepositoryMessage::PullRequested,
        ));
    }

    let mut push_btn = button(
        column![
            push_icon,
            text("Push")
                .size(theme::FONT_XS)
                .style(move |_: &Theme| iced::widget::text::Style {
                    color: Some(push_text_color)
                }),
        ]
        .spacing(3)
        .align_x(iced::Alignment::Center),
    )
    .style(action_button_style)
    .padding(Padding::from([6, 12]))
    .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32));
    if push_enabled {
        push_btn = push_btn.on_press(Message::repo(
            crate::screens::repository::RepositoryMessage::PushRequested,
        ));
    }

    let actions: Vec<(Element<'a, Message>, &'static str)> =
        vec![
        (pull_btn.into(), "Pull"),
        (push_btn.into(), "Push"),
        ({
            let mut branch_btn = button(
                column![
                    assets::toolbar_icon(assets::BRANCH, branch_icon_color),
                    text("Branch").size(theme::FONT_XS).style(move |_: &Theme| {
                        iced::widget::text::Style { color: Some(branch_text_color) }
                    }),
                ]
                .spacing(3)
                .align_x(iced::Alignment::Center),
            )
            .style(action_button_style)
            .padding(Padding::from([6, 12]))
            .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32));
            if let Some(action) = branch_action {
                branch_btn = branch_btn.on_press(action);
            }
            branch_btn.into()
        }, "Branch"),
        (
            button(
                column![
                    assets::toolbar_icon(assets::STASH, theme::TEXT_SECONDARY),
                    text("Stash").size(theme::FONT_XS).style(style::dim_text),
                ]
                .spacing(3)
                .align_x(iced::Alignment::Center),
            )
            .style(|_: &Theme, status: button::Status| button::Style {
                background: match status {
                    button::Status::Hovered => Some(theme::BG_HOVER.into()),
                    _ => None,
                },
                text_color: theme::TEXT_PRIMARY,
                border: Border::default(),
                shadow: Default::default(),
            snap: false,
            })
            .padding(Padding::from([6, 12]))
            .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32))
            .on_press(Message::repo(
                crate::screens::repository::RepositoryMessage::Center(
                    crate::screens::repository::panel_messages::CenterAction::StashCreateRequested,
                ),
            ))
            .into(),
            "Stash",
        ),
        (
            button(
                column![
                    assets::toolbar_icon(assets::POP, theme::TEXT_SECONDARY),
                    text("Pop").size(theme::FONT_XS).style(style::dim_text),
                ]
                .spacing(3)
                .align_x(iced::Alignment::Center),
            )
            .style(|_: &Theme, status: button::Status| button::Style {
                background: match status {
                    button::Status::Hovered => Some(theme::BG_HOVER.into()),
                    _ => None,
                },
                text_color: theme::TEXT_PRIMARY,
                border: Border::default(),
                shadow: Default::default(),
            snap: false,
            })
            .padding(Padding::from([6, 12]))
            .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32))
            .on_press(Message::repo(
                crate::screens::repository::RepositoryMessage::Center(
                    crate::screens::repository::panel_messages::CenterAction::StashPopRequested {
                        stash_index: 0,
                    },
                ),
            ))
            .into(),
            "Pop",
        ),
    ];

    let repo_col = column![
        text("repository")
            .size(theme::FONT_XS)
            .style(style::dim_text),
        text(repo_name)
            .size(theme::FONT_SM)
            .style(style::primary_text),
    ];

    let branch_col = column![
        text("branch").size(theme::FONT_XS).style(style::dim_text),
        text(branch_name)
            .size(theme::FONT_SM)
            .style(|_: &Theme| text::Style {
                color: Some(theme::TEXT_ACTIVE_BRANCH)
            }),
    ];

    let refresh_indicator: Element<'a, Message> = match fetch_started_at {
        Some(started_at) => spinner::spinner(started_at, theme::TEXT_DIM, 13.0),
        None => assets::tab_icon(assets::REFRESH, theme::TEXT_DIM),
    };

    let left = row![
        container(repo_col).padding(Padding::from([0, 10])),
        assets::icon(assets::CHEVRON_RIGHT, 20.0, theme::BORDER),
        container(branch_col).padding(Padding::from([0, 10])),
        container(refresh_indicator).padding(Padding::from([0, 6])),
    ]
    .align_y(iced::Alignment::Center);

    let action_btns: Vec<Element<Message>> = actions.into_iter().map(|(btn, _label)| btn).collect();

    let center = row(action_btns).spacing(0).align_y(iced::Alignment::Center);

    let search_size = 32.0_f32;
    let search_btn = button(
        container(assets::icon(assets::SEARCH, 16.0, theme::TEXT_SECONDARY))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(search_size))
    .height(Length::Fixed(search_size))
    .padding(0)
    .style(|_: &Theme, status: button::Status| button::Style {
        background: match status {
            button::Status::Hovered => Some(theme::BG_HOVER.into()),
            _ => None,
        },
        text_color: theme::TEXT_PRIMARY,
        border: Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        shadow: Default::default(),
        snap: false,
    })
    .on_press(Message::repo(
        crate::screens::repository::RepositoryMessage::OpenCommitSearch,
    ));

    let right = row![search_btn]
        .align_y(iced::Alignment::Center)
        .padding(Padding::from([0, 10]));

    let bar = row![left, horizontal_space(), center, horizontal_space(), right,]
        .align_y(iced::Alignment::Center)
        .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32));

    container(bar)
        .height(Length::Fixed(theme::TOOLBAR_HEIGHT as f32))
        .width(Length::Fill)
        .style(style::toolbar_container)
        .into()
}

//! Rendering for the AddRemote side panel. State+animation live in `mod.rs`;
//! this file is pure view composition so the state file stays small.

use iced::{
    widget::{button, column, container, row, text, text_input, MouseArea},
    Border, Color, Element, Length, Padding, Theme,
};

use crate::{assets, message::Message, style, theme, widgets::shared::horizontal_space};

use super::super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::styles::{green_button_style, input_style};
use super::{input_id, State, PANEL_WIDTH};

const COMMIT_ACTION_BUTTON_HEIGHT: f32 = 40.0;

pub(crate) fn view<'a>(state: &'a State) -> Element<'a, Message> {
    let close_btn = button(assets::icon(assets::CLOSE, 12.0, Color::WHITE))
        .on_press(Message::repo(RepositoryMessage::OverlayPanel(
            OverlayPanelAction::AddRemoteClose,
        )))
        .padding(Padding::from([4, 8]))
        .style(|_: &Theme, status: button::Status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(theme::BG_HOVER.into()),
                _ => None,
            },
            border: Border::default(),
            text_color: theme::TEXT_DIM,
            shadow: Default::default(),
            snap: false,
        });

    let header = row![
        assets::icon(assets::CLOUD, 16.0, theme::TEXT_SECONDARY),
        text("Add Remote")
            .size(theme::FONT_LG)
            .style(style::primary_text),
        horizontal_space().width(Length::Fill),
        close_btn,
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .padding(Padding {
        top: 12.0,
        right: 16.0,
        bottom: 12.0,
        left: 16.0,
    });

    let name_input = form_text_input(
        "origin",
        &state.name,
        OverlayPanelAction::AddRemoteNameChanged,
    )
    .id(input_id());
    let pull_url_input = form_text_input(
        "https://github.com/user/repo.git",
        &state.pull_url,
        OverlayPanelAction::AddRemotePullUrlChanged,
    );
    let push_url_input = form_text_input(
        "Push URL (optional)",
        &state.push_url,
        OverlayPanelAction::AddRemotePushUrlChanged,
    );

    let can_submit = state.can_submit();
    let add_btn = button(
        container(text("Add Remote").size(theme::FONT_SM))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center),
    )
    .style(green_button_style(can_submit))
    .padding(Padding::from([0, 16]))
    .height(Length::Fixed(COMMIT_ACTION_BUTTON_HEIGHT))
    .width(Length::Fill);

    let add_btn = if can_submit {
        add_btn.on_press(Message::repo(RepositoryMessage::OverlayPanel(
            OverlayPanelAction::AddRemoteConfirmed,
        )))
    } else {
        add_btn
    };

    let form = column![
        label_container("Name"),
        input_container(name_input.into()),
        label_container("Pull URL"),
        input_container(pull_url_input.into()),
        label_container("Push URL"),
        input_container(push_url_input.into()),
        container(add_btn).padding(Padding {
            top: 0.0,
            right: 16.0,
            bottom: 16.0,
            left: 16.0
        }),
    ]
    .spacing(0);

    let panel_content = column![header, form].spacing(0).height(Length::Fill);

    container(panel_content)
        .width(Length::Fixed(PANEL_WIDTH))
        .height(Length::Fill)
        .style(|_: &Theme| container::Style {
            background: Some(theme::BG_PANEL.into()),
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

pub(crate) fn overlay_layers<'a>(
    state: &'a State,
    sidebar_width: f32,
) -> Vec<Element<'a, Message>> {
    use crate::widgets::SlideOverlay;

    let slide = state.slide_offset();
    let left_offset = sidebar_width + 5.0;
    let top_offset = theme::TAB_HEIGHT as f32 + 22.0 + theme::TOOLBAR_HEIGHT as f32;

    let backdrop = MouseArea::new(
        container(horizontal_space())
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(Message::repo(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::AddRemoteClose,
    )))
    .into();

    let panel_elem = view(state);
    let panel_barrier = MouseArea::new(
        container(panel_elem)
            .width(Length::Fixed(PANEL_WIDTH))
            .height(Length::Fill),
    )
    .on_press(Message::noop());

    let positioned_panel = SlideOverlay::new(
        panel_barrier,
        slide,
        top_offset,
        left_offset,
        PANEL_WIDTH,
        theme::STATUS_BAR_HEIGHT as f32,
    )
    .into();

    vec![backdrop, positioned_panel]
}

fn form_text_input<'a>(
    placeholder: &'static str,
    value: &'a str,
    on_input: fn(String) -> OverlayPanelAction,
) -> iced::widget::TextInput<'a, Message> {
    text_input(placeholder, value)
        .on_input(move |s| Message::repo(RepositoryMessage::OverlayPanel(on_input(s))))
        .on_submit(Message::repo(RepositoryMessage::OverlayPanel(
            OverlayPanelAction::AddRemoteConfirmed,
        )))
        .size(theme::FONT_SM)
        .padding(theme::INPUT_PADDING)
        .width(Length::Fill)
        .style(input_style)
}

fn label_container(label: &'static str) -> iced::widget::Container<'static, Message> {
    container(text(label).size(theme::FONT_SM).style(style::dim_text)).padding(Padding {
        top: 16.0,
        right: 16.0,
        bottom: 4.0,
        left: 16.0,
    })
}

fn input_container<'a>(input: Element<'a, Message>) -> iced::widget::Container<'a, Message> {
    container(input).padding(Padding {
        top: 0.0,
        right: 16.0,
        bottom: 8.0,
        left: 16.0,
    })
}

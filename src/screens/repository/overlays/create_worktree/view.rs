//! Rendering for the CreateWorktree side panel. State+animation live in `mod.rs`;
//! this file is pure view composition so the state file stays small.

use iced::{
    widget::{button, column, container, row, text, text_input, MouseArea},
    Border, Color, Element, Length, Padding, Theme,
};

use crate::{
    assets,
    message::Message,
    style, theme,
    widgets::{
        dropdown::{dropdown_item, dropdown_menu, dropdown_trigger, icon_label, Dropdown},
        shared::horizontal_space,
    },
};

use super::super::super::{panel_messages::OverlayPanelAction, RepositoryMessage};
use super::styles::{blue_button_style, green_submit_style, input_style};
use super::{input_id, RefChoice, State, PANEL_WIDTH};

const SUBMIT_BUTTON_HEIGHT: f32 = 40.0;
const BROWSE_BUTTON_WIDTH: f32 = 90.0;

pub(crate) fn view<'a>(state: &'a State) -> Element<'a, Message> {
    let close_btn = button(assets::icon(assets::CLOSE, 12.0, Color::WHITE))
        .on_press(Message::repo(RepositoryMessage::OverlayPanel(
            OverlayPanelAction::CreateWorktreeClose,
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
        assets::icon(assets::TREE, 16.0, theme::TEXT_SECONDARY),
        text("Create Worktree").size(theme::FONT_LG).style(style::primary_text),
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

    let ref_dropdown = ref_dropdown_stack(state);

    let branch_input = form_text_input(
        "new-branch-name",
        &state.branch_name,
        OverlayPanelAction::CreateWorktreeBranchNameChanged,
    )
    .id(input_id());

    let working_dir_input = form_text_input(
        "/path/to/worktree",
        &state.working_dir,
        OverlayPanelAction::CreateWorktreeWorkingDirChanged,
    );

    let browse_btn = button(
        container(
            text("Browse").size(theme::FONT_SM),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center),
    )
    .on_press(Message::repo(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::CreateWorktreeBrowseRequested,
    )))
    .style(blue_button_style(true))
    .width(Length::Fixed(BROWSE_BUTTON_WIDTH))
    .height(Length::Fixed(theme::INPUT_HEIGHT));

    let working_dir_row = row![Element::from(working_dir_input), browse_btn]
        .spacing(8)
        .align_y(iced::Alignment::Center);

    let can_submit = state.can_submit();
    let submit_btn = button(
        container(
            text("Create Worktree").size(theme::FONT_SM),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center),
    )
    .style(green_submit_style(can_submit))
    .padding(Padding::from([0, 16]))
    .height(Length::Fixed(SUBMIT_BUTTON_HEIGHT))
    .width(Length::Fill);

    let submit_btn = if can_submit {
        submit_btn.on_press(Message::repo(RepositoryMessage::OverlayPanel(
            OverlayPanelAction::CreateWorktreeConfirmed,
        )))
    } else {
        submit_btn
    };

    let mut form_items: Vec<Element<Message>> = vec![
        label_container("Reference to checkout").into(),
        input_container(ref_dropdown).into(),
        label_container("Worktree branch to create").into(),
        input_container(branch_input.into()).into(),
        label_container("Working directory").into(),
        input_container(working_dir_row.into()).into(),
    ];

    if let Some(err) = &state.error {
        form_items.push(
            container(
                text(err.clone())
                    .size(theme::FONT_SM)
                    .style(|_: &Theme| text::Style {
                        color: Some(theme::ACCENT_RED),
                    }),
            )
            .padding(Padding::from([0, 16]))
            .into(),
        );
    }

    form_items.push(
        container(submit_btn)
            .padding(Padding {
                top: 8.0,
                right: 16.0,
                bottom: 16.0,
                left: 16.0,
            })
            .into(),
    );

    let form = column(form_items).spacing(0);

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
        OverlayPanelAction::CreateWorktreeClose,
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
            OverlayPanelAction::CreateWorktreeConfirmed,
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

fn ref_dropdown_stack<'a>(state: &'a State) -> Element<'a, Message> {
    let toggle_msg = Message::repo(RepositoryMessage::OverlayPanel(
        OverlayPanelAction::CreateWorktreeDropdownToggled,
    ));

    let label = state.reference.as_ref().map(|c| ref_label(c, theme::TEXT_PRIMARY));
    let trigger = dropdown_trigger(label, "Select a branch…", toggle_msg.clone());

    let menu = if state.dropdown_open {
        let items: Vec<Element<Message>> = state
            .available_refs
            .iter()
            .map(|choice| {
                dropdown_item(
                    ref_label(choice, theme::TEXT_PRIMARY),
                    Message::repo(RepositoryMessage::OverlayPanel(
                        OverlayPanelAction::CreateWorktreeReferenceChanged(choice.clone()),
                    )),
                )
            })
            .collect();
        Some(dropdown_menu(items))
    } else {
        None
    };

    Dropdown::new(trigger, menu, toggle_msg).into()
}

fn ref_label<'a>(choice: &'a RefChoice, text_color: Color) -> Element<'a, Message> {
    match choice {
        RefChoice::LocalBranch(name) => icon_label(
            assets::BRANCH,
            theme::TEXT_SECONDARY,
            text(name.clone())
                .size(theme::FONT_SM)
                .style(move |_: &Theme| text::Style { color: Some(text_color) })
                .into(),
        ),
        RefChoice::RemoteBranch { remote, branch } => {
            let parts: Element<Message> = row![
                text(format!("{remote}/"))
                    .size(theme::FONT_SM)
                    .style(|_: &Theme| text::Style {
                        color: Some(theme::ACCENT_BLUE),
                    }),
                text(branch.clone())
                    .size(theme::FONT_SM)
                    .style(move |_: &Theme| text::Style { color: Some(text_color) }),
            ]
            .spacing(0)
            .align_y(iced::Alignment::Center)
            .into();
            icon_label(assets::CLOUD, theme::TEXT_SECONDARY, parts)
        }
    }
}

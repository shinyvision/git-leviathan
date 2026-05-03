//! Top-level view composition for the diff panel. Two entry points:
//! `diff_center_view` for single-file diffs (dirty / commit / merged) and
//! `conflict_center_view` for the three-buffer conflict resolver. Both share
//! the same header via `diff_header`. Sub-modules carry the specialised
//! pieces: `rows` transforms diff/conflict data into canvas rows, `buffers`
//! builds the conflict side/output panels, `styles` holds scrollbar/button
//! styling.

use iced::{
    widget::{button, column, container, row, scrollable, text, MouseArea},
    Element, Length, Padding, Theme,
};

use crate::{
    assets,
    message::Message,
    services::{HighlightedFile, WorkingTreeDiffLine},
    style, theme,
    widgets::{
        diff_canvas::{
            self, diff_char_width, diff_content_canvas, diff_gutter_canvas, DiffSelection,
        },
        shared::horizontal_space,
    },
};

use crate::screens::repository::{
    panel_messages::{DetailAction, DiffPanelAction},
    state::diff_content_scroll_id,
    RepositoryMessage,
};

use super::conflict::ConflictResolverViewModel;

mod buffers;
mod rows;
mod styles;

pub(in crate::screens::repository) use rows::{
    build_conflict_rows_for_canvas, build_diff_rows_public,
};

use buffers::{conflict_buffer_panel, output_buffer_panel, ConflictBufferPanelInput};
use styles::{diff_scrollbar_style, save_button_style, DIFF_SCROLLBAR_WIDTH};

pub(in crate::screens::repository) struct DiffViewModel<'a> {
    pub(in crate::screens::repository) file_path: &'a str,
    pub(in crate::screens::repository) diff_lines: &'a [WorkingTreeDiffLine],
    pub(in crate::screens::repository) old_highlighted: Option<&'a HighlightedFile>,
    pub(in crate::screens::repository) new_highlighted: Option<&'a HighlightedFile>,
    pub(in crate::screens::repository) selection: Option<DiffSelection>,
    pub(in crate::screens::repository) scroll_y: f32,
    pub(in crate::screens::repository) shift_held: bool,
    pub(in crate::screens::repository) search_bar: Option<Element<'a, Message>>,
}

/// Shared header row: "Back to graph" button + right-aligned file path,
/// optionally followed by a trailing element (e.g. the Save button on the
/// conflict resolver). Identical styling/padding in both views.
fn diff_header<'a>(
    file_path: &'a str,
    trailing: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let back_btn = button(
        row![
            assets::icon(assets::UNDO, 14.0, theme::TEXT_SECONDARY),
            text("Back to graph")
                .size(theme::FONT_SM)
                .style(style::dim_text),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
    )
    .style(|_: &Theme, _: button::Status| button::Style {
        background: None,
        text_color: theme::TEXT_SECONDARY,
        border: Default::default(),
        shadow: Default::default(),
        snap: false,
    })
    .on_press(Message::repo(RepositoryMessage::Detail(
        DetailAction::CloseDirtyFileDiff,
    )))
    .padding(Padding::from([4, 8]));

    let file_label = text(file_path)
        .size(theme::FONT_SM)
        .style(|_: &Theme| text::Style {
            color: Some(theme::ACCENT_BLUE),
        });

    let header_row = match trailing {
        Some(trailing) => row![back_btn, horizontal_space(), file_label, trailing]
            .spacing(10)
            .align_y(iced::Alignment::Center)
            .padding(Padding::from([6, 10]))
            .width(Length::Fill),
        None => row![back_btn, horizontal_space(), file_label]
            .align_y(iced::Alignment::Center)
            .padding(Padding::from([6, 10]))
            .width(Length::Fill),
    };

    container(header_row)
        .width(Length::Fill)
        .style(style::header_container)
        .into()
}

pub(in crate::screens::repository) fn diff_center_view<'a>(
    model: DiffViewModel<'a>,
) -> Element<'a, Message> {
    let DiffViewModel {
        file_path,
        diff_lines,
        old_highlighted,
        new_highlighted,
        selection,
        scroll_y,
        shift_held,
        search_bar,
    } = model;

    let header_container = diff_header(file_path, None);

    let body: Element<'_, Message> = if diff_lines.is_empty() {
        container(
            text("No diff content")
                .size(theme::FONT_MD)
                .style(style::dim_text),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into()
    } else {
        let rows = rows::build_diff_rows(diff_lines, old_highlighted, new_highlighted);
        let char_w = diff_char_width();
        let data = diff_canvas::build_canvas_data(rows, char_w);

        // Layout: row[ sticky_gutter | scrollable(content_canvas) ]. Gutter
        // canvas sits *outside* the scrollable so horizontal scroll never
        // moves it; it reads `scroll_y` from the paired scrollable to mirror
        // vertical scrolling. `responsive` gives container size to the
        // content scrollable, so it stretches to fill and adds a horizontal
        // scrollbar when content overflows.
        let gutter_data = data.clone();
        let body_content = iced::widget::responsive(move |size| {
            let inner_viewport = iced::Size::new(
                diff_canvas::available_content_width(size.width),
                size.height,
            );
            let content = diff_content_canvas(
                data.clone(),
                selection,
                inner_viewport,
                DIFF_SCROLLBAR_WIDTH,
                scroll_y,
            );
            let scroller = scrollable(content)
                .id(diff_content_scroll_id())
                .width(Length::Fill)
                .height(Length::Fill)
                .direction(scrollable::Direction::Both {
                    vertical: scrollable::Scrollbar::new()
                        .width(DIFF_SCROLLBAR_WIDTH)
                        .scroller_width(DIFF_SCROLLBAR_WIDTH),
                    horizontal: scrollable::Scrollbar::new()
                        .width(DIFF_SCROLLBAR_WIDTH)
                        .scroller_width(DIFF_SCROLLBAR_WIDTH),
                })
                .style(diff_scrollbar_style)
                .on_scroll(|viewport| {
                    Message::repo(RepositoryMessage::DiffPanel(
                        DiffPanelAction::DiffContentScrolled { viewport },
                    ))
                });
            let scroller_el =
                crate::widgets::text::shift_wheel_lock(scroller, shift_held, |delta_lines| {
                    Message::repo(RepositoryMessage::DiffPanel(
                        DiffPanelAction::DiffShiftWheel { delta_lines },
                    ))
                });

            row![
                diff_gutter_canvas(gutter_data.clone(), scroll_y),
                scroller_el,
            ]
            .spacing(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        });

        container(body_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(theme::BG_PANEL.into()),
                ..Default::default()
            })
            .into()
    };

    let body = crate::widgets::search_widget::overlay(body, search_bar);
    let body = MouseArea::new(body).on_enter(Message::repo(RepositoryMessage::DiffPanel(
        DiffPanelAction::CanvasHoverEntered(diff_canvas::CANVAS_ID),
    )));

    let full = column![header_container, body]
        .spacing(0)
        .height(Length::Fill);

    container(full)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::panel_container)
        .into()
}

pub(in crate::screens::repository) fn conflict_center_view<'a>(
    model: ConflictResolverViewModel<'a>,
) -> Element<'a, Message> {
    let ConflictResolverViewModel {
        file_path,
        result,
        selections,
        ours_highlighted,
        theirs_highlighted,
        ours_scroll_offset_y,
        theirs_scroll_offset_y,
        output_scroll_offset_y,
        ours_selection,
        theirs_selection,
        output_selection,
        shift_held,
        search_overlay,
    } = model;

    let save_button: Option<Element<Message>> = result.map(|_| {
        button(text("Save").size(theme::FONT_SM).style(style::white_text))
            .style(save_button_style)
            .padding(Padding::from([4, 14]))
            .on_press(Message::repo(RepositoryMessage::DiffPanel(
                DiffPanelAction::ConflictResolutionSaveRequested,
            )))
            .into()
    });

    let header_container = diff_header(file_path, save_button);

    let body: Element<'a, Message> = match result {
        Some(result) => {
            use super::conflict::ConflictSide;
            let ours = conflict_buffer_panel(ConflictBufferPanelInput {
                prefix: "A",
                label: &result.ours_label,
                side: Some(ConflictSide::Ours),
                result,
                selections,
                highlighted: ours_highlighted.clone(),
                scroll_offset_y: ours_scroll_offset_y,
                selection: ours_selection,
                shift_held,
            });
            let theirs = conflict_buffer_panel(ConflictBufferPanelInput {
                prefix: "B",
                label: &result.theirs_label,
                side: Some(ConflictSide::Theirs),
                result,
                selections,
                highlighted: theirs_highlighted.clone(),
                scroll_offset_y: theirs_scroll_offset_y,
                selection: theirs_selection,
                shift_held,
            });
            let top_buffers = row![
                ours,
                container(horizontal_space())
                    .width(Length::Fixed(1.0))
                    .height(Length::Fill)
                    .style(|_: &Theme| container::Style {
                        background: Some(theme::BORDER.into()),
                        ..Default::default()
                    }),
                theirs,
            ]
            .spacing(0)
            .height(Length::FillPortion(2));

            let output = output_buffer_panel(
                result,
                selections,
                output_scroll_offset_y,
                output_selection,
                shift_held,
            );

            let body: Element<'a, Message> = column![top_buffers, output]
                .spacing(0)
                .height(Length::Fill)
                .into();

            // Single search overlay lives at a stable tree position so the
            // text_input keeps its state (caret, selection) when the hovered
            // buffer changes.
            crate::widgets::search_widget::overlay(body, search_overlay)
        }
        None => container(
            text("Loading conflict…")
                .size(theme::FONT_MD)
                .style(style::dim_text),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into(),
    };

    let full = column![header_container, body]
        .spacing(0)
        .height(Length::Fill);

    container(full)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::panel_container)
        .into()
}

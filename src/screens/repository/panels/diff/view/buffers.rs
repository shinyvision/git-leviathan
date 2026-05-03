//! Conflict-buffer sub-components: the per-side buffer panel (ours/theirs),
//! the output buffer panel, and the shared `row![ gutter | scrollable ]`
//! body used by all three. `diff_center_view` reuses none of these; the
//! single-file diff view has its own simpler body.

use std::sync::Arc;

use iced::{
    widget::{checkbox, column, container, row, scrollable, text, MouseArea},
    Border, Element, Length, Padding, Theme,
};

use crate::{
    message::Message,
    services::{ConflictBlock, ConflictResolutionResult, HighlightedFile},
    style, theme,
    widgets::{
        conflict_canvas::{
            self, canvas_id_for_side, conflict_content_canvas, conflict_gutter_canvas, side_color,
            CANVAS_ID_OUTPUT,
        },
        diff_canvas::{diff_char_width, DiffCanvasId, DiffSelection},
        shared::horizontal_space,
        text::TextCanvasData,
    },
};

use crate::screens::repository::{
    panel_messages::DiffPanelAction,
    state::{conflict_ours_scroll_id, conflict_output_scroll_id, conflict_theirs_scroll_id},
    RepositoryMessage,
};

use super::super::conflict::{ConflictHunkSelection, ConflictScrollTarget, ConflictSide};
use super::rows::{build_conflict_output_rows, build_conflict_side_rows};
use super::styles::{
    both_scrollbars, conflict_checkbox_style, conflict_scrollbar_style, CONFLICT_SCROLLBAR_WIDTH,
};

pub(super) const CONFLICT_HEADER_HEIGHT: f32 = 28.0;
const CONFLICT_SIDE_MIN_BUFFER_WIDTH: f32 = 920.0;
const CONFLICT_OUTPUT_MIN_BUFFER_WIDTH: f32 = 1500.0;
pub(super) const CONFLICT_PICK_WIDTH: f32 = 24.0;

fn side_all_checkbox<'a>(
    result: &'a ConflictResolutionResult,
    selections: &'a [ConflictHunkSelection],
    side: ConflictSide,
) -> Element<'static, Message> {
    let hunk_count = result
        .blocks
        .iter()
        .filter(|block| matches!(block, ConflictBlock::Conflict(_)))
        .count();
    let is_checked =
        hunk_count > 0 && selections.len() >= hunk_count && selections.iter().all(|s| s.has(side));

    checkbox(is_checked)
        .size(14)
        .text_size(theme::FONT_SM)
        .on_toggle(move |_| {
            Message::repo(RepositoryMessage::DiffPanel(
                DiffPanelAction::ConflictSideAllToggled(side),
            ))
        })
        .style(conflict_checkbox_style)
        .into()
}

pub(super) struct ConflictBufferPanelInput<'a> {
    pub(super) prefix: &'a str,
    pub(super) label: &'a str,
    pub(super) side: Option<ConflictSide>,
    pub(super) result: &'a ConflictResolutionResult,
    pub(super) selections: &'a [ConflictHunkSelection],
    pub(super) highlighted: Option<Arc<HighlightedFile>>,
    pub(super) scroll_offset_y: f32,
    pub(super) selection: Option<DiffSelection>,
    pub(super) shift_held: bool,
}

pub(super) fn conflict_buffer_panel<'a>(
    input: ConflictBufferPanelInput<'a>,
) -> Element<'a, Message> {
    let ConflictBufferPanelInput {
        prefix,
        label,
        side,
        result,
        selections,
        highlighted,
        scroll_offset_y,
        selection,
        shift_held,
    } = input;

    let Some(side) = side else {
        return output_buffer_panel(result, selections, scroll_offset_y, selection, shift_held);
    };

    let label_color = side_color(side);
    let pick_all = container(side_all_checkbox(result, selections, side))
        .width(Length::Fixed(CONFLICT_PICK_WIDTH))
        .align_x(iced::alignment::Horizontal::Left)
        .padding(Padding {
            left: 5.0,
            ..Padding::ZERO
        });

    let header = row![
        pick_all,
        container(
            text(prefix.to_string())
                .size(theme::FONT_SM)
                .style(move |_: &Theme| text::Style {
                    color: Some(label_color),
                }),
        )
        .width(Length::Fixed(24.0))
        .align_x(iced::alignment::Horizontal::Center),
        text(label.to_string())
            .size(theme::FONT_SM)
            .style(style::secondary_text),
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center)
    .padding(Padding::from([0, 8]))
    .height(Length::Fixed(CONFLICT_HEADER_HEIGHT))
    .width(Length::Fill);

    let rows = build_conflict_side_rows(result, selections, side, highlighted.as_deref());
    let char_w = diff_char_width();
    let data = conflict_canvas::build_side_canvas_data(rows, char_w);
    let data_for_min = data.clone();
    let canvas_id = canvas_id_for_side(side);

    let body = conflict_scrolled_canvas(ConflictScrolledCanvasInput {
        canvas_id,
        data,
        selection,
        scroll_offset_y,
        min_buffer_width: CONFLICT_SIDE_MIN_BUFFER_WIDTH,
        variant: SideOrOutput::Side(side),
        gutter_data: data_for_min,
        shift_held,
    });

    let panel = container(column![header, body].spacing(0).height(Length::Fill))
        .height(Length::Fill)
        .width(Length::FillPortion(1))
        .style(|_: &Theme| container::Style {
            background: Some(theme::BG_PANEL.into()),
            border: Border {
                color: theme::BORDER,
                width: 0.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        });

    MouseArea::new(panel)
        .on_enter(Message::repo(RepositoryMessage::DiffPanel(
            DiffPanelAction::CanvasHoverEntered(canvas_id),
        )))
        .into()
}

pub(super) fn output_buffer_panel<'a>(
    result: &ConflictResolutionResult,
    selections: &[ConflictHunkSelection],
    scroll_offset_y: f32,
    selection: Option<DiffSelection>,
    shift_held: bool,
) -> Element<'a, Message> {
    let output_header = row![
        horizontal_space().width(Length::Fixed(CONFLICT_PICK_WIDTH)),
        text("Output")
            .size(theme::FONT_SM)
            .style(style::secondary_text),
        horizontal_space(),
        text("Save writes this file and marks it resolved")
            .size(theme::FONT_XS)
            .style(style::dim_text),
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center)
    .padding(Padding::from([0, 8]))
    .height(Length::Fixed(CONFLICT_HEADER_HEIGHT));

    let rows = build_conflict_output_rows(result, selections);
    let char_w = diff_char_width();
    let data = conflict_canvas::build_output_canvas_data(rows, char_w);
    let data_for_min = data.clone();

    let body = conflict_scrolled_canvas(ConflictScrolledCanvasInput {
        canvas_id: CANVAS_ID_OUTPUT,
        data,
        selection,
        scroll_offset_y,
        min_buffer_width: CONFLICT_OUTPUT_MIN_BUFFER_WIDTH,
        variant: SideOrOutput::Output,
        gutter_data: data_for_min,
        shift_held,
    });

    let panel = container(column![output_header, body].spacing(0).height(Length::Fill))
        .height(Length::FillPortion(1))
        .width(Length::Fill)
        .style(|_: &Theme| container::Style {
            border: Border {
                color: theme::BORDER,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        });

    MouseArea::new(panel)
        .on_enter(Message::repo(RepositoryMessage::DiffPanel(
            DiffPanelAction::CanvasHoverEntered(CANVAS_ID_OUTPUT),
        )))
        .into()
}

#[derive(Clone, Copy)]
enum SideOrOutput {
    Side(ConflictSide),
    Output,
}

/// Build a `row![ sticky_gutter_canvas | scrollable(content_canvas) ]`
/// body for a conflict buffer. Mirrors the diff-view layout.
struct ConflictScrolledCanvasInput {
    canvas_id: DiffCanvasId,
    data: Arc<TextCanvasData>,
    selection: Option<DiffSelection>,
    scroll_offset_y: f32,
    min_buffer_width: f32,
    variant: SideOrOutput,
    gutter_data: Arc<TextCanvasData>,
    shift_held: bool,
}

fn conflict_scrolled_canvas(input: ConflictScrolledCanvasInput) -> Element<'static, Message> {
    let ConflictScrolledCanvasInput {
        canvas_id,
        data,
        selection,
        scroll_offset_y,
        min_buffer_width,
        variant,
        gutter_data,
        shift_held,
    } = input;

    let data_for_canvas = data.clone();
    let view = iced::widget::responsive(move |size| {
        let gutter_w = data_for_canvas.gutter_width();
        let inner_viewport = iced::Size::new(
            (size.width - gutter_w - CONFLICT_SCROLLBAR_WIDTH).max(0.0),
            (size.height - CONFLICT_SCROLLBAR_WIDTH).max(0.0),
        );

        let min_width = min_buffer_width.max(inner_viewport.width);
        let mut effective_data = data_for_canvas.clone();
        if effective_data.content_width() < min_width {
            effective_data = Arc::new(effective_data.with_content_width(min_width));
        }

        let content = conflict_content_canvas(
            canvas_id,
            effective_data,
            selection,
            inner_viewport,
            CONFLICT_SCROLLBAR_WIDTH,
            scroll_offset_y,
        );

        let scroll_id = match variant {
            SideOrOutput::Side(ConflictSide::Ours) => conflict_ours_scroll_id(),
            SideOrOutput::Side(ConflictSide::Theirs) => conflict_theirs_scroll_id(),
            SideOrOutput::Output => conflict_output_scroll_id(),
        };

        let scroller = scrollable(content)
            .id(scroll_id)
            .width(Length::Fill)
            .height(Length::Fill)
            .direction(both_scrollbars())
            .style(conflict_scrollbar_style)
            .on_scroll(move |viewport| match variant {
                SideOrOutput::Side(side) => Message::repo(RepositoryMessage::DiffPanel(
                    DiffPanelAction::ConflictBufferScrolled { side, viewport },
                )),
                SideOrOutput::Output => Message::repo(RepositoryMessage::DiffPanel(
                    DiffPanelAction::ConflictOutputScrolled { viewport },
                )),
            });

        let target = match variant {
            SideOrOutput::Side(ConflictSide::Ours) => ConflictScrollTarget::Ours,
            SideOrOutput::Side(ConflictSide::Theirs) => ConflictScrollTarget::Theirs,
            SideOrOutput::Output => ConflictScrollTarget::Output,
        };
        let scroller_el =
            crate::widgets::text::shift_wheel_lock(scroller, shift_held, move |delta_lines| {
                Message::repo(RepositoryMessage::DiffPanel(
                    DiffPanelAction::ConflictShiftWheel {
                        target,
                        delta_lines,
                    },
                ))
            });

        row![
            conflict_gutter_canvas(canvas_id, gutter_data.clone(), scroll_offset_y),
            scroller_el,
        ]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    });

    Element::from(view)
}

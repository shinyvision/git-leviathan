//! `canvas::Program` implementation + widget factories.
//!
//! Interactivity flow (the widget emits these through closures supplied at
//! construction time — the generic layer is oblivious to the concrete
//! `Message` enum):
//!   - press in content → `on_selection_begin(canvas_id, pos, viewport_rect, data)`
//!   - drag            → `on_selection_extend(canvas_id, pos)`
//!   - release         → `on_selection_end(canvas_id)`
//!   - press on a gutter "hot spot" → `on_gutter_click(canvas_id, row, meta)`
//!
//! `meta` is an opaque u64 returned by the clicked row's `gutter_click()` —
//! encodes whatever the domain needs (e.g. hunk_idx for conflict checkboxes).
use std::sync::Arc;

use iced::{
    mouse,
    widget::canvas::{self, Canvas, Frame, Geometry, Program},
    Color, Element, Length, Point, Rectangle, Renderer, Size, Theme,
};

use crate::theme;

use super::hit_test::col_to_pixel;
use super::layout::{CanvasId, TextCanvasData, CONTENT_PAD_X};
use super::selection::{TextPosition, TextSelection};

const SELECTION_BG: Color = Color {
    r: 0.30,
    g: 0.45,
    b: 0.75,
    a: 0.45,
};
const GUTTER_BG: Color = Color {
    r: 0.055,
    g: 0.059,
    b: 0.086,
    a: 1.0,
};
const GUTTER_BORDER: Color = Color {
    r: 0.141,
    g: 0.149,
    b: 0.208,
    a: 1.0,
};

#[derive(Debug, Default)]
pub struct CanvasState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintMode {
    Gutter,
    Content,
}

/// Closures that turn raw canvas events into host `Message`s. Stored as
/// `Arc<dyn Fn>` so the same canvas can be rebuilt every frame without
/// per-frame allocation churn on the hot path.
pub struct CanvasCallbacks<Msg: 'static> {
    pub on_selection_begin:
        Arc<dyn Fn(CanvasId, TextPosition, Rectangle, Arc<TextCanvasData>) -> Msg + Send + Sync>,
    pub on_selection_extend: Arc<dyn Fn(CanvasId, TextPosition) -> Msg + Send + Sync>,
    pub on_selection_end: Arc<dyn Fn(CanvasId) -> Msg + Send + Sync>,
    pub on_gutter_click: Arc<dyn Fn(CanvasId, usize, u64) -> Msg + Send + Sync>,
}

impl<Msg: 'static> Clone for CanvasCallbacks<Msg> {
    fn clone(&self) -> Self {
        Self {
            on_selection_begin: self.on_selection_begin.clone(),
            on_selection_extend: self.on_selection_extend.clone(),
            on_selection_end: self.on_selection_end.clone(),
            on_gutter_click: self.on_gutter_click.clone(),
        }
    }
}

pub struct TextCanvasProgram<Msg: 'static> {
    pub data: Arc<TextCanvasData>,
    pub selection: Option<TextSelection>,
    pub canvas_id: CanvasId,
    pub mode: PaintMode,
    pub scroll_y: f32,
    pub viewport_w: f32,
    pub viewport_h: f32,
    pub callbacks: CanvasCallbacks<Msg>,
}

impl<Msg: 'static> TextCanvasProgram<Msg> {
    fn hit_test_local(&self, point: Point) -> Option<TextPosition> {
        // Content-mode hit test: point is widget-local (no scroll applied by
        // caller; content canvas is sized to the full content, so widget-
        // local == content-space here).
        self.data.hit_test(point)
    }

    fn gutter_row_at(&self, y: f32) -> Option<usize> {
        let content_y = y + self.scroll_y;
        self.data.row_at_y(content_y)
    }

    fn draw_gutter(&self, frame: &mut Frame, bounds: Rectangle) {
        let gutter_w = self.data.gutter_width;
        frame.fill_rectangle(
            Point::new(0.0, 0.0),
            Size::new(gutter_w, bounds.height),
            GUTTER_BG,
        );
        frame.fill_rectangle(
            Point::new(gutter_w - 1.0, 0.0),
            Size::new(1.0, bounds.height),
            GUTTER_BORDER,
        );

        let offs = &self.data.row_offsets;
        let scroll_y = self.scroll_y;
        let view_top = scroll_y;
        let view_bot = scroll_y + bounds.height;
        let range = self
            .data
            .visible_row_range(scroll_y, bounds.height, 0)
            .with_overscan;
        for idx in range.clone() {
            let row = &self.data.rows[idx];
            let y_top = offs[idx];
            let y_bot = offs[idx + 1];
            if y_bot < view_top || y_top > view_bot {
                continue;
            }
            let draw_y = y_top - scroll_y;
            row.draw_gutter(frame, draw_y, gutter_w);
        }
    }

    fn draw_content(&self, frame: &mut Frame, bounds: Rectangle) {
        let offs = &self.data.row_offsets;
        let content_w = bounds.width;
        let char_width = self.data.char_width;

        frame.fill_rectangle(
            Point::new(0.0, 0.0),
            Size::new(content_w, bounds.height),
            theme::BG_PANEL,
        );

        let viewport_h = if self.viewport_h > 0.0 {
            self.viewport_h
        } else {
            bounds.height
        };
        let view_top = self.scroll_y;
        let view_bot = self.scroll_y + viewport_h;
        let range = self
            .data
            .visible_row_range(self.scroll_y, viewport_h, 0)
            .with_overscan;
        for idx in range.clone() {
            let row = &self.data.rows[idx];
            let y_top = offs[idx];
            let y_bot = offs[idx + 1];
            if y_bot < view_top || y_top > view_bot {
                continue;
            }
            row.draw_content(frame, y_top, content_w, char_width);
        }

        if let Some(sel) = &self.selection {
            if !sel.is_empty() {
                let (start, end) = sel.ordered();
                let sel_range = range.start.max(start.row)..range.end.min(end.row + 1);
                for idx in sel_range {
                    let row = &self.data.rows[idx];
                    if !row.selectable() {
                        continue;
                    }
                    let char_count = row.char_count();
                    let from = if idx == start.row { start.col } else { 0 };
                    let to = if idx == end.row { end.col } else { char_count };
                    let from = from.min(char_count);
                    let to = to.min(char_count).max(from);
                    let sx = col_to_pixel(from, char_width);
                    let ex = if idx < end.row {
                        CONTENT_PAD_X
                            + (char_count as f32 * char_width).max((to - from) as f32 * char_width)
                            + char_width * 0.5
                    } else {
                        col_to_pixel(to, char_width)
                    };
                    let y = offs[idx];
                    let h = row.height();
                    frame.fill_rectangle(
                        Point::new(sx, y),
                        Size::new((ex - sx).max(0.0), h),
                        SELECTION_BG,
                    );
                }
            }
        }
    }
}

impl<Msg: 'static + Clone> Program<Msg> for TextCanvasProgram<Msg> {
    type State = CanvasState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        match self.mode {
            PaintMode::Gutter => self.draw_gutter(&mut frame, bounds),
            PaintMode::Content => self.draw_content(&mut frame, bounds),
        }
        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<Msg>> {
        use iced::mouse::Button;
        match self.mode {
            PaintMode::Gutter => {
                if let canvas::Event::Mouse(mouse::Event::ButtonPressed(Button::Left)) = event {
                    if let Some(local) = cursor.position_in(bounds) {
                        if let Some(row_idx) = self.gutter_row_at(local.y) {
                            let row_top_widget = self.data.row_offsets[row_idx] - self.scroll_y;
                            let local_y_in_row = local.y - row_top_widget;
                            if let Some(meta) = self.data.rows[row_idx].gutter_click(
                                local.x,
                                local_y_in_row,
                                self.data.gutter_width,
                            ) {
                                let msg =
                                    (self.callbacks.on_gutter_click)(self.canvas_id, row_idx, meta);
                                return Some(iced::widget::Action::publish(msg).and_capture());
                            }
                        }
                    }
                }
                None
            }
            PaintMode::Content => self.update_content(event, bounds, cursor),
        }
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match self.mode {
            PaintMode::Content => {
                if cursor.position_in(bounds).is_some() {
                    return mouse::Interaction::Text;
                }
            }
            PaintMode::Gutter => {
                if let Some(local) = cursor.position_in(bounds) {
                    if let Some(row_idx) = self.gutter_row_at(local.y) {
                        let row_top_widget = self.data.row_offsets[row_idx] - self.scroll_y;
                        let local_y_in_row = local.y - row_top_widget;
                        if self.data.rows[row_idx].gutter_hover_interactive(
                            local.x,
                            local_y_in_row,
                            self.data.gutter_width,
                        ) {
                            return mouse::Interaction::Pointer;
                        }
                    }
                }
            }
        }
        mouse::Interaction::default()
    }
}

impl<Msg: 'static + Clone> TextCanvasProgram<Msg> {
    fn update_content(
        &self,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<Msg>> {
        use iced::mouse::Button;
        let dragging = self.selection.as_ref().map(|s| s.dragging).unwrap_or(false);

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(Button::Left)) => {
                if let Some(local) = cursor.position_in(bounds) {
                    if let Some(pos) = self.hit_test_local(local) {
                        // Viewport rect used by the auto-scroll tick for
                        // edge detection. Prefer the smaller of (canvas
                        // layout bounds, provided viewport size) — bounds
                        // reflects the visible viewport when the canvas
                        // overflows; viewport_w/h is a hint from the caller
                        // so we don't include scrollbar rails.
                        let viewport_rect = Rectangle {
                            x: bounds.x,
                            y: bounds.y,
                            width: bounds.width.min(self.viewport_w),
                            height: bounds.height.min(self.viewport_h),
                        };
                        let msg = (self.callbacks.on_selection_begin)(
                            self.canvas_id,
                            pos,
                            viewport_rect,
                            self.data.clone(),
                        );
                        return Some(iced::widget::Action::publish(msg).and_capture());
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if dragging => {
                if let Some(raw) = cursor.position() {
                    let local = Point::new(
                        (raw.x - bounds.x).clamp(0.0, bounds.width),
                        (raw.y - bounds.y).clamp(0.0, bounds.height),
                    );
                    if let Some(pos) = self.hit_test_local(local) {
                        let msg = (self.callbacks.on_selection_extend)(self.canvas_id, pos);
                        return Some(iced::widget::Action::publish(msg));
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(Button::Left)) if dragging => {
                let msg = (self.callbacks.on_selection_end)(self.canvas_id);
                return Some(iced::widget::Action::publish(msg));
            }
            _ => {}
        }
        None
    }
}

/// Scrollable content canvas — sized to `max(content, viewport)` so row
/// backgrounds fill horizontally when content is narrow, and overflow the
/// viewport when content is wide (caller's scrollable picks up the overflow).
pub fn content_canvas<Msg: 'static + Clone>(
    canvas_id: CanvasId,
    data: Arc<TextCanvasData>,
    selection: Option<TextSelection>,
    viewport: iced::Size,
    bottom_pad: f32,
    scroll_y: f32,
    callbacks: CanvasCallbacks<Msg>,
) -> Element<'static, Msg> {
    let w = data.content_width.max(viewport.width).max(1.0);
    let h = (data.total_height + bottom_pad.max(0.0)).max(1.0);
    Canvas::new(TextCanvasProgram {
        data,
        selection,
        canvas_id,
        mode: PaintMode::Content,
        scroll_y,
        viewport_w: viewport.width,
        viewport_h: viewport.height,
        callbacks,
    })
    .width(Length::Fixed(w))
    .height(Length::Fixed(h))
    .into()
}

/// Sticky gutter canvas — mirrors `scroll_y` from the paired content
/// scrollable to draw gutter cells in vertical sync without itself being
/// scrolled.
pub fn gutter_canvas<Msg: 'static + Clone>(
    canvas_id: CanvasId,
    data: Arc<TextCanvasData>,
    scroll_y: f32,
    callbacks: CanvasCallbacks<Msg>,
) -> Element<'static, Msg> {
    let w = data.gutter_width;
    Canvas::new(TextCanvasProgram {
        data,
        selection: None,
        canvas_id,
        mode: PaintMode::Gutter,
        scroll_y,
        viewport_w: 0.0,
        viewport_h: 0.0,
        callbacks,
    })
    .width(Length::Fixed(w))
    .height(Length::Fill)
    .into()
}

/// Wrap a scrollable-containing element so that shift+wheel over it
/// translates to a horizontal scroll delta via `on_shift_wheel` and is
/// swallowed (preventing vertical scroll). When `shift_held` is false
/// the event passes through untouched. The wrapper is always present so
/// toggling `shift_held` doesn't re-key the child scrollable and lose
/// its scroll offset.
pub fn shift_wheel_lock<'a, Msg>(
    content: impl Into<Element<'a, Msg>>,
    shift_held: bool,
    on_shift_wheel: impl Fn(f32) -> Msg + 'a,
) -> Element<'a, Msg>
where
    Msg: 'a,
{
    crate::widgets::primitives::wheel_intercept::WheelIntercept::new(content)
        .enabled(shift_held)
        .on_scroll(move |delta| {
            let dy = match delta {
                mouse::ScrollDelta::Lines { y, .. } => y,
                mouse::ScrollDelta::Pixels { y, .. } => y / 60.0,
            };
            on_shift_wheel(dy)
        })
        .into()
}

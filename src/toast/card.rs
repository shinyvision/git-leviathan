//! Toast domain model + canvas card painting (background, strip, icons, text,
//! close button) and the Iced element wrapper.
use iced::{
    alignment, mouse,
    widget::{canvas, container},
    Color, Element, Length, Padding, Point, Rectangle, Renderer, Theme, Vector,
};
use std::cell::RefCell;

use crate::message::Message;
use crate::theme;

use super::text_paths::{measure_wrapped_text_height, push_wrapped_text_paths, CachedPathFill};
use super::{ToastVisual, TOAST_WIDTH};

const TOAST_MARGIN: f32 = 16.0;

const TOAST_MIN_HEIGHT: f32 = 96.0;
const CARD_RADIUS: f32 = 10.0;
const STRIP_WIDTH: f32 = 54.0;
const CLOSE_BUTTON_SIZE: f32 = 22.0;
const CLOSE_BUTTON_INSET: f32 = 10.0;
const TEXT_LEFT_INSET: f32 = 84.0;
const TEXT_TOP_INSET: f32 = 18.0;
const TEXT_BOTTOM_INSET: f32 = 18.0;
const TEXT_RIGHT_INSET: f32 = 16.0;
const TEXT_CLOSE_GAP: f32 = 12.0;
const TITLE_BODY_GAP: f32 = 10.0;
const TITLE_FONT_SIZE: f32 = 18.0;
const BODY_FONT_SIZE: f32 = 12.0;
const TITLE_LINE_HEIGHT: f32 = 22.0;
const BODY_LINE_HEIGHT: f32 = 17.0;
const TEXT_CONTENT_WIDTH: f32 = TOAST_WIDTH
    - TEXT_LEFT_INSET
    - TEXT_RIGHT_INSET
    - CLOSE_BUTTON_SIZE
    - CLOSE_BUTTON_INSET
    - TEXT_CLOSE_GAP;

const ERROR_STRIP: Color = theme::ACCENT_DANGER;
const SUCCESS_STRIP: Color = Color {
    r: 0.180,
    g: 0.718,
    b: 0.439,
    a: 1.0,
};
const CARD_BG: Color = Color {
    r: 0.208,
    g: 0.216,
    b: 0.267,
    a: 1.0,
};
const CARD_BORDER: Color = Color {
    r: 0.286,
    g: 0.302,
    b: 0.373,
    a: 1.0,
};
const TITLE_COLOR: Color = Color {
    r: 0.953,
    g: 0.961,
    b: 0.988,
    a: 1.0,
};
const BODY_COLOR: Color = Color {
    r: 0.765,
    g: 0.784,
    b: 0.847,
    a: 1.0,
};
const SHADOW_COLOR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.18,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Error,
    Success,
}

#[derive(Debug, Clone)]
pub struct ToastData {
    pub kind: ToastKind,
    pub title: String,
    pub body: String,
}

impl ToastData {
    pub fn error(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Error,
            title: title.into(),
            body: body.into(),
        }
    }

    pub fn success(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Success,
            title: title.into(),
            body: body.into(),
        }
    }

    pub fn delete_failed(branch_name: &str, is_remote: bool, body: impl Into<String>) -> Self {
        let reference_name = if is_remote {
            format!("refs/remotes/{}", branch_name)
        } else {
            format!("refs/heads/{}", branch_name)
        };

        Self::error(format!("Delete Failed: {}", reference_name), body)
    }

    pub fn rename_failed(
        old_name: &str,
        new_name: &str,
        is_remote: bool,
        body: impl Into<String>,
    ) -> Self {
        let reference_name = if is_remote {
            format!("refs/remotes/{}", old_name)
        } else {
            format!("refs/heads/{}", old_name)
        };

        Self::error(
            format!("Rename Failed: {} to {}", reference_name, new_name),
            body,
        )
    }

    pub fn create_failed(branch_name: &str, body: impl Into<String>) -> Self {
        let reference_name = format!("refs/heads/{}", branch_name);
        Self::error(format!("Create Branch Failed: {}", reference_name), body)
    }

    pub fn push_succeeded(branch_name: &str) -> Self {
        Self {
            kind: ToastKind::Success,
            title: "Push Succeeded".to_string(),
            body: format!("Pushed {} to remote", branch_name),
        }
    }
}

pub(super) fn toast_view(
    data: &ToastData,
    height: f32,
    visual: ToastVisual,
    bottom_offset: f32,
    dismiss_id: Option<u64>,
) -> Element<'static, Message> {
    container(
        container(
            canvas(ToastCard {
                data: data.clone(),
                dismiss_id,
                visual,
            })
            .width(Length::Fixed(TOAST_WIDTH))
            .height(Length::Fixed(height)),
        )
        .width(Length::Fixed(TOAST_WIDTH))
        .height(Length::Fixed(height))
        .clip(true),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(alignment::Horizontal::Left)
    .align_y(alignment::Vertical::Bottom)
    .padding(Padding {
        left: TOAST_MARGIN,
        bottom: TOAST_MARGIN + bottom_offset.max(0.0),
        ..Default::default()
    })
    .into()
}

#[derive(Debug, Clone)]
struct ToastCard {
    data: ToastData,
    dismiss_id: Option<u64>,
    visual: ToastVisual,
}

impl canvas::Program<Message> for ToastCard {
    type State = ToastCardState;

    fn update(
        &self,
        _state: &mut ToastCardState,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<Message>> {
        let dismiss_id = self.dismiss_id?;

        let is_hovering_close = cursor
            .position_in(bounds)
            .is_some_and(|position| close_button_bounds(self.visual).contains(position));

        if matches!(
            event,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) && is_hovering_close
        {
            return Some(
                iced::widget::Action::publish(Message::dismiss_toast(dismiss_id)).and_capture(),
            );
        }

        None
    }

    fn draw(
        &self,
        state: &ToastCardState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let card_size = frame.size();
        let close_hovered = is_hovering_close(self.dismiss_id, self.visual, bounds, cursor);

        frame.with_save(|frame| {
            frame.translate(Vector::new(self.visual.offset_x, 0.0));
            frame.scale(self.visual.scale);
            draw_card(
                frame,
                card_size,
                &self.data,
                self.visual.alpha,
                self.dismiss_id.is_some(),
                close_hovered,
                state,
            );
        });

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &ToastCardState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if is_hovering_close(self.dismiss_id, self.visual, bounds, cursor) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

#[derive(Debug, Default)]
struct ToastCardState {
    text_cache: RefCell<Option<CachedToastText>>,
}

#[derive(Debug, Clone)]
struct CachedToastText {
    title: String,
    body: String,
    fills: Vec<CachedPathFill>,
}

fn draw_card(
    frame: &mut canvas::Frame<Renderer>,
    size: iced::Size,
    toast: &ToastData,
    alpha: f32,
    show_close_button: bool,
    close_hovered: bool,
    state: &ToastCardState,
) {
    let shadow = canvas::Path::rounded_rectangle(Point::new(0.0, 4.0), size, CARD_RADIUS.into());
    frame.fill(&shadow, apply_alpha(SHADOW_COLOR, alpha));

    let background = canvas::Path::rounded_rectangle(Point::ORIGIN, size, CARD_RADIUS.into());
    frame.fill(&background, apply_alpha(CARD_BG, alpha));
    frame.stroke(
        &background,
        canvas::Stroke::default()
            .with_width(1.0)
            .with_color(apply_alpha(CARD_BORDER, alpha)),
    );

    let (strip_color, draw_icon): (Color, fn(&mut canvas::Frame<Renderer>, f32)) = match toast.kind
    {
        ToastKind::Error => (ERROR_STRIP, draw_error_icon),
        ToastKind::Success => (SUCCESS_STRIP, draw_success_icon),
    };
    let strip = canvas::Path::rounded_rectangle(
        Point::ORIGIN,
        iced::Size::new(STRIP_WIDTH, size.height),
        CARD_RADIUS.into(),
    );
    frame.fill(&strip, apply_alpha(strip_color, alpha));
    draw_icon(frame, alpha);
    draw_cached_text(frame, toast, alpha, state);
    if show_close_button {
        draw_close_button(frame, alpha, close_hovered);
    }
}

fn draw_error_icon(frame: &mut canvas::Frame<Renderer>, alpha: f32) {
    let center = Point::new(26.0, frame.height() / 2.0);
    let circle = canvas::Path::circle(center, 16.0);
    frame.fill(&circle, apply_alpha(Color::WHITE, alpha));

    let stroke = canvas::Stroke::default()
        .with_width(3.0)
        .with_line_cap(canvas::LineCap::Round)
        .with_color(apply_alpha(ERROR_STRIP, alpha));

    frame.stroke(
        &canvas::Path::line(
            Point::new(center.x - 5.0, center.y - 5.0),
            Point::new(center.x + 5.0, center.y + 5.0),
        ),
        stroke,
    );
    frame.stroke(
        &canvas::Path::line(
            Point::new(center.x + 5.0, center.y - 5.0),
            Point::new(center.x - 5.0, center.y + 5.0),
        ),
        stroke,
    );
}

fn draw_success_icon(frame: &mut canvas::Frame<Renderer>, alpha: f32) {
    let center = Point::new(26.0, frame.height() / 2.0);
    let circle = canvas::Path::circle(center, 16.0);
    frame.fill(&circle, apply_alpha(Color::WHITE, alpha));

    let stroke = canvas::Stroke::default()
        .with_width(3.0)
        .with_line_cap(canvas::LineCap::Round)
        .with_color(apply_alpha(SUCCESS_STRIP, alpha));

    frame.stroke(
        &canvas::Path::new(|path| {
            path.move_to(Point::new(center.x - 7.0, center.y));
            path.line_to(Point::new(center.x - 2.0, center.y + 6.0));
            path.line_to(Point::new(center.x + 8.0, center.y - 6.0));
        }),
        stroke,
    );
}

fn draw_close_button(frame: &mut canvas::Frame<Renderer>, alpha: f32, hovered: bool) {
    let bounds = close_button_bounds(ToastVisual {
        offset_x: 0.0,
        scale: 1.0,
        alpha: 1.0,
    });
    let background = canvas::Path::rounded_rectangle(
        Point::new(bounds.x, bounds.y),
        iced::Size::new(bounds.width, bounds.height),
        5.0.into(),
    );

    if hovered {
        frame.fill(&background, apply_alpha(CARD_BORDER, alpha));
    }

    let stroke = canvas::Stroke::default()
        .with_width(1.8)
        .with_line_cap(canvas::LineCap::Round)
        .with_color(apply_alpha(Color::WHITE, alpha * 0.72));
    let inset = 6.0;
    let top_left = Point::new(bounds.x + inset, bounds.y + inset);
    let top_right = Point::new(bounds.x + bounds.width - inset, bounds.y + inset);
    let bottom_left = Point::new(bounds.x + inset, bounds.y + bounds.height - inset);
    let bottom_right = Point::new(
        bounds.x + bounds.width - inset,
        bounds.y + bounds.height - inset,
    );

    frame.stroke(&canvas::Path::line(top_left, bottom_right), stroke);
    frame.stroke(&canvas::Path::line(top_right, bottom_left), stroke);
}

fn close_button_bounds(visual: ToastVisual) -> Rectangle {
    Rectangle {
        x: visual.offset_x + (TOAST_WIDTH - CLOSE_BUTTON_INSET - CLOSE_BUTTON_SIZE) * visual.scale,
        y: CLOSE_BUTTON_INSET * visual.scale,
        width: CLOSE_BUTTON_SIZE * visual.scale,
        height: CLOSE_BUTTON_SIZE * visual.scale,
    }
}

fn is_hovering_close(
    dismiss_id: Option<u64>,
    visual: ToastVisual,
    bounds: Rectangle,
    cursor: mouse::Cursor,
) -> bool {
    dismiss_id.is_some_and(|_| {
        cursor
            .position_in(bounds)
            .is_some_and(|position| close_button_bounds(visual).contains(position))
    })
}

fn apply_alpha(color: Color, alpha: f32) -> Color {
    Color {
        a: (color.a * alpha).clamp(0.0, 1.0),
        ..color
    }
}

fn draw_cached_text(
    frame: &mut canvas::Frame<Renderer>,
    toast: &ToastData,
    alpha: f32,
    state: &ToastCardState,
) {
    let mut cache = state.text_cache.borrow_mut();
    let needs_rebuild = cache
        .as_ref()
        .is_none_or(|cached| cached.title != toast.title || cached.body != toast.body);

    if needs_rebuild {
        *cache = Some(build_text_cache(toast));
    }

    if let Some(cached) = cache.as_ref() {
        for fill in &cached.fills {
            frame.fill(&fill.path, apply_alpha(fill.color, alpha));
        }
    }
}

fn build_text_cache(toast: &ToastData) -> CachedToastText {
    let mut fills = Vec::new();
    let title_height = push_wrapped_text_paths(
        &toast.title,
        Point::new(TEXT_LEFT_INSET, TEXT_TOP_INSET),
        TEXT_CONTENT_WIDTH,
        TITLE_FONT_SIZE,
        TITLE_LINE_HEIGHT,
        TITLE_COLOR,
        &mut fills,
    );
    let body_origin = Point::new(
        TEXT_LEFT_INSET,
        TEXT_TOP_INSET + title_height + TITLE_BODY_GAP,
    );
    push_wrapped_text_paths(
        &toast.body,
        body_origin,
        TEXT_CONTENT_WIDTH,
        BODY_FONT_SIZE,
        BODY_LINE_HEIGHT,
        BODY_COLOR,
        &mut fills,
    );

    CachedToastText {
        title: toast.title.clone(),
        body: toast.body.clone(),
        fills,
    }
}

pub(super) fn measure_toast_height(toast: &ToastData) -> f32 {
    let title_height = measure_wrapped_text_height(
        &toast.title,
        TEXT_CONTENT_WIDTH,
        TITLE_FONT_SIZE,
        TITLE_LINE_HEIGHT,
    );
    let body_height = measure_wrapped_text_height(
        &toast.body,
        TEXT_CONTENT_WIDTH,
        BODY_FONT_SIZE,
        BODY_LINE_HEIGHT,
    );

    (TEXT_TOP_INSET + title_height + TITLE_BODY_GAP + body_height + TEXT_BOTTOM_INSET)
        .max(TOAST_MIN_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::ToastData;

    #[test]
    fn delete_failed_formats_local_ref_title() {
        let toast = ToastData::delete_failed(
            "develop",
            false,
            "Cannot delete currently checked out branch. Switch branches first.",
        );

        assert_eq!(toast.title, "Delete Failed: refs/heads/develop");
    }

    #[test]
    fn delete_failed_formats_remote_ref_title() {
        let toast = ToastData::delete_failed("origin/develop", true, "failed");

        assert_eq!(toast.title, "Delete Failed: refs/remotes/origin/develop");
    }
}

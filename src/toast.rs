use iced::advanced::graphics::text::cosmic_text::Command as ZenoCommand;
use iced::{
    advanced::{
        graphics::text::{self as graphics_text, cosmic_text},
        text::{Shaping, Wrapping},
    },
    alignment, mouse,
    widget::{canvas, container, Stack},
    Border, Color, Element, Length, Padding, Point, Rectangle, Renderer, Task, Theme, Vector,
};
use std::cell::RefCell;

use crate::message::Message;
use crate::work::timer_work;

const TOAST_WIDTH: f32 = 420.0;
const TOAST_STACK_GAP: f32 = 10.0;
const TOAST_MARGIN: f32 = 16.0;
const MAX_VISIBLE_TOASTS: usize = 5;

const ENTER_MS: f32 = 180.0;
const HOLD_MS: f32 = 8000.0;
const EXIT_MS: f32 = 200.0;
const POSITION_EPSILON: f32 = 0.5;

const ENTER_START_X: f32 = -240.0;
const EXIT_END_X: f32 = -(TOAST_WIDTH + 32.0);
const MIN_SCALE: f32 = 0.88;
const STACK_SLIDE_SPEED_PX_PER_MS: f32 = 2.0;

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

const ERROR_STRIP: Color = Color {
    r: 0.858,
    g: 0.243,
    b: 0.243,
    a: 1.0,
};
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

#[derive(Debug, Clone)]
struct ActiveToast {
    id: u64,
    data: ToastData,
    height: f32,
    age_ms: f32,
    current_bottom_offset: f32,
    target_bottom_offset: f32,
}

impl ActiveToast {
    fn new(id: u64, data: ToastData, height: f32) -> Self {
        Self {
            id,
            data,
            height,
            age_ms: 0.0,
            current_bottom_offset: 0.0,
            target_bottom_offset: 0.0,
        }
    }

    fn advance_age(&mut self, delta_ms: f32) {
        self.age_ms = (self.age_ms + delta_ms).min(ENTER_MS);
    }

    fn update_position(&mut self, delta_ms: f32) {
        self.current_bottom_offset = move_toward(
            self.current_bottom_offset,
            self.target_bottom_offset,
            STACK_SLIDE_SPEED_PX_PER_MS * delta_ms,
        );
    }

    fn visual(&self) -> ToastVisual {
        if self.age_ms < ENTER_MS {
            let progress = ease_out_cubic(self.age_ms / ENTER_MS);
            ToastVisual {
                offset_x: lerp(ENTER_START_X, 0.0, progress),
                scale: lerp(MIN_SCALE, 1.0, progress),
                alpha: progress,
            }
        } else {
            ToastVisual {
                offset_x: 0.0,
                scale: 1.0,
                alpha: 1.0,
            }
        }
    }

    fn into_exiting(self, placement: ExitPlacement) -> ExitingToast {
        let visual = self.visual();

        ExitingToast {
            data: self.data,
            height: self.height,
            elapsed_ms: 0.0,
            current_bottom_offset: self.current_bottom_offset,
            target_bottom_offset: self.current_bottom_offset,
            placement,
            start_offset_x: visual.offset_x,
            start_scale: visual.scale,
            start_alpha: visual.alpha,
        }
    }

    fn is_animating(&self) -> bool {
        self.age_ms < ENTER_MS
            || (self.current_bottom_offset - self.target_bottom_offset).abs() > POSITION_EPSILON
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitPlacement {
    Fixed,
    AboveStack,
}

#[derive(Debug, Clone)]
struct ExitingToast {
    data: ToastData,
    height: f32,
    elapsed_ms: f32,
    current_bottom_offset: f32,
    target_bottom_offset: f32,
    placement: ExitPlacement,
    start_offset_x: f32,
    start_scale: f32,
    start_alpha: f32,
}

impl ExitingToast {
    fn update_position(&mut self, delta_ms: f32) {
        self.current_bottom_offset = move_toward(
            self.current_bottom_offset,
            self.target_bottom_offset,
            STACK_SLIDE_SPEED_PX_PER_MS * delta_ms,
        );
    }

    fn tick(&mut self, delta_ms: f32) -> bool {
        self.elapsed_ms += delta_ms;
        self.elapsed_ms >= EXIT_MS
    }

    fn is_animating(&self) -> bool {
        self.elapsed_ms < EXIT_MS
            || (self.current_bottom_offset - self.target_bottom_offset).abs() > POSITION_EPSILON
    }

    fn visual(&self) -> ToastVisual {
        let progress = ease_in_cubic(self.elapsed_ms / EXIT_MS);

        ToastVisual {
            offset_x: lerp(self.start_offset_x, EXIT_END_X, progress),
            scale: lerp(self.start_scale, MIN_SCALE, progress),
            alpha: lerp(self.start_alpha, 0.0, progress),
        }
    }
}

#[derive(Debug, Default)]
pub struct ToastManager {
    active: Vec<ActiveToast>,
    exiting: Vec<ExitingToast>,
    next_id: u64,
}

impl ToastManager {
    pub fn push(&mut self, toast: ToastData) -> u64 {
        let mut overflowed = false;

        while self.active.len() >= MAX_VISIBLE_TOASTS {
            self.force_pop_oldest(ExitPlacement::AboveStack);
            overflowed = true;
        }

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let height = measure_toast_height(&toast);
        self.active.push(ActiveToast::new(id, toast, height));
        self.update_targets();

        if overflowed {
            self.snap_overflow_reflow_positions();
        }

        id
    }

    pub fn dismiss(&mut self, id: u64) {
        let Some(index) = self.active.iter().position(|toast| toast.id == id) else {
            return;
        };

        self.exiting
            .push(self.active.remove(index).into_exiting(ExitPlacement::Fixed));
        self.update_targets();
    }

    pub fn tick(&mut self, delta_ms: f32) {
        if delta_ms <= 0.0 {
            return;
        }

        for toast in &mut self.active {
            toast.advance_age(delta_ms);
            if toast.is_animating() {
                toast.update_position(delta_ms);
            }
        }

        for toast in &mut self.exiting {
            if toast.is_animating() {
                toast.update_position(delta_ms);
            }
        }

        self.exiting.retain_mut(|toast| !toast.tick(delta_ms));
    }

    pub fn has_active_toasts(&self) -> bool {
        !self.active.is_empty() || !self.exiting.is_empty()
    }

    pub fn is_animating(&self) -> bool {
        self.active.iter().any(ActiveToast::is_animating)
            || self.exiting.iter().any(ExitingToast::is_animating)
    }

    pub fn overlay(&self) -> Option<Element<'static, Message>> {
        if !self.has_active_toasts() {
            return None;
        }

        let mut children = self
            .active
            .iter()
            .map(|toast| {
                toast_view(
                    &toast.data,
                    toast.height,
                    toast.visual(),
                    toast.current_bottom_offset,
                    Some(toast.id),
                )
            })
            .collect::<Vec<_>>();

        children.extend(self.exiting.iter().map(|toast| {
            toast_view(
                &toast.data,
                toast.height,
                toast.visual(),
                toast.current_bottom_offset,
                None,
            )
        }));

        Some(
            Stack::with_children(children)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        )
    }

    fn force_pop_oldest(&mut self, placement: ExitPlacement) {
        if self.active.is_empty() {
            return;
        }

        self.exiting
            .push(self.active.remove(0).into_exiting(placement));
        self.update_targets();
    }

    fn update_targets(&mut self) {
        let mut next_offset = 0.0;

        for toast in self.active.iter_mut().rev() {
            toast.target_bottom_offset = next_offset;
            next_offset += toast.height + TOAST_STACK_GAP;
        }

        for toast in self
            .exiting
            .iter_mut()
            .rev()
            .filter(|toast| toast.placement == ExitPlacement::AboveStack)
        {
            toast.target_bottom_offset = next_offset;
            next_offset += toast.height + TOAST_STACK_GAP;
        }
    }

    fn snap_overflow_reflow_positions(&mut self) {
        // When the stack is already full, snap the reflowed toasts into place so
        // only the entering toast and the overflowed exit animate.
        for toast in &mut self.active {
            toast.current_bottom_offset = toast.target_bottom_offset;
        }

        for toast in &mut self.exiting {
            if toast.placement == ExitPlacement::AboveStack {
                toast.current_bottom_offset = toast.target_bottom_offset;
            }
        }
    }
}

pub fn auto_dismiss_task(id: u64) -> Task<Message> {
    let delay = std::time::Duration::from_millis((ENTER_MS + HOLD_MS).round() as u64);
    Task::perform(
        async move {
            timer_work(delay).await;
            id
        },
        Message::dismiss_toast,
    )
}

fn toast_view(
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

#[derive(Debug, Clone, Copy)]
struct ToastVisual {
    offset_x: f32,
    scale: f32,
    alpha: f32,
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

#[derive(Debug, Clone)]
struct CachedPathFill {
    path: canvas::Path,
    color: Color,
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

    match toast.kind {
        ToastKind::Error => {
            let strip = canvas::Path::rounded_rectangle(
                Point::ORIGIN,
                iced::Size::new(STRIP_WIDTH, size.height),
                Border {
                    radius: CARD_RADIUS.into(),
                    ..Default::default()
                }
                .radius,
            );
            frame.fill(&strip, apply_alpha(ERROR_STRIP, alpha));
            draw_error_icon(frame, alpha);
        }
        ToastKind::Success => {
            let strip = canvas::Path::rounded_rectangle(
                Point::ORIGIN,
                iced::Size::new(STRIP_WIDTH, size.height),
                Border {
                    radius: CARD_RADIUS.into(),
                    ..Default::default()
                }
                .radius,
            );
            frame.fill(&strip, apply_alpha(SUCCESS_STRIP, alpha));
            draw_success_icon(frame, alpha);
        }
    }
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

fn measure_toast_height(toast: &ToastData) -> f32 {
    let title_height =
        measure_wrapped_text_height(&toast.title, TITLE_FONT_SIZE, TITLE_LINE_HEIGHT);
    let body_height = measure_wrapped_text_height(&toast.body, BODY_FONT_SIZE, BODY_LINE_HEIGHT);

    (TEXT_TOP_INSET + title_height + TITLE_BODY_GAP + body_height + TEXT_BOTTOM_INSET)
        .max(TOAST_MIN_HEIGHT)
}

fn measure_wrapped_text_height(content: &str, font_size: f32, line_height: f32) -> f32 {
    with_wrapped_buffer(
        content,
        TEXT_CONTENT_WIDTH,
        font_size,
        line_height,
        |buffer: &cosmic_text::Buffer,
         _: &mut graphics_text::FontSystem,
         _: &mut cosmic_text::SwashCache| {
            graphics_text::measure(buffer).0.height.max(line_height)
        },
    )
}

fn push_wrapped_text_paths(
    content: &str,
    origin: Point,
    max_width: f32,
    font_size: f32,
    line_height: f32,
    color: Color,
    fills: &mut Vec<CachedPathFill>,
) -> f32 {
    with_wrapped_buffer(
        content,
        max_width,
        font_size,
        line_height,
        |buffer: &cosmic_text::Buffer,
         font_system: &mut graphics_text::FontSystem,
         swash_cache: &mut cosmic_text::SwashCache| {
            for run in buffer.layout_runs() {
                for glyph in run.glyphs.iter() {
                    let offset = Point::new(
                        origin.x + glyph.x + glyph.x_offset,
                        origin.y + run.line_y + glyph.y_offset,
                    );
                    let cache_key = glyph.physical((0.0, 0.0), 1.0).cache_key;

                    if let Some(commands) =
                        swash_cache.get_outline_commands(font_system.raw(), cache_key)
                    {
                        let path = canvas::Path::new(|path| {
                            for command in commands {
                                match command {
                                    ZenoCommand::MoveTo(point) => {
                                        path.move_to(Point::new(
                                            point.x + offset.x,
                                            -point.y + offset.y,
                                        ));
                                    }
                                    ZenoCommand::LineTo(point) => {
                                        path.line_to(Point::new(
                                            point.x + offset.x,
                                            -point.y + offset.y,
                                        ));
                                    }
                                    ZenoCommand::CurveTo(control_a, control_b, to) => {
                                        path.bezier_curve_to(
                                            Point::new(
                                                control_a.x + offset.x,
                                                -control_a.y + offset.y,
                                            ),
                                            Point::new(
                                                control_b.x + offset.x,
                                                -control_b.y + offset.y,
                                            ),
                                            Point::new(to.x + offset.x, -to.y + offset.y),
                                        );
                                    }
                                    ZenoCommand::QuadTo(control, to) => {
                                        path.quadratic_curve_to(
                                            Point::new(control.x + offset.x, -control.y + offset.y),
                                            Point::new(to.x + offset.x, -to.y + offset.y),
                                        );
                                    }
                                    ZenoCommand::Close => {
                                        path.close();
                                    }
                                }
                            }
                        });

                        fills.push(CachedPathFill { path, color });
                    }
                }
            }

            graphics_text::measure(buffer).0.height.max(line_height)
        },
    )
}

fn with_wrapped_buffer<R>(
    content: &str,
    max_width: f32,
    font_size: f32,
    line_height: f32,
    f: impl FnOnce(
        &cosmic_text::Buffer,
        &mut graphics_text::FontSystem,
        &mut cosmic_text::SwashCache,
    ) -> R,
) -> R {
    let mut font_system = graphics_text::font_system()
        .write()
        .expect("write font system");
    let metrics = cosmic_text::Metrics::new(font_size, line_height.max(f32::MIN_POSITIVE));
    let mut buffer = cosmic_text::Buffer::new(font_system.raw(), metrics);

    buffer.set_wrap(
        font_system.raw(),
        graphics_text::to_wrap(Wrapping::WordOrGlyph),
    );
    buffer.set_size(font_system.raw(), Some(max_width), None);
    let attrs = graphics_text::to_attributes(iced::Font::default());
    buffer.set_text(
        font_system.raw(),
        content,
        &attrs,
        graphics_text::to_shaping(Shaping::Advanced, content),
        None,
    );

    let mut swash_cache = cosmic_text::SwashCache::new();

    f(&buffer, &mut font_system, &mut swash_cache)
}

fn lerp(start: f32, end: f32, progress: f32) -> f32 {
    start + (end - start) * progress.clamp(0.0, 1.0)
}

fn move_toward(current: f32, target: f32, max_step: f32) -> f32 {
    let delta = target - current;

    if delta.abs() <= max_step {
        target
    } else if delta.is_sign_positive() {
        current + max_step
    } else {
        current - max_step
    }
}

fn ease_out_cubic(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - progress).powi(3)
}

fn ease_in_cubic(progress: f32) -> f32 {
    progress.clamp(0.0, 1.0).powi(3)
}

#[cfg(test)]
mod tests {
    use super::{ToastData, ToastManager, ENTER_MS, EXIT_END_X, EXIT_MS, MAX_VISIBLE_TOASTS};

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

    #[test]
    fn toast_manager_removes_toast_after_dismiss_animation_finishes() {
        let mut manager = ToastManager::default();
        let toast_id = manager.push(ToastData::error("Test", "Body"));

        manager.tick(ENTER_MS);
        assert!(manager.has_active_toasts());
        assert_eq!(manager.active.len(), 1);
        assert!(!manager.is_animating());

        manager.dismiss(toast_id);
        assert!(manager.has_active_toasts());
        assert!(manager.active.is_empty());
        assert_eq!(manager.exiting.len(), 1);

        manager.tick(EXIT_MS);
        assert!(!manager.has_active_toasts());
    }

    #[test]
    fn toast_manager_force_pops_top_to_keep_stack_at_five() {
        let mut manager = ToastManager::default();

        for index in 0..=MAX_VISIBLE_TOASTS {
            manager.push(ToastData::error(format!("Toast {index}"), "Body"));
        }

        assert_eq!(manager.active.len(), MAX_VISIBLE_TOASTS);
        assert_eq!(manager.exiting.len(), 1);
        assert_eq!(manager.active.last().unwrap().data.title, "Toast 5");
        assert_eq!(manager.active.first().unwrap().data.title, "Toast 1");
    }

    #[test]
    fn overflow_popped_toast_retargets_above_visible_stack() {
        let mut manager = ToastManager::default();

        for index in 0..MAX_VISIBLE_TOASTS {
            manager.push(ToastData::error(format!("Toast {index}"), "Body"));
        }

        for toast in &mut manager.active {
            toast.current_bottom_offset = toast.target_bottom_offset;
        }

        let previous_top_offset = manager.active.first().unwrap().current_bottom_offset;

        manager.push(ToastData::error("Toast 5", "Body"));

        let exiting = manager.exiting.last().unwrap();
        assert!(exiting.current_bottom_offset > previous_top_offset);
        assert!(exiting.target_bottom_offset > previous_top_offset);

        for toast in &manager.active {
            assert_eq!(toast.current_bottom_offset, toast.target_bottom_offset);
        }
    }

    #[test]
    fn toast_manager_only_animates_while_toasts_are_moving() {
        let mut manager = ToastManager::default();

        manager.push(ToastData::error("Test", "Body"));
        assert!(manager.is_animating());

        manager.tick(ENTER_MS);
        assert!(!manager.is_animating());
    }

    #[test]
    fn overflow_insert_limits_animation_to_enter_and_exit_transitions() {
        let mut manager = ToastManager::default();

        for index in 0..MAX_VISIBLE_TOASTS {
            manager.push(ToastData::error(format!("Toast {index}"), "Body"));
        }

        for toast in &mut manager.active {
            toast.age_ms = ENTER_MS;
            toast.current_bottom_offset = toast.target_bottom_offset;
        }
        assert!(!manager.is_animating());

        manager.push(ToastData::error("Toast 5", "Body"));

        assert_eq!(manager.exiting.len(), 1);
        assert_eq!(
            manager
                .active
                .iter()
                .filter(|toast| toast.is_animating())
                .count(),
            1
        );
        assert!(manager
            .active
            .iter()
            .all(|toast| toast.current_bottom_offset == toast.target_bottom_offset));
    }

    #[test]
    fn exiting_toast_moves_left_off_screen() {
        const { assert!(EXIT_END_X < 0.0) };
    }
}

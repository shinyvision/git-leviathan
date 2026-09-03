//! Zoomable / pannable image canvas used by the media diff.
//!
//! One canvas draws one *pane*: a single image (2-up), a swipe composite of
//! two images, an onion-skin blend, or a precomputed pixel-difference map.
//! Zoom/pan state (`ImageView`) lives in the app so two panes can share it
//! ("linked" mode); the canvas only computes and publishes new views.
//! Animated images tick on their own via redraw requests and report frame
//! changes so toolbar counters stay accurate.

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::advanced::image::FilterMethod;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Program, Stroke, Text};
use iced::{alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

use crate::services::media::image::DecodedImage;
use crate::theme;

pub const MIN_SCALE: f32 = 0.02;
pub const MAX_SCALE: f32 = 64.0;
const PIXEL_GRID_MIN_SCALE: f32 = 8.0;
const SWIPE_HANDLE_RADIUS: f32 = 11.0;

/// Zoom/pan state shared by linked panes. `scale` is *pixels per image
/// pixel*; `None` means "fit to pane". `center` is the image point shown at
/// the pane centre, normalized to `0..1` so it survives different sizes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageView {
    pub scale: Option<f32>,
    pub center: (f32, f32),
}

impl Default for ImageView {
    fn default() -> Self {
        Self {
            scale: None,
            center: (0.5, 0.5),
        }
    }
}

impl ImageView {
    pub fn fit() -> Self {
        Self::default()
    }

    pub fn is_fit(&self) -> bool {
        self.scale.is_none()
    }
}

/// Playback state of an animated image. The wall clock drives frames; the
/// canvas derives the frame index from `anchor` and reports it back.
#[derive(Debug, Clone, Copy)]
pub struct AnimationPlayback {
    pub playing: bool,
    /// `(instant, loop phase in ms at that instant)` while playing.
    pub anchor: Option<(Instant, u32)>,
    /// Frame shown while paused.
    pub paused_frame: usize,
}

impl Default for AnimationPlayback {
    fn default() -> Self {
        Self {
            playing: true,
            anchor: None,
            paused_frame: 0,
        }
    }
}

impl AnimationPlayback {
    /// Loop phase in milliseconds at `now`.
    pub fn phase_ms(&self, image: &DecodedImage, now: Instant) -> u32 {
        if image.total_duration_ms == 0 {
            return 0;
        }
        match (self.playing, self.anchor) {
            (true, Some((at, phase))) => {
                let elapsed = now.saturating_duration_since(at).as_millis() as u64;
                ((phase as u64 + elapsed) % image.total_duration_ms as u64) as u32
            }
            _ => image.frame_start_ms(self.paused_frame.min(image.frame_count().saturating_sub(1))),
        }
    }

    pub fn frame_at(&self, image: &DecodedImage, now: Instant) -> usize {
        if !self.playing || self.anchor.is_none() {
            return self.paused_frame.min(image.frame_count().saturating_sub(1));
        }
        image.frame_index_at(self.phase_ms(image, now))
    }

    pub fn play(&mut self, image: &DecodedImage, now: Instant) {
        let phase = image.frame_start_ms(self.paused_frame.min(image.frame_count().saturating_sub(1)));
        self.anchor = Some((now, phase));
        self.playing = true;
    }

    pub fn pause(&mut self, image: &DecodedImage, now: Instant) {
        self.paused_frame = self.frame_at(image, now);
        self.playing = false;
        self.anchor = None;
    }

    pub fn seek_frame(&mut self, image: &DecodedImage, frame: usize, now: Instant) {
        let frame = frame.min(image.frame_count().saturating_sub(1));
        self.paused_frame = frame;
        if self.playing {
            self.anchor = Some((now, image.frame_start_ms(frame)));
        }
    }
}

/// One image plus which of its frames to show.
#[derive(Clone)]
pub struct ImageLayer {
    pub image: Arc<DecodedImage>,
    pub playback: AnimationPlayback,
}

impl ImageLayer {
    fn frame_index(&self, now: Instant) -> usize {
        self.playback.frame_at(&self.image, now)
    }
}

#[derive(Clone)]
pub enum PaneContent {
    Single(ImageLayer),
    /// `base` on the left of the split, `overlay` on the right.
    Swipe {
        base: ImageLayer,
        overlay: ImageLayer,
        position: f32,
    },
    Onion {
        base: ImageLayer,
        overlay: ImageLayer,
        opacity: f32,
    },
    Difference {
        handle: iced::widget::image::Handle,
        width: u32,
        height: u32,
    },
}

impl PaneContent {
    /// Image-space size the view is expressed against.
    fn image_size(&self) -> (f32, f32) {
        match self {
            PaneContent::Single(layer) => (layer.image.width as f32, layer.image.height as f32),
            PaneContent::Swipe { overlay, base, .. } | PaneContent::Onion { overlay, base, .. } => {
                let w = overlay.image.width.max(base.image.width) as f32;
                let h = overlay.image.height.max(base.image.height) as f32;
                (w, h)
            }
            PaneContent::Difference { width, height, .. } => (*width as f32, *height as f32),
        }
    }

    fn layers(&self) -> Vec<&ImageLayer> {
        match self {
            PaneContent::Difference { .. } => Vec::new(),
            PaneContent::Single(layer) => vec![layer],
            PaneContent::Swipe { base, overlay, .. } | PaneContent::Onion { base, overlay, .. } => {
                vec![base, overlay]
            }
        }
    }
}

/// Events the canvas publishes.
#[derive(Debug, Clone)]
pub enum ImageViewerEvent {
    ViewChanged(ImageView),
    /// Double-click: toggle between fit and 1:1.
    ToggleFit,
    SwipeMoved(f32),
    /// Effective pixels-per-image-pixel currently drawn (changes with fit)
    /// plus the pane size, so toolbar zoom steps can be centred properly.
    ScaleReported { scale: f32, pane: Size },
    /// Displayed animation frame changed (layer index, frame index).
    FrameChanged { layer: usize, frame: usize },
}

pub struct ImageViewerSpec<'a, Message> {
    pub content: PaneContent,
    pub view: ImageView,
    pub checkerboard: bool,
    pub nearest: bool,
    pub pixel_grid: bool,
    pub inspector: bool,
    pub on_event: Box<dyn Fn(ImageViewerEvent) -> Message + 'a>,
}

#[derive(Debug, Default)]
pub struct ViewerState {
    drag: Option<DragState>,
    hover: Option<Point>,
    last_scale: Option<f32>,
    last_frames: [Option<usize>; 2],
    last_redraw: Option<Instant>,
    last_click: Option<(Instant, Point)>,
}

#[derive(Debug, Clone, Copy)]
enum DragState {
    Pan { last: Point, moved: bool },
    Swipe,
}

struct Placement {
    /// Effective scale (pixels per image pixel).
    scale: f32,
    /// Top-left of the image in canvas coordinates.
    origin: Point,
    image_w: f32,
    image_h: f32,
}

impl Placement {
    fn rect(&self) -> Rectangle {
        Rectangle::new(
            self.origin,
            Size::new(self.image_w * self.scale, self.image_h * self.scale),
        )
    }

    fn to_image(&self, p: Point) -> Point {
        Point::new(
            (p.x - self.origin.x) / self.scale,
            (p.y - self.origin.y) / self.scale,
        )
    }
}

pub fn fit_scale(bounds: Size, image_w: f32, image_h: f32) -> f32 {
    if image_w <= 0.0 || image_h <= 0.0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return 1.0;
    }
    let pad = 16.0;
    let avail_w = (bounds.width - pad).max(1.0);
    let avail_h = (bounds.height - pad).max(1.0);
    (avail_w / image_w)
        .min(avail_h / image_h)
        .clamp(MIN_SCALE, 1.0)
}

/// Clamp a view so the image never leaves the pane entirely; axes on which
/// the image is smaller than the pane are centred.
pub fn clamp_view(view: ImageView, bounds: Size, image_w: f32, image_h: f32) -> ImageView {
    let Some(scale) = view.scale else {
        return ImageView::fit();
    };
    let scale = scale.clamp(MIN_SCALE, MAX_SCALE);
    let clamp_axis = |c: f32, image_len: f32, pane_len: f32| -> f32 {
        let shown = image_len * scale;
        if shown <= pane_len || image_len <= 0.0 {
            0.5
        } else {
            let half = pane_len / (2.0 * shown);
            c.clamp(half, 1.0 - half)
        }
    };
    ImageView {
        scale: Some(scale),
        center: (
            clamp_axis(view.center.0, image_w, bounds.width),
            clamp_axis(view.center.1, image_h, bounds.height),
        ),
    }
}

fn placement(view: ImageView, bounds: Size, image_w: f32, image_h: f32) -> Placement {
    let fit = fit_scale(bounds, image_w, image_h);
    let scale = view.scale.unwrap_or(fit).clamp(MIN_SCALE, MAX_SCALE);
    let view = clamp_view(
        ImageView {
            scale: Some(scale),
            center: if view.scale.is_none() {
                (0.5, 0.5)
            } else {
                view.center
            },
        },
        bounds,
        image_w,
        image_h,
    );
    let (cx, cy) = view.center;
    let origin = Point::new(
        (bounds.width / 2.0 - cx * image_w * scale).round(),
        (bounds.height / 2.0 - cy * image_h * scale).round(),
    );
    Placement {
        scale,
        origin,
        image_w,
        image_h,
    }
}

/// Zoom `view` by `factor` keeping the image point under `cursor` fixed.
pub fn zoom_at(
    view: ImageView,
    factor: f32,
    cursor: Point,
    bounds: Size,
    image_w: f32,
    image_h: f32,
) -> ImageView {
    let place = placement(view, bounds, image_w, image_h);
    let new_scale = (place.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
    let anchor = place.to_image(cursor);
    // New origin keeps `anchor` under the cursor; derive the centre from it.
    let origin_x = cursor.x - anchor.x * new_scale;
    let origin_y = cursor.y - anchor.y * new_scale;
    let cx = (bounds.width / 2.0 - origin_x) / (image_w * new_scale);
    let cy = (bounds.height / 2.0 - origin_y) / (image_h * new_scale);
    clamp_view(
        ImageView {
            scale: Some(new_scale),
            center: (cx, cy),
        },
        bounds,
        image_w,
        image_h,
    )
}

/// Zoom around the pane centre (toolbar buttons / keyboard).
pub fn zoom_centered(view: ImageView, factor: f32, bounds: Size, image_w: f32, image_h: f32) -> ImageView {
    zoom_at(
        view,
        factor,
        Point::new(bounds.width / 2.0, bounds.height / 2.0),
        bounds,
        image_w,
        image_h,
    )
}

pub fn pan_by(view: ImageView, delta: Vector, bounds: Size, image_w: f32, image_h: f32) -> ImageView {
    let place = placement(view, bounds, image_w, image_h);
    let (cx, cy) = view.center;
    let center = if view.scale.is_none() { (0.5, 0.5) } else { (cx, cy) };
    clamp_view(
        ImageView {
            scale: Some(place.scale),
            center: (
                center.0 - delta.x / (image_w * place.scale),
                center.1 - delta.y / (image_h * place.scale),
            ),
        },
        bounds,
        image_w,
        image_h,
    )
}

struct ImageViewerProgram<'a, Message> {
    spec: ImageViewerSpec<'a, Message>,
}

impl<'a, Message: Clone + 'a> Program<Message> for ImageViewerProgram<'a, Message> {
    type State = ViewerState;

    fn update(
        &self,
        state: &mut ViewerState,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let emit = |e: ImageViewerEvent| (self.spec.on_event)(e);
        let (iw, ih) = self.spec.content.image_size();
        let size = bounds.size();
        let place = placement(self.spec.view, size, iw, ih);

        match event {
            canvas::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                state.last_redraw = Some(*now);
                let mut action: Option<canvas::Action<Message>> = None;
                // Report fit-scale changes so the toolbar zoom label is right.
                if state.last_scale.is_none_or(|s| (s - place.scale).abs() > 1e-4) {
                    state.last_scale = Some(place.scale);
                    action = Some(canvas::Action::publish(emit(ImageViewerEvent::ScaleReported {
                        scale: place.scale,
                        pane: size,
                    })));
                }
                // Animation ticking.
                let mut next_wait: Option<Duration> = None;
                for (idx, layer) in self.spec.content.layers().iter().enumerate().take(2) {
                    if !layer.image.is_animated() {
                        continue;
                    }
                    let frame = layer.frame_index(*now);
                    if state.last_frames[idx] != Some(frame) {
                        state.last_frames[idx] = Some(frame);
                        if action.is_none() {
                            action = Some(canvas::Action::publish(emit(
                                ImageViewerEvent::FrameChanged { layer: idx, frame },
                            )));
                        }
                    }
                    if layer.playback.playing && layer.playback.anchor.is_some() {
                        let phase = layer.playback.phase_ms(&layer.image, *now);
                        let wait = layer.image.ms_until_next_frame(phase).max(1);
                        let wait = Duration::from_millis(wait as u64);
                        next_wait = Some(next_wait.map_or(wait, |w: Duration| w.min(wait)));
                    }
                }
                // Publishing a message already forces a redraw (and this
                // program gets a fresh RedrawRequested to reschedule); only
                // arm the timer when there is nothing to publish.
                match (action, next_wait) {
                    (Some(action), _) => Some(action),
                    (None, Some(wait)) => Some(canvas::Action::request_redraw_at(*now + wait)),
                    (None, None) => None,
                }
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let pos = cursor.position_in(bounds)?;
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 40.0,
                };
                if lines == 0.0 {
                    return None;
                }
                let factor = 1.15f32.powf(lines.clamp(-6.0, 6.0));
                let view = zoom_at(self.spec.view, factor, pos, size, iw, ih);
                Some(canvas::Action::publish(emit(ImageViewerEvent::ViewChanged(view))).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let pos = cursor.position_in(bounds)?;
                if *button == mouse::Button::Left {
                    if let PaneContent::Swipe { position, .. } = &self.spec.content {
                        let handle_x = place.rect().x + place.rect().width * position;
                        if (pos.x - handle_x).abs() <= SWIPE_HANDLE_RADIUS + 4.0 {
                            state.drag = Some(DragState::Swipe);
                            return Some(canvas::Action::request_redraw().and_capture());
                        }
                    }
                }
                if matches!(button, mouse::Button::Left | mouse::Button::Middle) {
                    state.drag = Some(DragState::Pan {
                        last: pos,
                        moved: false,
                    });
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                None
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let pos = cursor.position_in(bounds).or_else(|| {
                    // Keep dragging while the cursor is outside the pane.
                    cursor.position().map(|p| Point::new(p.x - bounds.x, p.y - bounds.y))
                });
                state.hover = cursor.position_in(bounds);
                match (state.drag, pos) {
                    (Some(DragState::Pan { last, .. }), Some(pos)) => {
                        let delta = Vector::new(pos.x - last.x, pos.y - last.y);
                        state.drag = Some(DragState::Pan { last: pos, moved: true });
                        if delta.x == 0.0 && delta.y == 0.0 {
                            return None;
                        }
                        let view = pan_by(self.spec.view, delta, size, iw, ih);
                        Some(
                            canvas::Action::publish(emit(ImageViewerEvent::ViewChanged(view)))
                                .and_capture(),
                        )
                    }
                    (Some(DragState::Swipe), Some(pos)) => {
                        let rect = place.rect();
                        let t = ((pos.x - rect.x) / rect.width.max(1.0)).clamp(0.0, 1.0);
                        Some(
                            canvas::Action::publish(emit(ImageViewerEvent::SwipeMoved(t)))
                                .and_capture(),
                        )
                    }
                    _ => {
                        if self.spec.inspector || matches!(self.spec.content, PaneContent::Swipe { .. }) {
                            Some(canvas::Action::request_redraw())
                        } else {
                            None
                        }
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(button)) => {
                if !matches!(button, mouse::Button::Left | mouse::Button::Middle) {
                    return None;
                }
                let drag = state.drag.take();
                if let Some(DragState::Pan { moved: false, last }) = drag {
                    // Click without movement: a second one within 400 ms at
                    // (nearly) the same spot toggles fit / 1:1.
                    if *button == mouse::Button::Left {
                        let now = Instant::now();
                        let double = state.last_click.is_some_and(|(at, p)| {
                            now.duration_since(at).as_millis() < 400
                                && (p.x - last.x).abs() < 4.0
                                && (p.y - last.y).abs() < 4.0
                        });
                        if double {
                            state.last_click = None;
                            return Some(
                                canvas::Action::publish(emit(ImageViewerEvent::ToggleFit))
                                    .and_capture(),
                            );
                        }
                        state.last_click = Some((now, last));
                        return Some(canvas::Action::request_redraw().and_capture());
                    }
                }
                Some(canvas::Action::request_redraw())
            }
            canvas::Event::Mouse(mouse::Event::CursorLeft) => {
                state.hover = None;
                Some(canvas::Action::request_redraw())
            }
            canvas::Event::Keyboard(_) => None,
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &ViewerState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        let now = state.last_redraw.unwrap_or_else(Instant::now);

        let (iw, ih) = self.spec.content.image_size();
        let place = placement(self.spec.view, size, iw, ih);
        let rect = place.rect();

        // Transparency backdrop.
        if self.spec.checkerboard {
            draw_checkerboard(&mut frame, rect, size);
        } else {
            frame.fill_rectangle(rect.position(), rect.size(), Color::from_rgb8(0x10, 0x11, 0x19));
        }

        let filter = if self.spec.nearest || place.scale >= 3.0 {
            FilterMethod::Nearest
        } else {
            FilterMethod::Linear
        };

        match &self.spec.content {
            PaneContent::Single(layer) => {
                draw_layer(&mut frame, layer, &place, filter, 1.0, now);
            }
            PaneContent::Swipe {
                base,
                overlay,
                position,
            } => {
                let split_x = rect.x + rect.width * position;
                let left = Rectangle::new(
                    Point::new(0.0, 0.0),
                    Size::new(split_x.max(0.0), size.height),
                );
                let right = Rectangle::new(
                    Point::new(split_x.max(0.0), 0.0),
                    Size::new((size.width - split_x).max(0.0), size.height),
                );
                frame.with_clip(left, |f| draw_layer(f, base, &place, filter, 1.0, now));
                frame.with_clip(right, |f| draw_layer(f, overlay, &place, filter, 1.0, now));
                draw_swipe_handle(&mut frame, split_x, rect, state.hover, state.drag.is_some());
            }
            PaneContent::Onion {
                base,
                overlay,
                opacity,
            } => {
                draw_layer(&mut frame, base, &place, filter, 1.0, now);
                draw_layer(&mut frame, overlay, &place, filter, *opacity, now);
            }
            PaneContent::Difference { handle, .. } => {
                frame.draw_image(
                    rect,
                    canvas::Image::new(handle.clone())
                        .filter_method(filter)
                        .snap(true),
                );
            }
        }

        if self.spec.pixel_grid && place.scale >= PIXEL_GRID_MIN_SCALE {
            draw_pixel_grid(&mut frame, &place, size);
        }

        // Thin frame around the image so its edge is visible on any backdrop.
        frame.stroke(
            &Path::rectangle(rect.position(), rect.size()),
            Stroke::default()
                .with_color(Color {
                    a: 0.35,
                    ..theme::BORDER
                })
                .with_width(1.0),
        );

        if self.spec.inspector {
            if let Some(hover) = state.hover {
                draw_inspector(&mut frame, &self.spec.content, &place, hover, size, now);
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &ViewerState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if matches!(state.drag, Some(DragState::Pan { .. })) {
            return mouse::Interaction::Grabbing;
        }
        if matches!(state.drag, Some(DragState::Swipe)) {
            return mouse::Interaction::ResizingHorizontally;
        }
        let Some(pos) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        if let PaneContent::Swipe { position, .. } = &self.spec.content {
            let (iw, ih) = self.spec.content.image_size();
            let place = placement(self.spec.view, bounds.size(), iw, ih);
            let handle_x = place.rect().x + place.rect().width * position;
            if (pos.x - handle_x).abs() <= SWIPE_HANDLE_RADIUS + 4.0 {
                return mouse::Interaction::ResizingHorizontally;
            }
        }
        mouse::Interaction::Grab
    }
}

fn draw_layer(
    frame: &mut Frame,
    layer: &ImageLayer,
    place: &Placement,
    filter: FilterMethod,
    opacity: f32,
    now: Instant,
) {
    let img = &layer.image;
    // Layers of different sizes share the pane's transform; each is drawn at
    // its own pixel size anchored at the top-left so overlays line up.
    let rect = Rectangle::new(
        place.origin,
        Size::new(img.width as f32 * place.scale, img.height as f32 * place.scale),
    );
    if let Some(svg) = &img.svg {
        frame.draw_svg(rect, iced::advanced::svg::Svg::new(svg.clone()).opacity(opacity));
        return;
    }
    let idx = layer.frame_index(now);
    let Some(image_frame) = img.frames.get(idx).or_else(|| img.frames.first()) else {
        return;
    };
    frame.draw_image(
        rect,
        canvas::Image::new(image_frame.handle.clone())
            .filter_method(filter)
            .opacity(opacity)
            .snap(place.scale >= 1.0),
    );
}

fn draw_checkerboard(frame: &mut Frame, rect: Rectangle, size: Size) {
    let clip = Rectangle::new(Point::ORIGIN, size).intersection(&rect);
    let Some(clip) = clip else {
        return;
    };
    let light = Color::from_rgb8(0x2a, 0x2c, 0x3a);
    let dark = Color::from_rgb8(0x1c, 0x1e, 0x2a);
    frame.fill_rectangle(clip.position(), clip.size(), dark);
    let cell = 12.0f32;
    let start_col = ((clip.x - rect.x) / cell).floor().max(0.0) as i32;
    let start_row = ((clip.y - rect.y) / cell).floor().max(0.0) as i32;
    let end_col = ((clip.x + clip.width - rect.x) / cell).ceil() as i32;
    let end_row = ((clip.y + clip.height - rect.y) / cell).ceil() as i32;
    // Cap cell count for very large panes (12px cells on 4K = ~65k cells).
    if (end_col - start_col) as i64 * (end_row - start_row) as i64 > 40_000 {
        return;
    }
    frame.with_clip(clip, |f| {
        for row in start_row..end_row {
            for col in start_col..end_col {
                if (row + col) % 2 == 0 {
                    continue;
                }
                let x = rect.x + col as f32 * cell;
                let y = rect.y + row as f32 * cell;
                f.fill_rectangle(Point::new(x, y), Size::new(cell, cell), light);
            }
        }
    });
}

fn draw_pixel_grid(frame: &mut Frame, place: &Placement, size: Size) {
    let rect = place.rect();
    let Some(clip) = Rectangle::new(Point::ORIGIN, size).intersection(&rect) else {
        return;
    };
    let color = Color {
        a: 0.25,
        ..Color::BLACK
    };
    let stroke = Stroke::default().with_color(color).with_width(1.0);
    let first_col = ((clip.x - rect.x) / place.scale).floor() as i32;
    let last_col = ((clip.x + clip.width - rect.x) / place.scale).ceil() as i32;
    let first_row = ((clip.y - rect.y) / place.scale).floor() as i32;
    let last_row = ((clip.y + clip.height - rect.y) / place.scale).ceil() as i32;
    if (last_col - first_col) + (last_row - first_row) > 4000 {
        return;
    }
    for col in first_col..=last_col {
        let x = (rect.x + col as f32 * place.scale).round() + 0.5;
        frame.stroke(
            &Path::line(Point::new(x, clip.y), Point::new(x, clip.y + clip.height)),
            stroke,
        );
    }
    for row in first_row..=last_row {
        let y = (rect.y + row as f32 * place.scale).round() + 0.5;
        frame.stroke(
            &Path::line(Point::new(clip.x, y), Point::new(clip.x + clip.width, y)),
            stroke,
        );
    }
}

fn draw_swipe_handle(frame: &mut Frame, x: f32, rect: Rectangle, hover: Option<Point>, dragging: bool) {
    let top = rect.y.max(0.0);
    let bottom = (rect.y + rect.height).min(frame.height());
    let hot = dragging || hover.is_some_and(|h| (h.x - x).abs() <= SWIPE_HANDLE_RADIUS + 4.0);
    let color = if hot {
        theme::ACCENT_BLUE
    } else {
        Color::WHITE
    };
    frame.stroke(
        &Path::line(Point::new(x, top), Point::new(x, bottom)),
        Stroke::default()
            .with_color(Color { a: 0.55, ..Color::BLACK })
            .with_width(3.0),
    );
    frame.stroke(
        &Path::line(Point::new(x, top), Point::new(x, bottom)),
        Stroke::default().with_color(color).with_width(1.5),
    );
    let cy = (top + bottom) / 2.0;
    let knob = Point::new(x, cy);
    frame.fill(&Path::circle(knob, SWIPE_HANDLE_RADIUS), Color { a: 0.85, ..theme::BG_HEADER });
    frame.stroke(
        &Path::circle(knob, SWIPE_HANDLE_RADIUS),
        Stroke::default().with_color(color).with_width(1.5),
    );
    // ‹ › glyph
    let arrows = Path::new(|b| {
        b.move_to(Point::new(x - 6.0, cy));
        b.line_to(Point::new(x - 2.5, cy - 3.5));
        b.move_to(Point::new(x - 6.0, cy));
        b.line_to(Point::new(x - 2.5, cy + 3.5));
        b.move_to(Point::new(x + 6.0, cy));
        b.line_to(Point::new(x + 2.5, cy - 3.5));
        b.move_to(Point::new(x + 6.0, cy));
        b.line_to(Point::new(x + 2.5, cy + 3.5));
    });
    frame.stroke(&arrows, Stroke::default().with_color(color).with_width(1.5));
}

fn draw_inspector(
    frame: &mut Frame,
    content: &PaneContent,
    place: &Placement,
    hover: Point,
    size: Size,
    now: Instant,
) {
    let img_pt = place.to_image(hover);
    let px = img_pt.x.floor();
    let py = img_pt.y.floor();
    if px < 0.0 || py < 0.0 || px >= place.image_w || py >= place.image_h {
        return;
    }
    let (x, y) = (px as u32, py as u32);
    let mut lines = vec![format!("x {x}   y {y}")];
    for (label, layer) in content_layers_labelled(content) {
        let img = &layer.image;
        if x >= img.width || y >= img.height {
            continue;
        }
        let idx = layer.frame_index(now);
        let Some(f) = img.frames.get(idx).or_else(|| img.frames.first()) else {
            continue;
        };
        // Rasterized SVGs may be scaled relative to the intrinsic size.
        let (fw, fh) = frame_dims(f, img);
        let sx = ((x as f32 / img.width as f32) * fw as f32) as u32;
        let sy = ((y as f32 / img.height as f32) * fh as f32) as u32;
        let i = ((sy.min(fh.saturating_sub(1)) as usize) * fw as usize + sx.min(fw.saturating_sub(1)) as usize) * 4;
        if i + 3 < f.rgba.len() {
            let (r, g, b, a) = (f.rgba[i], f.rgba[i + 1], f.rgba[i + 2], f.rgba[i + 3]);
            lines.push(format!(
                "{label}#{r:02X}{g:02X}{b:02X}{a:02X}  rgba({r}, {g}, {b}, {})",
                a as f32 / 255.0
            ));
        }
    }
    if lines.len() < 2 {
        return;
    }
    let line_h = 16.0;
    let w = 300.0f32.min(size.width - 16.0);
    let h = line_h * lines.len() as f32 + 10.0;
    let x0 = 8.0;
    let y0 = size.height - h - 8.0;
    frame.fill(
        &Path::rounded_rectangle(Point::new(x0, y0), Size::new(w, h), 4.0.into()),
        Color { a: 0.88, ..theme::BG_HEADER },
    );
    for (i, line) in lines.iter().enumerate() {
        frame.fill_text(Text {
            content: line.clone(),
            position: Point::new(x0 + 8.0, y0 + 5.0 + i as f32 * line_h),
            color: if i == 0 {
                theme::TEXT_SECONDARY
            } else {
                theme::TEXT_PRIMARY
            },
            size: iced::Pixels(11.0),
            font: theme::MONO,
            align_x: iced::advanced::text::Alignment::Left,
            align_y: alignment::Vertical::Top,
            ..Text::default()
        });
    }
}

fn frame_dims(frame: &crate::services::media::image::ImageFrame, img: &DecodedImage) -> (u32, u32) {
    let pixels = (frame.rgba.len() / 4) as u64;
    if img.width == 0 || img.height == 0 {
        return (0, 0);
    }
    if pixels == img.width as u64 * img.height as u64 {
        return (img.width, img.height);
    }
    let scale = (pixels as f64 / (img.width as f64 * img.height as f64)).sqrt();
    let w = ((img.width as f64 * scale).round() as u32).max(1);
    (w, (pixels / w as u64) as u32)
}

fn content_layers_labelled(content: &PaneContent) -> Vec<(&'static str, &ImageLayer)> {
    match content {
        PaneContent::Difference { .. } => Vec::new(),
        PaneContent::Single(layer) => vec![("", layer)],
        PaneContent::Swipe { base, overlay, .. } | PaneContent::Onion { base, overlay, .. } => {
            vec![("old  ", base), ("new  ", overlay)]
        }
    }
}

pub fn image_viewer<'a, Message: Clone + 'a>(
    spec: ImageViewerSpec<'a, Message>,
) -> Element<'a, Message> {
    canvas::Canvas::new(ImageViewerProgram { spec })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_scale_never_upscales() {
        assert_eq!(fit_scale(Size::new(1000.0, 1000.0), 100.0, 50.0), 1.0);
        let s = fit_scale(Size::new(216.0, 1000.0), 400.0, 100.0);
        assert!((s - 0.5).abs() < 1e-6);
    }

    #[test]
    fn clamp_centres_small_images_and_bounds_large_ones() {
        let bounds = Size::new(200.0, 200.0);
        let v = clamp_view(
            ImageView {
                scale: Some(1.0),
                center: (0.9, 0.1),
            },
            bounds,
            100.0,
            100.0,
        );
        assert_eq!(v.center, (0.5, 0.5));
        let v = clamp_view(
            ImageView {
                scale: Some(1.0),
                center: (0.0, 1.0),
            },
            bounds,
            1000.0,
            1000.0,
        );
        assert!((v.center.0 - 0.1).abs() < 1e-6);
        assert!((v.center.1 - 0.9).abs() < 1e-6);
    }

    #[test]
    fn zoom_at_keeps_cursor_point_fixed() {
        let bounds = Size::new(400.0, 400.0);
        let (iw, ih) = (1000.0, 1000.0);
        let view = ImageView {
            scale: Some(1.0),
            center: (0.5, 0.5),
        };
        let cursor = Point::new(100.0, 300.0);
        let before = placement(view, bounds, iw, ih).to_image(cursor);
        let zoomed = zoom_at(view, 2.0, cursor, bounds, iw, ih);
        let after = placement(zoomed, bounds, iw, ih).to_image(cursor);
        assert!((before.x - after.x).abs() < 1.5, "{before:?} vs {after:?}");
        assert!((before.y - after.y).abs() < 1.5, "{before:?} vs {after:?}");
        assert_eq!(zoomed.scale, Some(2.0));
    }

    #[test]
    fn zoom_is_clamped_to_limits() {
        let bounds = Size::new(400.0, 400.0);
        let v = zoom_centered(
            ImageView {
                scale: Some(60.0),
                center: (0.5, 0.5),
            },
            10.0,
            bounds,
            100.0,
            100.0,
        );
        assert_eq!(v.scale, Some(MAX_SCALE));
        let v = zoom_centered(
            ImageView {
                scale: Some(0.03),
                center: (0.5, 0.5),
            },
            0.01,
            bounds,
            100.0,
            100.0,
        );
        assert_eq!(v.scale, Some(MIN_SCALE));
    }

    #[test]
    fn pan_moves_center_in_image_space() {
        let bounds = Size::new(200.0, 200.0);
        let view = ImageView {
            scale: Some(2.0),
            center: (0.5, 0.5),
        };
        let v = pan_by(view, Vector::new(-100.0, 0.0), bounds, 1000.0, 1000.0);
        assert!((v.center.0 - 0.55).abs() < 1e-6);
        assert_eq!(v.center.1, 0.5);
    }

    #[test]
    fn animation_playback_tracks_frames_from_anchor() {
        let bytes = {
            use image::codecs::gif::GifEncoder;
            use image::{Delay, Frame as ImgFrame, ImageBuffer, Rgba};
            let mut buf = std::io::Cursor::new(Vec::new());
            {
                let mut enc = GifEncoder::new(&mut buf);
                for c in [[255, 0, 0, 255], [0, 255, 0, 255]] {
                    let img = ImageBuffer::from_pixel(2, 2, Rgba(c));
                    enc.encode_frame(ImgFrame::from_parts(
                        img,
                        0,
                        0,
                        Delay::from_numer_denom_ms(100, 1),
                    ))
                    .unwrap();
                }
            }
            buf.into_inner()
        };
        let image = crate::services::media::image::decode_image(&bytes, "a.gif").unwrap();
        let t0 = Instant::now();
        let mut pb = AnimationPlayback::default();
        pb.play(&image, t0);
        assert_eq!(pb.frame_at(&image, t0), 0);
        assert_eq!(pb.frame_at(&image, t0 + Duration::from_millis(150)), 1);
        assert_eq!(pb.frame_at(&image, t0 + Duration::from_millis(250)), 0);
        pb.pause(&image, t0 + Duration::from_millis(150));
        assert!(!pb.playing);
        assert_eq!(pb.frame_at(&image, t0 + Duration::from_secs(5)), 1);
        pb.seek_frame(&image, 0, t0);
        assert_eq!(pb.frame_at(&image, t0), 0);
    }
}

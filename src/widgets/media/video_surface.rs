//! Canvas that presents a `VideoPlayer`'s frames. On every redraw it lets
//! the player advance its clock and pull due frames, then draws the current
//! frame letter-boxed into its bounds. While playing it schedules redraws at
//! half the frame interval so no frame is presented late.

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::advanced::image::FilterMethod;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Program, Stroke, Text};
use iced::{alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::services::media::video::VideoPlayer;
use crate::theme;

#[derive(Debug, Clone)]
pub enum VideoSurfaceEvent {
    /// Click on the picture toggles play/pause.
    TogglePlay,
    /// Playback stopped at the end (or the player state changed otherwise).
    PlaybackStateChanged,
}

pub struct VideoSurfaceSpec<'a, Message> {
    pub player: Arc<VideoPlayer>,
    pub on_event: Box<dyn Fn(VideoSurfaceEvent) -> Message + 'a>,
}

#[derive(Debug, Default)]
pub struct SurfaceState {
    last_playing: Option<bool>,
    last_redraw: Option<Instant>,
    hover: bool,
    pressed_at: Option<Instant>,
}

struct VideoSurfaceProgram<'a, Message> {
    spec: VideoSurfaceSpec<'a, Message>,
}

/// Largest rectangle with the video's aspect ratio that fits `bounds`.
pub fn letterbox(bounds: Size, width: u32, height: u32) -> Rectangle {
    if width == 0 || height == 0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return Rectangle::new(Point::ORIGIN, bounds);
    }
    let scale = (bounds.width / width as f32).min(bounds.height / height as f32);
    let w = (width as f32 * scale).floor();
    let h = (height as f32 * scale).floor();
    Rectangle::new(
        Point::new(((bounds.width - w) / 2.0).floor(), ((bounds.height - h) / 2.0).floor()),
        Size::new(w, h),
    )
}

impl<'a, Message: Clone + 'a> Program<Message> for VideoSurfaceProgram<'a, Message> {
    type State = SurfaceState;

    fn update(
        &self,
        state: &mut SurfaceState,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let emit = |e: VideoSurfaceEvent| (self.spec.on_event)(e);
        let player = &self.spec.player;
        match event {
            canvas::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                state.last_redraw = Some(*now);
                let changed = player.advance(*now);
                let playing = player.is_playing();
                if state.last_playing.is_some_and(|p| p != playing) {
                    state.last_playing = Some(playing);
                    return Some(canvas::Action::publish(emit(
                        VideoSurfaceEvent::PlaybackStateChanged,
                    )));
                }
                state.last_playing = Some(playing);
                if playing || player.is_buffering() {
                    let interval = player.info().frame_interval_secs();
                    let wait = Duration::from_secs_f64((interval / 2.0).clamp(0.004, 0.05));
                    return Some(canvas::Action::request_redraw_at(*now + wait));
                }
                if changed {
                    return Some(canvas::Action::request_redraw());
                }
                // Paused but still waiting for the poster / stepped frame.
                if player.current_frame().is_none() || player.is_buffering() {
                    return Some(canvas::Action::request_redraw_at(
                        *now + Duration::from_millis(40),
                    ));
                }
                None
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let hovering = cursor.position_in(bounds).is_some();
                if hovering != state.hover {
                    state.hover = hovering;
                    return Some(canvas::Action::request_redraw());
                }
                None
            }
            canvas::Event::Mouse(mouse::Event::CursorLeft) => {
                state.hover = false;
                Some(canvas::Action::request_redraw())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                cursor.position_in(bounds)?;
                state.pressed_at = Some(Instant::now());
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let pressed = state.pressed_at.take()?;
                cursor.position_in(bounds)?;
                if pressed.elapsed().as_millis() < 500 {
                    return Some(
                        canvas::Action::publish(emit(VideoSurfaceEvent::TogglePlay)).and_capture(),
                    );
                }
                None
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &SurfaceState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let size = bounds.size();
        let mut frame = Frame::new(renderer, size);
        let player = &self.spec.player;
        let info = player.info();

        frame.fill_rectangle(Point::ORIGIN, size, Color::BLACK);

        let rect = letterbox(size, info.width, info.height);
        match player.current_frame() {
            Some(video_frame) => {
                frame.draw_image(
                    rect,
                    canvas::Image::new(video_frame.handle.clone())
                        .filter_method(FilterMethod::Linear)
                        .snap(true),
                );
            }
            None => {
                frame.fill_rectangle(rect.position(), rect.size(), Color::from_rgb8(0x0b, 0x0c, 0x12));
            }
        }

        if let Some(err) = player.error() {
            draw_banner(&mut frame, size, &err, theme::ACCENT_DANGER);
        } else if player.is_buffering() {
            draw_spinner(&mut frame, size, state.last_redraw.unwrap_or_else(Instant::now));
        }

        // Big play glyph while paused (subtle when hovering).
        if !player.is_playing() && player.error().is_none() && player.current_frame().is_some() {
            let center = Point::new(size.width / 2.0, size.height / 2.0);
            let r = (size.width.min(size.height) * 0.09).clamp(14.0, 34.0);
            let alpha = if state.hover { 0.85 } else { 0.55 };
            frame.fill(
                &Path::circle(center, r),
                Color {
                    a: alpha * 0.6,
                    ..Color::BLACK
                },
            );
            frame.stroke(
                &Path::circle(center, r),
                Stroke::default()
                    .with_color(Color { a: alpha, ..Color::WHITE })
                    .with_width(1.5),
            );
            let glyph = if player.has_ended() {
                // ↺ replay: draw a small arc-ish triangle combo is overkill;
                // use a triangle too — the ended state is shown in the caption.
                triangle(center, r * 0.55)
            } else {
                triangle(center, r * 0.55)
            };
            frame.fill(&glyph, Color { a: alpha, ..Color::WHITE });
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &SurfaceState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.position_in(bounds).is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

fn triangle(center: Point, r: f32) -> Path {
    Path::new(|b| {
        b.move_to(Point::new(center.x - r * 0.75, center.y - r));
        b.line_to(Point::new(center.x + r * 1.05, center.y));
        b.line_to(Point::new(center.x - r * 0.75, center.y + r));
        b.close();
    })
}

fn draw_spinner(frame: &mut Frame, size: Size, now: Instant) {
    let center = Point::new(size.width / 2.0, size.height / 2.0);
    let r = 14.0;
    let t = (now.elapsed().as_millis() % 1000) as f32 / 1000.0;
    // Instant::elapsed of a "now" is ~0; use a monotonic phase from the
    // sub-second part of the wall clock instead.
    let phase = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_millis() % 1000) as f32 / 1000.0)
        .unwrap_or(t);
    let start = phase * std::f32::consts::TAU;
    let arc = Path::new(|b| {
        b.arc(canvas::path::Arc {
            center,
            radius: r,
            start_angle: iced::Radians(start),
            end_angle: iced::Radians(start + std::f32::consts::PI * 1.4),
        });
    });
    frame.fill(&Path::circle(center, r + 8.0), Color { a: 0.45, ..Color::BLACK });
    frame.stroke(
        &arc,
        Stroke::default()
            .with_color(theme::ACCENT_BLUE)
            .with_width(3.0),
    );
}

fn draw_banner(frame: &mut Frame, size: Size, message: &str, color: Color) {
    let h = 34.0;
    frame.fill_rectangle(
        Point::new(0.0, size.height - h),
        Size::new(size.width, h),
        Color { a: 0.85, ..theme::BG_HEADER },
    );
    frame.fill_rectangle(
        Point::new(0.0, size.height - h),
        Size::new(3.0, h),
        color,
    );
    frame.fill_text(Text {
        content: message.to_string(),
        position: Point::new(12.0, size.height - h / 2.0),
        color: theme::TEXT_PRIMARY,
        size: iced::Pixels(11.0),
        max_width: size.width - 20.0,
        align_x: iced::advanced::text::Alignment::Left,
        align_y: alignment::Vertical::Center,
        ..Text::default()
    });
}

pub fn video_surface<'a, Message: Clone + 'a>(
    spec: VideoSurfaceSpec<'a, Message>,
) -> Element<'a, Message> {
    canvas::Canvas::new(VideoSurfaceProgram { spec })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterbox_preserves_aspect_and_centres() {
        let r = letterbox(Size::new(400.0, 400.0), 1920, 1080);
        assert_eq!(r.width, 400.0);
        assert_eq!(r.height, 225.0);
        assert_eq!(r.x, 0.0);
        assert_eq!(r.y, 87.0);
        let r = letterbox(Size::new(400.0, 400.0), 1080, 1920);
        assert_eq!(r.height, 400.0);
        assert_eq!(r.width, 225.0);
    }

    #[test]
    fn letterbox_of_unknown_size_fills_bounds() {
        let r = letterbox(Size::new(50.0, 20.0), 0, 0);
        assert_eq!(r.size(), Size::new(50.0, 20.0));
    }
}

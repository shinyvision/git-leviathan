use std::time::{Duration, Instant};

use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{layout, renderer, Clipboard, Layout, Renderer as _, Shell, Widget};
use iced::widget::canvas::{self, path::Arc, Frame, Path, Stroke};
use iced::{
    mouse, window, Color, Element, Event, Length, Point, Radians, Rectangle, Renderer, Size, Theme,
    Vector,
};

const SPIN_PERIOD_SECS: f32 = 0.9;
const SPIN_PERIOD_MS: u128 = 900;
const SPINNER_FRAME_INTERVAL: Duration = Duration::from_millis(50);
const ARC_SWEEP_FRAC: f32 = 0.75;
const STROKE_WIDTH: f32 = 1.6;

#[derive(Debug, Clone, Copy, Default)]
struct SpinnerState {
    last_redraw: Option<Instant>,
}

pub struct Spinner {
    started_at: Instant,
    color: Color,
    size: f32,
}

impl Spinner {
    pub fn new(started_at: Instant, color: Color, size: f32) -> Self {
        Self {
            started_at,
            color,
            size,
        }
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Spinner {
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.size),
            height: Length::Fixed(self.size),
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<SpinnerState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(SpinnerState::default())
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return;
        };
        if !is_visible(&layout.bounds(), viewport) {
            return;
        }

        let state = tree.state.downcast_mut::<SpinnerState>();
        state.last_redraw = Some(*now);
        shell.request_redraw_at(next_spinner_frame_at(self.started_at, *now));
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if bounds.width < 1.0 || bounds.height < 1.0 {
            return;
        }

        let state = tree.state.downcast_ref::<SpinnerState>();
        let now = state.last_redraw.unwrap_or(self.started_at);

        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            let mut frame = Frame::new(renderer, bounds.size());
            draw_spinner(&mut frame, self.started_at, now, self.color);
            renderer.draw_geometry(frame.into_geometry());
        });
    }
}

fn draw_spinner(frame: &mut Frame<Renderer>, started_at: Instant, now: Instant, color: Color) {
    let start = spinner_start_angle(started_at, now);
    let end = start + std::f32::consts::TAU * ARC_SWEEP_FRAC;

    let cx = frame.width() / 2.0;
    let cy = frame.height() / 2.0;
    let radius = (frame.width().min(frame.height()) / 2.0) - STROKE_WIDTH;
    if radius <= 0.0 {
        return;
    }

    let path = Path::new(|b| {
        b.arc(Arc {
            center: Point::new(cx, cy),
            radius,
            start_angle: Radians(start),
            end_angle: Radians(end),
        });
    });

    frame.stroke(
        &path,
        Stroke::default()
            .with_color(color)
            .with_width(STROKE_WIDTH)
            .with_line_cap(canvas::LineCap::Round),
    );
}

fn is_visible(bounds: &Rectangle, viewport: &Rectangle) -> bool {
    bounds.width > 0.0 && bounds.height > 0.0 && bounds.intersects(viewport)
}

fn elapsed_ms(started_at: Instant, now: Instant) -> u128 {
    now.saturating_duration_since(started_at).as_millis()
}

fn quantized_elapsed_ms(started_at: Instant, now: Instant) -> u128 {
    let frame_ms = SPINNER_FRAME_INTERVAL.as_millis().max(1);
    (elapsed_ms(started_at, now) / frame_ms) * frame_ms
}

fn quantized_phase_ms(started_at: Instant, now: Instant) -> u128 {
    quantized_elapsed_ms(started_at, now) % SPIN_PERIOD_MS
}

fn spinner_start_angle(started_at: Instant, now: Instant) -> f32 {
    let phase_ms = quantized_phase_ms(started_at, now) as f32;
    (phase_ms / (SPIN_PERIOD_SECS * 1000.0)) * std::f32::consts::TAU
}

fn duration_from_millis_saturating(ms: u128) -> Duration {
    Duration::from_millis(ms.min(u64::MAX as u128) as u64)
}

fn next_spinner_frame_at(started_at: Instant, now: Instant) -> Instant {
    let frame_ms = SPINNER_FRAME_INTERVAL.as_millis().max(1);
    let next_frame_ms = ((elapsed_ms(started_at, now) / frame_ms) + 1) * frame_ms;
    started_at + duration_from_millis_saturating(next_frame_ms)
}

pub fn spinner<'a, Message: 'a>(
    started_at: Instant,
    color: Color,
    size: f32,
) -> Element<'a, Message> {
    Spinner::new(started_at, color, size).into()
}

impl<'a, Message: 'a> From<Spinner> for Element<'a, Message> {
    fn from(spinner: Spinner) -> Self {
        Element::new(spinner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn displayed_phase_is_quantized_to_spinner_interval() {
        let start = Instant::now();

        assert_eq!(quantized_phase_ms(start, at(start, 0)), 0);
        assert_eq!(quantized_phase_ms(start, at(start, 49)), 0);
        assert_eq!(quantized_phase_ms(start, at(start, 50)), 50);
        assert_eq!(quantized_phase_ms(start, at(start, 99)), 50);
        assert_eq!(quantized_phase_ms(start, at(start, 100)), 100);
    }

    #[test]
    fn extra_redraws_do_not_advance_angle_between_spinner_frames() {
        let start = Instant::now();
        let initial = spinner_start_angle(start, at(start, 1));

        assert_eq!(spinner_start_angle(start, at(start, 49)), initial);
        assert!(spinner_start_angle(start, at(start, 50)) > initial);
    }

    #[test]
    fn next_redraw_targets_next_spinner_frame_boundary() {
        let start = Instant::now();

        assert_eq!(next_spinner_frame_at(start, at(start, 0)), at(start, 50));
        assert_eq!(next_spinner_frame_at(start, at(start, 1)), at(start, 50));
        assert_eq!(next_spinner_frame_at(start, at(start, 49)), at(start, 50));
        assert_eq!(next_spinner_frame_at(start, at(start, 50)), at(start, 100));
        assert_eq!(next_spinner_frame_at(start, at(start, 899)), at(start, 900));
        assert_eq!(next_spinner_frame_at(start, at(start, 900)), at(start, 950));
    }

    #[test]
    fn phase_wraps_after_spin_period() {
        let start = Instant::now();

        assert_eq!(quantized_phase_ms(start, at(start, 849)), 800);
        assert_eq!(quantized_phase_ms(start, at(start, 899)), 850);
        assert_eq!(quantized_phase_ms(start, at(start, 900)), 0);
        assert_eq!(quantized_phase_ms(start, at(start, 949)), 0);
        assert_eq!(quantized_phase_ms(start, at(start, 950)), 50);
    }
}

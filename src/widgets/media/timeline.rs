//! Scrubbable timeline shared by the audio and video players (and animated
//! images). Draws the played/unplayed track, an optional waveform, the
//! playhead, hover time and the current/total timecodes — all read live
//! from a `TransportSource` on each redraw, so playback progress never has
//! to round-trip through app messages.

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Program, Stroke, Text};
use iced::{alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::services::media::audio::Waveform;
use crate::services::media::format_timecode;
use crate::theme;

/// Minimum interval between seek messages while scrubbing. Video seeks
/// restart FFmpeg, so a drag must not flood it.
const SCRUB_THROTTLE: Duration = Duration::from_millis(90);
const TRACK_PAD_X: f32 = 8.0;
const TIME_LABEL_WIDTH: f32 = 128.0;

/// Live playback state the timeline reads on each redraw.
pub trait TransportSource: Send + Sync {
    fn position_secs(&self) -> f64;
    fn duration_secs(&self) -> f64;
    fn is_playing(&self) -> bool;
    /// Playing but waiting on data (video decoder catching up).
    fn is_buffering(&self) -> bool {
        false
    }
    /// Seconds of content decoded ahead of the playhead, if streaming.
    fn buffered_until_secs(&self) -> Option<f64> {
        None
    }
    /// Frame-accurate stepping hint (seconds per frame) for the hover label.
    fn frame_interval_secs(&self) -> Option<f64> {
        None
    }
}

#[derive(Debug, Clone)]
pub enum TimelineEvent {
    Seek(f64),
    /// Playback reached the end (or looped) — lets the app refresh play/pause
    /// icons.
    PlaybackStateChanged,
}

pub struct TimelineSpec<'a, Message> {
    pub source: Arc<dyn TransportSource>,
    pub waveform: Option<Arc<Waveform>>,
    /// Height of the canvas; waveforms want more room than a plain scrubber.
    pub height: f32,
    pub accent: Color,
    pub on_event: Box<dyn Fn(TimelineEvent) -> Message + 'a>,
}

#[derive(Debug, Default)]
pub struct TimelineState {
    scrubbing: bool,
    last_seek_sent: Option<Instant>,
    pending_seek: Option<f64>,
    hover: Option<Point>,
    last_playing: Option<bool>,
    last_redraw: Option<Instant>,
}

struct TimelineProgram<'a, Message> {
    spec: TimelineSpec<'a, Message>,
}

fn track_rect(size: Size, with_waveform: bool) -> Rectangle {
    let label_h = 18.0;
    let top = if with_waveform { 4.0 } else { 6.0 };
    Rectangle::new(
        Point::new(TRACK_PAD_X, top),
        Size::new(
            (size.width - TRACK_PAD_X * 2.0).max(1.0),
            (size.height - top - label_h).max(4.0),
        ),
    )
}

fn secs_at(x: f32, track: Rectangle, duration: f64) -> f64 {
    let t = ((x - track.x) / track.width.max(1.0)).clamp(0.0, 1.0) as f64;
    t * duration
}

impl<'a, Message: Clone + 'a> Program<Message> for TimelineProgram<'a, Message> {
    type State = TimelineState;

    fn update(
        &self,
        state: &mut TimelineState,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let emit = |e: TimelineEvent| (self.spec.on_event)(e);
        let size = bounds.size();
        let track = track_rect(size, self.spec.waveform.is_some());
        let duration = self.spec.source.duration_secs();

        match event {
            canvas::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                state.last_redraw = Some(*now);
                let playing = self.spec.source.is_playing();
                if state.last_playing.is_some_and(|p| p != playing) {
                    state.last_playing = Some(playing);
                    return Some(canvas::Action::publish(emit(TimelineEvent::PlaybackStateChanged)));
                }
                state.last_playing = Some(playing);
                // Throttled seek flush while scrubbing.
                if state.scrubbing {
                    if let Some(target) = state.pending_seek {
                        let due = state
                            .last_seek_sent
                            .is_none_or(|t| now.duration_since(t) >= SCRUB_THROTTLE);
                        if due {
                            state.pending_seek = None;
                            state.last_seek_sent = Some(*now);
                            return Some(canvas::Action::publish(emit(TimelineEvent::Seek(target))));
                        }
                    }
                    return Some(canvas::Action::request_redraw_at(*now + Duration::from_millis(16)));
                }
                if playing || self.spec.source.is_buffering() {
                    Some(canvas::Action::request_redraw_at(*now + Duration::from_millis(33)))
                } else {
                    None
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                if pos.y > track.y + track.height + 4.0 || duration <= 0.0 {
                    return None;
                }
                state.scrubbing = true;
                state.pending_seek = None;
                state.last_seek_sent = Some(Instant::now());
                let target = secs_at(pos.x, track, duration);
                Some(canvas::Action::publish(emit(TimelineEvent::Seek(target))).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                state.hover = cursor.position_in(bounds);
                if state.scrubbing {
                    let pos = cursor
                        .position()
                        .map(|p| Point::new(p.x - bounds.x, p.y - bounds.y))?;
                    state.pending_seek = Some(secs_at(pos.x, track, duration));
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                Some(canvas::Action::request_redraw())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !state.scrubbing {
                    return None;
                }
                state.scrubbing = false;
                let action = match state.pending_seek.take() {
                    Some(t) => canvas::Action::publish(emit(TimelineEvent::Seek(t))),
                    None => canvas::Action::request_redraw(),
                };
                Some(action.and_capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorLeft) => {
                state.hover = None;
                Some(canvas::Action::request_redraw())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &TimelineState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let size = bounds.size();
        let mut frame = Frame::new(renderer, size);
        let with_waveform = self.spec.waveform.is_some();
        let track = track_rect(size, with_waveform);
        let duration = self.spec.source.duration_secs().max(0.0);
        let position = if state.scrubbing {
            state
                .pending_seek
                .unwrap_or_else(|| self.spec.source.position_secs())
        } else {
            self.spec.source.position_secs()
        }
        .clamp(0.0, duration.max(0.0));
        let progress = if duration > 0.0 {
            (position / duration) as f32
        } else {
            0.0
        };
        let accent = self.spec.accent;
        let played_x = track.x + track.width * progress;

        // Track background.
        frame.fill(
            &Path::rounded_rectangle(track.position(), track.size(), 3.0.into()),
            Color::from_rgb8(0x14, 0x15, 0x1f),
        );

        // Buffered region (video streaming).
        if let Some(buffered) = self.spec.source.buffered_until_secs() {
            if duration > 0.0 {
                let end = (buffered / duration).clamp(0.0, 1.0) as f32;
                if end > progress {
                    frame.fill_rectangle(
                        Point::new(played_x, track.y),
                        Size::new(track.width * (end - progress), track.height),
                        Color::from_rgb8(0x22, 0x24, 0x33),
                    );
                }
            }
        }

        match &self.spec.waveform {
            Some(waveform) => draw_waveform(&mut frame, waveform, track, played_x, accent),
            None => {
                // Plain progress bar in the vertical middle.
                let bar_h = 6.0;
                let y = track.y + (track.height - bar_h) / 2.0;
                frame.fill(
                    &Path::rounded_rectangle(
                        Point::new(track.x, y),
                        Size::new(track.width, bar_h),
                        3.0.into(),
                    ),
                    Color::from_rgb8(0x2c, 0x2f, 0x40),
                );
                if progress > 0.0 {
                    frame.fill(
                        &Path::rounded_rectangle(
                            Point::new(track.x, y),
                            Size::new((track.width * progress).max(bar_h), bar_h),
                            3.0.into(),
                        ),
                        accent,
                    );
                }
            }
        }

        // Playhead.
        frame.stroke(
            &Path::line(
                Point::new(played_x, track.y),
                Point::new(played_x, track.y + track.height),
            ),
            Stroke::default().with_color(Color::WHITE).with_width(1.5),
        );
        let knob_y = if with_waveform {
            track.y + track.height
        } else {
            track.y + track.height / 2.0
        };
        frame.fill(&Path::circle(Point::new(played_x, knob_y), 5.0), Color::WHITE);
        frame.fill(&Path::circle(Point::new(played_x, knob_y), 3.0), accent);

        // Hover time marker.
        if let Some(hover) = state.hover {
            if duration > 0.0 && hover.y <= track.y + track.height + 8.0 {
                let x = hover.x.clamp(track.x, track.x + track.width);
                frame.stroke(
                    &Path::line(Point::new(x, track.y), Point::new(x, track.y + track.height)),
                    Stroke::default()
                        .with_color(Color { a: 0.5, ..Color::WHITE })
                        .with_width(1.0),
                );
                let secs = secs_at(x, track, duration);
                let mut label = format_timecode(secs);
                if let Some(interval) = self.spec.source.frame_interval_secs() {
                    if interval > 0.0 {
                        label = format!("{label}  f{}", (secs / interval).floor() as u64);
                    }
                }
                let label_w = 8.0 + label.len() as f32 * 6.6;
                let lx = (x - label_w / 2.0).clamp(0.0, (size.width - label_w).max(0.0));
                let ly = (track.y - 18.0).max(0.0);
                frame.fill(
                    &Path::rounded_rectangle(Point::new(lx, ly), Size::new(label_w, 16.0), 3.0.into()),
                    Color { a: 0.92, ..theme::BG_HEADER },
                );
                frame.fill_text(Text {
                    content: label,
                    position: Point::new(lx + label_w / 2.0, ly + 8.0),
                    color: theme::TEXT_PRIMARY,
                    size: iced::Pixels(10.0),
                    font: theme::MONO,
                    align_x: iced::advanced::text::Alignment::Center,
                    align_y: alignment::Vertical::Center,
                    ..Text::default()
                });
            }
        }

        // Timecodes.
        let label_y = track.y + track.height + 9.0;
        frame.fill_text(Text {
            content: format_timecode(position),
            position: Point::new(track.x, label_y),
            color: theme::TEXT_PRIMARY,
            size: iced::Pixels(10.0),
            font: theme::MONO,
            align_x: iced::advanced::text::Alignment::Left,
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
        let status = if self.spec.source.is_buffering() {
            "buffering…".to_string()
        } else {
            String::new()
        };
        if !status.is_empty() {
            frame.fill_text(Text {
                content: status,
                position: Point::new(track.x + track.width / 2.0, label_y),
                color: theme::TEXT_DIM,
                size: iced::Pixels(10.0),
                font: theme::MONO,
                align_x: iced::advanced::text::Alignment::Center,
                align_y: alignment::Vertical::Center,
                ..Text::default()
            });
        }
        frame.fill_text(Text {
            content: format_timecode(duration),
            position: Point::new(track.x + track.width, label_y),
            color: theme::TEXT_SECONDARY,
            size: iced::Pixels(10.0),
            font: theme::MONO,
            align_x: iced::advanced::text::Alignment::Right,
            align_y: alignment::Vertical::Center,
            ..Text::default()
        });
        let _ = TIME_LABEL_WIDTH;

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &TimelineState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.scrubbing {
            return mouse::Interaction::Grabbing;
        }
        let track = track_rect(bounds.size(), self.spec.waveform.is_some());
        match cursor.position_in(bounds) {
            Some(p) if p.y <= track.y + track.height + 4.0 => mouse::Interaction::Pointer,
            _ => mouse::Interaction::default(),
        }
    }
}

fn draw_waveform(frame: &mut Frame, waveform: &Waveform, track: Rectangle, played_x: f32, accent: Color) {
    let channels = waveform.channels.len().max(1) as f32;
    let lane_h = track.height / channels;
    let gain = if waveform.peak > 0.0 {
        (0.95 / waveform.peak).min(4.0)
    } else {
        1.0
    };
    let px_count = track.width.max(1.0) as usize;
    let unplayed_peak = Color::from_rgb8(0x3a, 0x3e, 0x55);
    let unplayed_rms = Color::from_rgb8(0x50, 0x55, 0x74);
    let played_peak = Color { a: 0.55, ..accent };
    let played_rms = accent;

    for (ch, buckets) in waveform.channels.iter().enumerate() {
        if buckets.is_empty() {
            continue;
        }
        let lane_top = track.y + ch as f32 * lane_h;
        let mid = lane_top + lane_h / 2.0;
        let half = (lane_h / 2.0 - 1.0).max(1.0);
        let per_px = buckets.len() as f32 / px_count as f32;
        // Build filled polygons: one for peaks, one for RMS, split at the
        // playhead so the played portion takes the accent colour.
        let mut peaks_played = canvas::path::Builder::new();
        let mut peaks_rest = canvas::path::Builder::new();
        let mut rms_played = canvas::path::Builder::new();
        let mut rms_rest = canvas::path::Builder::new();
        for px in 0..px_count {
            let start = (px as f32 * per_px) as usize;
            let end = (((px + 1) as f32 * per_px) as usize).max(start + 1).min(buckets.len());
            let mut min = 0.0f32;
            let mut max = 0.0f32;
            let mut rms = 0.0f32;
            for b in &buckets[start..end] {
                min = min.min(b.min);
                max = max.max(b.max);
                rms = rms.max(b.rms);
            }
            let x = track.x + px as f32 + 0.5;
            let y_top = mid - (max * gain).clamp(0.0, 1.0) * half;
            let y_bot = mid - (min * gain).clamp(-1.0, 0.0) * half;
            let r = (rms * gain).clamp(0.0, 1.0) * half;
            let (peaks, rms_b) = if x <= played_x {
                (&mut peaks_played, &mut rms_played)
            } else {
                (&mut peaks_rest, &mut rms_rest)
            };
            peaks.move_to(Point::new(x, y_top.min(mid - 0.5)));
            peaks.line_to(Point::new(x, y_bot.max(mid + 0.5)));
            rms_b.move_to(Point::new(x, mid - r.max(0.5)));
            rms_b.line_to(Point::new(x, mid + r.max(0.5)));
        }
        let stroke = |c: Color| Stroke::default().with_color(c).with_width(1.0);
        frame.stroke(&peaks_rest.build(), stroke(unplayed_peak));
        frame.stroke(&rms_rest.build(), stroke(unplayed_rms));
        frame.stroke(&peaks_played.build(), stroke(played_peak));
        frame.stroke(&rms_played.build(), stroke(played_rms));
        // Centre line.
        frame.stroke(
            &Path::line(Point::new(track.x, mid), Point::new(track.x + track.width, mid)),
            Stroke::default()
                .with_color(Color { a: 0.35, ..unplayed_rms })
                .with_width(1.0),
        );
    }
}

pub fn timeline<'a, Message: Clone + 'a>(spec: TimelineSpec<'a, Message>) -> Element<'a, Message> {
    let height = spec.height;
    canvas::Canvas::new(TimelineProgram { spec })
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secs_at_clamps_to_track() {
        let track = Rectangle::new(Point::new(10.0, 0.0), Size::new(100.0, 10.0));
        assert_eq!(secs_at(-50.0, track, 20.0), 0.0);
        assert_eq!(secs_at(60.0, track, 20.0), 10.0);
        assert_eq!(secs_at(500.0, track, 20.0), 20.0);
    }

    #[test]
    fn track_rect_leaves_room_for_labels() {
        let r = track_rect(Size::new(300.0, 60.0), false);
        assert!(r.y + r.height < 60.0 - 10.0);
        let w = track_rect(Size::new(300.0, 120.0), true);
        assert!(w.height > r.height);
    }
}

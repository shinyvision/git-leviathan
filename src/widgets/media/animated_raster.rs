//! Animated raster image playback shared by built-in UI and plugin widgets.
//!
//! The widget schedules redraws at the source frame boundaries via Iced's
//! `RedrawRequest::At`, so animation timing does not depend on app-level
//! subscriptions like fetch spinners or plugin runtime polling.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use iced::advanced::layout;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::widget::image::{self, FilterMethod, Handle};
use iced::{
    border, mouse, window, ContentFit, Element, Event, Length, Rectangle, Renderer, Rotation, Size,
    Theme,
};

#[derive(Clone)]
struct AnimatedFrame {
    handle: Handle,
    delay_ms: u32,
}

#[derive(Clone)]
pub struct AnimatedRaster {
    frames: Vec<AnimatedFrame>,
    total_ms: u32,
}

impl AnimatedRaster {
    pub fn from_gif_path(path: &Path) -> Option<Self> {
        use ::image::codecs::gif::GifDecoder;
        use ::image::AnimationDecoder;

        let file = File::open(path).ok()?;
        let decoder = GifDecoder::new(BufReader::new(file)).ok()?;
        let frames = decoder.into_frames().collect_frames().ok()?;

        let mut out = Vec::with_capacity(frames.len());
        let mut total_ms: u32 = 0;
        for frame in frames {
            let (num, den) = frame.delay().numer_denom_ms();
            let delay_ms = (num as f64 / den.max(1) as f64).round() as u32;
            let delay_ms = delay_ms.max(10);
            let buf = frame.into_buffer();
            let (w, h) = (buf.width(), buf.height());
            let handle = Handle::from_rgba(w, h, buf.into_raw());
            out.push(AnimatedFrame { handle, delay_ms });
            total_ms = total_ms.saturating_add(delay_ms);
        }

        if out.is_empty() {
            return None;
        }

        Some(Self {
            frames: out,
            total_ms: total_ms.max(1),
        })
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn total_duration_ms(&self) -> u32 {
        self.total_ms
    }

    fn frame_at(&self, started_at: Instant, now: Instant) -> &Handle {
        &self.frames[self.frame_index_at(started_at, now)].handle
    }

    fn frame_index_at(&self, started_at: Instant, now: Instant) -> usize {
        let phase = phase_ms(started_at, now, self.total_ms);
        let mut acc = 0u32;
        for (idx, frame) in self.frames.iter().enumerate() {
            acc = acc.saturating_add(frame.delay_ms);
            if phase < acc {
                return idx;
            }
        }
        self.frames.len().saturating_sub(1)
    }

    fn next_frame_at(&self, started_at: Instant, now: Instant) -> Instant {
        let phase = phase_ms(started_at, now, self.total_ms);
        let mut acc = 0u32;
        for frame in &self.frames {
            acc = acc.saturating_add(frame.delay_ms);
            if phase < acc {
                let wait_ms = acc.saturating_sub(phase).max(1);
                return now + Duration::from_millis(wait_ms as u64);
            }
        }
        now + Duration::from_millis(1)
    }

    #[cfg(test)]
    fn from_test_delays(delays: &[u32]) -> Self {
        let frames = delays
            .iter()
            .copied()
            .map(|delay_ms| AnimatedFrame {
                handle: Handle::from_rgba(1, 1, vec![0, 0, 0, 0]),
                delay_ms,
            })
            .collect::<Vec<_>>();
        let total_ms = delays.iter().copied().sum::<u32>().max(1);
        Self { frames, total_ms }
    }
}

fn phase_ms(started_at: Instant, now: Instant, total_ms: u32) -> u32 {
    (now.saturating_duration_since(started_at).as_millis() % total_ms.max(1) as u128) as u32
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Arc<AnimatedRaster>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<AnimatedRaster>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn load_animated_raster(path: &Path) -> Option<Arc<AnimatedRaster>> {
    {
        let lock = cache().lock().ok()?;
        if let Some(existing) = lock.get(path) {
            return Some(existing.clone());
        }
    }
    let decoded = Arc::new(AnimatedRaster::from_gif_path(path)?);
    let mut lock = cache().lock().ok()?;
    Some(
        lock.entry(path.to_path_buf())
            .or_insert_with(|| decoded.clone())
            .clone(),
    )
}

#[derive(Debug, Clone, Copy, Default)]
struct PlaybackState {
    started_at: Option<Instant>,
    last_redraw: Option<Instant>,
}

pub struct AnimatedRasterImage<Message> {
    raster: Arc<AnimatedRaster>,
    width: Length,
    height: Length,
    _message: PhantomData<Message>,
}

impl<Message> AnimatedRasterImage<Message> {
    pub fn new(raster: Arc<AnimatedRaster>) -> Self {
        Self {
            raster,
            width: Length::Shrink,
            height: Length::Shrink,
            _message: PhantomData,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.width = Length::Fixed(size);
        self.height = Length::Fixed(size);
        self
    }
}

pub fn animated_raster<'a, Message: 'a>(
    raster: Arc<AnimatedRaster>,
    size: f32,
) -> Element<'a, Message> {
    AnimatedRasterImage::new(raster).size(size).into()
}

fn is_visible(bounds: &Rectangle, viewport: &Rectangle) -> bool {
    bounds.width > 0.0 && bounds.height > 0.0 && bounds.intersects(viewport)
}

impl<Message> Widget<Message, Theme, Renderer> for AnimatedRasterImage<Message> {
    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<PlaybackState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(PlaybackState::default())
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        image::layout(
            renderer,
            limits,
            &self.raster.frames[0].handle,
            self.width,
            self.height,
            None,
            ContentFit::Contain,
            Rotation::default(),
            false,
        )
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

        let state = tree.state.downcast_mut::<PlaybackState>();
        let started_at = *state.started_at.get_or_insert(*now);
        state.last_redraw = Some(*now);
        shell.request_redraw_at(self.raster.next_frame_at(started_at, *now));
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<PlaybackState>();
        let now = state.last_redraw.unwrap_or_else(Instant::now);
        let started_at = state.started_at.unwrap_or(now);
        let handle = self.raster.frame_at(started_at, now);
        image::draw(
            renderer,
            layout,
            handle,
            None,
            border::Radius::default(),
            ContentFit::Contain,
            FilterMethod::default(),
            Rotation::default(),
            1.0,
            1.0,
        );
    }
}

impl<'a, Message: 'a> From<AnimatedRasterImage<Message>> for Element<'a, Message> {
    fn from(image: AnimatedRasterImage<Message>) -> Self {
        Element::new(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn frame_index_uses_per_frame_delays() {
        let raster = AnimatedRaster::from_test_delays(&[50, 100, 150]);
        let start = Instant::now();

        assert_eq!(raster.frame_index_at(start, at(start, 0)), 0);
        assert_eq!(raster.frame_index_at(start, at(start, 49)), 0);
        assert_eq!(raster.frame_index_at(start, at(start, 50)), 1);
        assert_eq!(raster.frame_index_at(start, at(start, 149)), 1);
        assert_eq!(raster.frame_index_at(start, at(start, 150)), 2);
        assert_eq!(raster.frame_index_at(start, at(start, 299)), 2);
        assert_eq!(raster.frame_index_at(start, at(start, 300)), 0);
    }

    #[test]
    fn next_frame_at_targets_next_boundary() {
        let raster = AnimatedRaster::from_test_delays(&[50, 100, 150]);
        let start = Instant::now();

        assert_eq!(raster.next_frame_at(start, at(start, 0)), at(start, 50));
        assert_eq!(raster.next_frame_at(start, at(start, 49)), at(start, 50));
        assert_eq!(raster.next_frame_at(start, at(start, 50)), at(start, 150));
        assert_eq!(raster.next_frame_at(start, at(start, 299)), at(start, 300));
        assert_eq!(raster.next_frame_at(start, at(start, 300)), at(start, 350));
    }
}

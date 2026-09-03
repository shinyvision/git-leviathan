//! Streaming video playback backed by FFmpeg subprocesses.
//!
//! Frames arrive as raw RGBA over a pipe from `ffmpeg -f rawvideo` at a
//! constant frame rate, so frame `i` of a stream started at `t0` has
//! presentation time `t0 + i / fps`. The audio track is decoded by a second
//! `ffmpeg` into a `StreamBuffer` that the audio engine plays; when the file
//! has audio it is the master clock, otherwise a wall clock drives frames.
//! Seeking restarts both pipelines at the target time.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bytes::Bytes;
use iced::widget::image::Handle;

use super::engine::{StreamBuffer, Voice, VoiceSource};
use super::ffmpeg::{self, ChildGuard};
use super::{format_bytes, format_timecode, MediaError, MediaSource};

/// Longest side of the RGBA frames pushed through the pipe.
pub const MAX_DECODE_DIMENSION: u32 = 1920;
/// Frames buffered ahead of the playhead.
const FRAME_QUEUE_DEPTH: usize = 6;
/// Output sample rate requested from FFmpeg for the audio track.
const AUDIO_SAMPLE_RATE: u32 = 48_000;
/// If the clock is this far ahead of the newest decoded frame we consider
/// playback stalled (waiting on the decoder) and keep showing the last frame.
const MAX_FRAME_LAG_SECS: f64 = 0.75;

#[derive(Debug, Clone, Default)]
pub struct VideoInfo {
    /// Display dimensions (rotation applied).
    pub width: u32,
    pub height: u32,
    /// Dimensions of the frames actually decoded (≤ display).
    pub decode_width: u32,
    pub decode_height: u32,
    pub fps: f64,
    pub duration_secs: f64,
    pub codec: String,
    pub codec_long: String,
    pub pix_fmt: String,
    pub container: String,
    pub container_long: String,
    pub bit_rate: Option<u64>,
    pub video_bit_rate: Option<u64>,
    pub frame_count: Option<u64>,
    pub rotation: i32,
    pub has_audio: bool,
    pub audio_codec: String,
    pub audio_sample_rate: u32,
    pub audio_channels: u16,
    pub audio_bit_rate: Option<u64>,
    pub file_size: u64,
    pub tags: Vec<(String, String)>,
}

impl VideoInfo {
    pub fn frame_interval_secs(&self) -> f64 {
        if self.fps > 0.0 {
            1.0 / self.fps
        } else {
            1.0 / 30.0
        }
    }

    pub fn properties(&self) -> Vec<(String, String)> {
        let mut rows = vec![
            ("Duration".to_string(), format_timecode(self.duration_secs)),
            (
                "Dimensions".to_string(),
                format!("{} × {}", self.width, self.height),
            ),
            ("Frame rate".to_string(), format!("{:.3} fps", self.fps)),
            (
                "Video codec".to_string(),
                if self.codec_long.is_empty() {
                    self.codec.clone()
                } else {
                    format!("{} ({})", self.codec_long, self.codec)
                },
            ),
        ];
        if !self.pix_fmt.is_empty() {
            rows.push(("Pixel format".to_string(), self.pix_fmt.clone()));
        }
        if let Some(fc) = self.frame_count {
            rows.push(("Frames".to_string(), fc.to_string()));
        }
        if let Some(br) = self.video_bit_rate {
            rows.push(("Video bitrate".to_string(), format!("{} kb/s", br / 1000)));
        }
        rows.push((
            "Container".to_string(),
            if self.container_long.is_empty() {
                self.container.clone()
            } else {
                self.container_long.clone()
            },
        ));
        if let Some(br) = self.bit_rate {
            rows.push(("Total bitrate".to_string(), format!("{} kb/s", br / 1000)));
        }
        if self.has_audio {
            rows.push(("Audio codec".to_string(), self.audio_codec.clone()));
            rows.push((
                "Audio".to_string(),
                format!(
                    "{} Hz, {}",
                    self.audio_sample_rate,
                    match self.audio_channels {
                        1 => "mono".to_string(),
                        2 => "stereo".to_string(),
                        n => format!("{n} channels"),
                    }
                ),
            ));
            if let Some(br) = self.audio_bit_rate {
                rows.push(("Audio bitrate".to_string(), format!("{} kb/s", br / 1000)));
            }
        } else {
            rows.push(("Audio".to_string(), "None".to_string()));
        }
        rows.push(("File size".to_string(), format_bytes(self.file_size)));
        if self.rotation != 0 {
            rows.push((
                "Rotation".to_string(),
                format!("{}° (applied)", self.rotation),
            ));
        }
        if self.decode_width < self.width || self.decode_height < self.height {
            rows.push((
                "Preview".to_string(),
                format!("Decoded at {} × {}", self.decode_width, self.decode_height),
            ));
        }
        for (k, v) in &self.tags {
            rows.push((k.clone(), v.clone()));
        }
        rows
    }
}

pub struct VideoFrame {
    pub handle: Handle,
    pub pts: f64,
    pub width: u32,
    pub height: u32,
}

struct RawFrame {
    index: u64,
    rgba: Vec<u8>,
}

/// One running decode pipeline (video + optional audio processes).
struct Pipeline {
    // Dropped first so the reader thread's blocked `send` fails and it exits.
    rx: Receiver<RawFrame>,
    start_secs: f64,
    fps: f64,
    width: u32,
    height: u32,
    /// Frame read from the pipe but not yet due.
    pending: Option<RawFrame>,
    video_done: Arc<AtomicBool>,
    video_error: Arc<Mutex<Option<String>>>,
    audio: Option<Arc<StreamBuffer>>,
    _video_child: ChildGuard,
    _audio_child: Option<ChildGuard>,
}

impl Pipeline {
    fn pts_of(&self, index: u64) -> f64 {
        self.start_secs + index as f64 / self.fps
    }

    fn next_frame(&mut self) -> Option<RawFrame> {
        if let Some(frame) = self.pending.take() {
            return Some(frame);
        }
        match self.rx.try_recv() {
            Ok(frame) => Some(frame),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.video_done.store(true, Ordering::Release);
                None
            }
        }
    }

    fn is_exhausted(&self) -> bool {
        self.pending.is_none() && self.video_done.load(Ordering::Acquire)
    }
}

enum Clock {
    Wall {
        base_media: f64,
        started: Option<Instant>,
    },
    Audio {
        base_media: f64,
    },
}

struct Inner {
    pipeline: Option<Pipeline>,
    clock: Clock,
    /// Media time the transport reports while paused / before first frame.
    position: f64,
    /// A frame should be shown as soon as one arrives (after seek/open).
    want_poster: bool,
    /// Wall-clock moment the current pipeline was (re)started; used to
    /// report "buffering" only after a grace period.
    pipeline_started_at: Instant,
}

pub struct VideoPlayer {
    info: VideoInfo,
    path: PathBuf,
    _temp: Option<tempfile::TempPath>,
    inner: Mutex<Inner>,
    current: Mutex<Option<Arc<VideoFrame>>>,
    voice: Option<Voice>,
    playing: AtomicBool,
    ended: AtomicBool,
    looping: AtomicBool,
    muted: AtomicBool,
    volume: AtomicU32,
    rate: AtomicU32,
    frame_serial: AtomicU64,
    error: Mutex<Option<String>>,
}

impl std::fmt::Debug for VideoPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoPlayer")
            .field("path", &self.path)
            .field("playing", &self.is_playing())
            .field("position", &self.position_secs())
            .finish()
    }
}

impl VideoPlayer {
    /// Probe and prepare the player. Blob sources are spilled to a temp file
    /// so FFmpeg can seek. The first frame starts decoding immediately.
    pub fn open(source: &MediaSource, file_path: &str) -> Result<VideoPlayer, MediaError> {
        if !ffmpeg::is_available() {
            return Err(MediaError::FfmpegMissing);
        }
        let (path, temp, file_size) = match source {
            MediaSource::WorkdirFile { path, size } => (path.clone(), None, *size),
            MediaSource::Blob { bytes, .. } => {
                let ext = file_path
                    .rsplit('.')
                    .next()
                    .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
                    .unwrap_or("bin");
                let temp = tempfile::Builder::new()
                    .prefix("git-leviathan-video-")
                    .suffix(&format!(".{ext}"))
                    .tempfile()
                    .map_err(|e| MediaError::Io(e.to_string()))?;
                std::fs::write(temp.path(), bytes).map_err(|e| MediaError::Io(e.to_string()))?;
                let temp = temp.into_temp_path();
                (temp.to_path_buf(), Some(temp), bytes.len() as u64)
            }
            MediaSource::Missing => return Err(MediaError::Missing),
            MediaSource::TooLarge { bytes, max } => {
                return Err(MediaError::TooLarge {
                    bytes: *bytes,
                    max: *max,
                })
            }
        };

        let probe = ffmpeg::probe(&path)?;
        let info = build_info(&probe, file_size);
        let voice = if info.has_audio {
            Some(Voice::new(VoiceSource::Silent {
                sample_rate: AUDIO_SAMPLE_RATE,
            }))
        } else {
            None
        };

        let player = VideoPlayer {
            info,
            path,
            _temp: temp,
            inner: Mutex::new(Inner {
                pipeline: None,
                clock: Clock::Wall {
                    base_media: 0.0,
                    started: None,
                },
                position: 0.0,
                want_poster: true,
                pipeline_started_at: Instant::now(),
            }),
            current: Mutex::new(None),
            voice,
            playing: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            looping: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            volume: AtomicU32::new(1f32.to_bits()),
            rate: AtomicU32::new(1f32.to_bits()),
            frame_serial: AtomicU64::new(0),
            error: Mutex::new(None),
        };
        if player.info.width > 0 && player.info.height > 0 {
            let mut inner = player.inner.lock().unwrap_or_else(|e| e.into_inner());
            player.start_pipeline(&mut inner, 0.0);
        }
        Ok(player)
    }

    pub fn info(&self) -> &VideoInfo {
        &self.info
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Acquire)
    }

    pub fn has_ended(&self) -> bool {
        self.ended.load(Ordering::Acquire)
    }

    pub fn is_looping(&self) -> bool {
        self.looping.load(Ordering::Relaxed)
    }

    pub fn set_looping(&self, looping: bool) {
        self.looping.store(looping, Ordering::Relaxed);
        if let Some(voice) = &self.voice {
            // The voice never loops itself: the player restarts the pipeline.
            voice.set_looping(false);
        }
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
        if let Some(voice) = &self.voice {
            voice.set_muted(muted);
        }
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    pub fn set_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        self.volume.store(volume.to_bits(), Ordering::Relaxed);
        if let Some(voice) = &self.voice {
            voice.set_volume(volume);
        }
    }

    pub fn rate(&self) -> f32 {
        f32::from_bits(self.rate.load(Ordering::Relaxed))
    }

    /// Playback speed. Audio is time-stretched by FFmpeg (pitch preserved),
    /// so this restarts the pipeline at the current position.
    pub fn set_rate(&self, rate: f32) {
        let rate = rate.clamp(0.25, 4.0);
        if (rate - self.rate()).abs() < 1e-3 {
            return;
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let position = self.media_time(&inner);
        self.rate.store(rate.to_bits(), Ordering::Relaxed);
        inner.position = position;
        self.start_pipeline(&mut inner, position);
        if self.is_playing() {
            self.resume_clock(&mut inner);
        }
    }

    pub fn duration_secs(&self) -> f64 {
        self.info.duration_secs
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn current_frame(&self) -> Option<Arc<VideoFrame>> {
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Monotonic counter bumped whenever the displayed frame changes.
    pub fn frame_serial(&self) -> u64 {
        self.frame_serial.load(Ordering::Acquire)
    }

    pub fn position_secs(&self) -> f64 {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_playing() {
            self.media_time(&inner)
                .min(self.info.duration_secs.max(0.0))
        } else {
            inner.position
        }
    }

    /// True while playing but the decoder hasn't caught up (after a seek,
    /// or on a slow machine).
    pub fn is_buffering(&self) -> bool {
        if !self.is_playing() {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            return inner.want_poster
                && inner.pipeline_started_at.elapsed().as_millis() > 150
                && self.current_frame().is_none();
        }
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.pipeline_started_at.elapsed().as_millis() < 150 {
            return false;
        }
        let starved_audio = self.voice.as_ref().is_some_and(|v| v.is_starved());
        let media_time = self.media_time(&inner);
        let newest = self
            .current_frame()
            .map(|f| f.pts)
            .unwrap_or(f64::NEG_INFINITY);
        starved_audio || media_time - newest > MAX_FRAME_LAG_SECS
    }

    pub fn play(&self) {
        if self.info.width == 0 {
            return;
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if self.has_ended() || inner.position >= self.info.duration_secs - 1e-3 {
            inner.position = 0.0;
            self.ended.store(false, Ordering::Release);
            self.start_pipeline(&mut inner, 0.0);
        } else if inner
            .pipeline
            .as_ref()
            .is_none_or(|p| p.is_exhausted())
        {
            let position = inner.position;
            self.start_pipeline(&mut inner, position);
        }
        self.playing.store(true, Ordering::Release);
        self.resume_clock(&mut inner);
    }

    pub fn pause(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_playing() {
            inner.position = self.media_time(&inner).min(self.info.duration_secs);
        }
        self.playing.store(false, Ordering::Release);
        if let Clock::Wall { base_media, started } = &mut inner.clock {
            if let Some(start) = started.take() {
                *base_media += start.elapsed().as_secs_f64() * self.rate() as f64;
            }
        }
        if let Some(voice) = &self.voice {
            voice.pause();
        }
    }

    pub fn toggle(&self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&self, secs: f64) {
        let target = secs.clamp(0.0, self.info.duration_secs.max(0.0));
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.position = target;
        self.ended.store(false, Ordering::Release);
        self.start_pipeline(&mut inner, target);
        if self.is_playing() {
            self.resume_clock(&mut inner);
        }
    }

    pub fn seek_relative(&self, delta: f64) {
        let position = self.position_secs();
        self.seek(position + delta);
    }

    /// Step one frame while paused. Forward pulls the next decoded frame;
    /// backward re-seeks one frame interval earlier.
    pub fn step_frame(&self, delta: i32) {
        if self.is_playing() {
            self.pause();
        }
        if delta == 0 {
            return;
        }
        let interval = self.info.frame_interval_secs();
        if delta < 0 {
            let position = self.position_secs();
            self.seek((position - interval * delta.unsigned_abs() as f64).max(0.0));
            return;
        }
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for _ in 0..delta {
            if inner
                .pipeline
                .as_ref()
                .is_none_or(|p| p.is_exhausted())
            {
                let position = inner.position + interval;
                if position >= self.info.duration_secs {
                    break;
                }
                self.start_pipeline(&mut inner, position);
                inner.want_poster = true;
                break;
            }
            let Some(pipeline) = inner.pipeline.as_mut() else {
                break;
            };
            // Block briefly for the next frame so stepping feels immediate.
            let frame = pipeline.pending.take().or_else(|| {
                pipeline
                    .rx
                    .recv_timeout(std::time::Duration::from_millis(250))
                    .ok()
            });
            let Some(frame) = frame else {
                break;
            };
            let pts = pipeline.pts_of(frame.index);
            let (w, h) = (pipeline.width, pipeline.height);
            self.publish_frame(frame, pts, w, h);
            inner.position = pts;
            inner.want_poster = false;
        }
    }

    /// Advance playback to `now`. Returns `true` when the displayed frame
    /// changed (so the caller can redraw).
    pub fn advance(&self, now: Instant) -> bool {
        let _ = now;
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut changed = false;

        // Surface pipeline failures (bad codec etc.) once.
        if let Some(pipeline) = inner.pipeline.as_ref() {
            if let Some(err) = pipeline
                .video_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
            {
                let mut slot = self.error.lock().unwrap_or_else(|e| e.into_inner());
                if slot.is_none() {
                    *slot = Some(err);
                }
            }
        }

        if !self.is_playing() {
            if inner.want_poster {
                if let Some(pipeline) = inner.pipeline.as_mut() {
                    if let Some(frame) = pipeline.next_frame() {
                        let pts = pipeline.pts_of(frame.index);
                        let (w, h) = (pipeline.width, pipeline.height);
                        self.publish_frame(frame, pts, w, h);
                        inner.want_poster = false;
                        changed = true;
                    }
                }
            }
            return changed;
        }

        // Audio track shorter than the video: fall over to the wall clock.
        if let (Clock::Audio { base_media }, Some(voice)) = (&inner.clock, &self.voice) {
            if voice.has_ended() {
                let media = *base_media + voice.position_secs() * self.rate() as f64;
                inner.clock = Clock::Wall {
                    base_media: media,
                    started: Some(Instant::now()),
                };
            }
        }

        let media_time = self.media_time(&inner);
        let half_frame = self.info.frame_interval_secs() * 0.5;
        let mut exhausted = false;
        let mut published = false;
        if let Some(pipeline) = inner.pipeline.as_mut() {
            let mut latest: Option<RawFrame> = None;
            let mut drained = 0;
            while drained < 64 {
                let Some(frame) = pipeline.next_frame() else {
                    break;
                };
                if pipeline.pts_of(frame.index) <= media_time + half_frame {
                    latest = Some(frame);
                    drained += 1;
                } else {
                    pipeline.pending = Some(frame);
                    break;
                }
            }
            if let Some(frame) = latest {
                let pts = pipeline.pts_of(frame.index);
                let (w, h) = (pipeline.width, pipeline.height);
                self.publish_frame(frame, pts, w, h);
                published = true;
                changed = true;
            }
            exhausted = pipeline.is_exhausted();
        }
        if published {
            inner.want_poster = false;
        }

        let duration = self.info.duration_secs;
        let audio_done = self.voice.as_ref().is_none_or(|v| v.has_ended() || !v.is_playing());
        let reached_end =
            exhausted && (audio_done || media_time >= duration - half_frame) && inner.pipeline.is_some();
        if reached_end || (duration > 0.0 && media_time >= duration + 0.25) {
            if self.is_looping() {
                inner.position = 0.0;
                self.start_pipeline(&mut inner, 0.0);
                self.resume_clock(&mut inner);
            } else {
                self.playing.store(false, Ordering::Release);
                self.ended.store(true, Ordering::Release);
                inner.position = duration;
                if let Clock::Wall { started, .. } = &mut inner.clock {
                    *started = None;
                }
                if let Some(voice) = &self.voice {
                    voice.pause();
                }
            }
            changed = true;
        }
        changed
    }

    fn media_time(&self, inner: &Inner) -> f64 {
        let rate = self.rate() as f64;
        match &inner.clock {
            Clock::Wall {
                base_media,
                started,
            } => match started {
                Some(start) if self.is_playing() => {
                    base_media + start.elapsed().as_secs_f64() * rate
                }
                _ => *base_media,
            },
            Clock::Audio { base_media } => match &self.voice {
                Some(voice) => base_media + voice.position_secs() * rate,
                None => *base_media,
            },
        }
    }

    fn resume_clock(&self, inner: &mut Inner) {
        let position = inner.position;
        let use_audio = matches!(inner.clock, Clock::Audio { .. });
        match &mut inner.clock {
            Clock::Wall { base_media, started } => {
                *base_media = position;
                *started = Some(Instant::now());
            }
            Clock::Audio { base_media } => {
                *base_media = position;
            }
        }
        if let Some(voice) = &self.voice {
            if use_audio {
                voice.play();
            } else {
                voice.pause();
            }
        }
    }

    /// Restart decoding at `start_secs` (kills any running pipeline).
    fn start_pipeline(&self, inner: &mut Inner, start_secs: f64) {
        inner.pipeline = None;
        inner.want_poster = true;
        inner.pipeline_started_at = Instant::now();
        let rate = self.rate() as f64;
        let start_secs = start_secs.clamp(0.0, self.info.duration_secs.max(0.0));

        let video_child = match ffmpeg::spawn_video_frames(
            &self.path,
            start_secs,
            self.info.decode_width,
            self.info.decode_height,
            self.info.fps,
            self.info.rotation,
        ) {
            Ok(child) => child,
            Err(err) => {
                *self.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(err.to_string());
                return;
            }
        };
        let mut video_child = ChildGuard(None, video_child);
        video_child.0 = video_child.1.stderr.take();
        let stdout = video_child.1.stdout.take().expect("piped stdout");
        let (tx, rx) = std::sync::mpsc::sync_channel::<RawFrame>(FRAME_QUEUE_DEPTH);
        let done = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let frame_bytes =
            (self.info.decode_width as usize) * (self.info.decode_height as usize) * 4;
        {
            let done = done.clone();
            let error = error.clone();
            let _ = std::thread::Builder::new()
                .name("leviathan-video-frames".into())
                .spawn(move || read_frames(stdout, frame_bytes, tx, done, error));
        }

        let mut audio_buffer = None;
        let mut audio_child = None;
        if self.info.has_audio && self.voice.is_some() {
            let channels: u16 = self.info.audio_channels.clamp(1, 2);
            match ffmpeg::spawn_audio_stream(
                &self.path,
                start_secs,
                AUDIO_SAMPLE_RATE,
                channels,
                rate,
            ) {
                Ok(child) => {
                    let mut child = ChildGuard(None, child);
                    child.0 = child.1.stderr.take();
                    let stdout = child.1.stdout.take().expect("piped stdout");
                    let buffer = Arc::new(StreamBuffer::new(AUDIO_SAMPLE_RATE, channels));
                    {
                        let buffer = buffer.clone();
                        let _ = std::thread::Builder::new()
                            .name("leviathan-video-audio".into())
                            .spawn(move || read_audio(stdout, buffer));
                    }
                    audio_buffer = Some(buffer);
                    audio_child = Some(child);
                }
                Err(_) => {
                    // No audio pipeline — fall back to the wall clock.
                }
            }
        }

        if let (Some(voice), Some(buffer)) = (&self.voice, &audio_buffer) {
            voice.set_source(VoiceSource::Stream(buffer.clone()));
            voice.set_volume(self.volume());
            voice.set_muted(self.is_muted());
            inner.clock = Clock::Audio {
                base_media: start_secs,
            };
        } else {
            if let Some(voice) = &self.voice {
                voice.pause();
            }
            inner.clock = Clock::Wall {
                base_media: start_secs,
                started: None,
            };
        }

        inner.pipeline = Some(Pipeline {
            rx,
            start_secs,
            fps: if self.info.fps > 0.0 {
                self.info.fps
            } else {
                30.0
            },
            width: self.info.decode_width,
            height: self.info.decode_height,
            pending: None,
            video_done: done,
            video_error: error,
            audio: audio_buffer,
            _video_child: video_child,
            _audio_child: audio_child,
        });
    }

    fn publish_frame(&self, frame: RawFrame, pts: f64, width: u32, height: u32) {
        let handle = Handle::from_rgba(width, height, Bytes::from(frame.rgba));
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(VideoFrame {
            handle,
            pts,
            width,
            height,
        }));
        self.frame_serial.fetch_add(1, Ordering::AcqRel);
    }

    /// Seconds of audio decoded ahead of the playhead (for the buffering
    /// indicator); `None` for silent video.
    pub fn audio_buffered_secs(&self) -> Option<f64> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let pipeline = inner.pipeline.as_ref()?;
        let buffer = pipeline.audio.as_ref()?;
        Some(buffer.seconds_available())
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        if let Some(voice) = &self.voice {
            voice.pause();
        }
        if let Ok(mut inner) = self.inner.lock() {
            inner.pipeline = None;
        }
    }
}

fn read_frames(
    mut stdout: std::process::ChildStdout,
    frame_bytes: usize,
    tx: SyncSender<RawFrame>,
    done: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
) {
    let mut index = 0u64;
    loop {
        let mut buf = vec![0u8; frame_bytes];
        let mut filled = 0;
        let mut eof = false;
        while filled < frame_bytes {
            match stdout.read(&mut buf[filled..]) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    if index == 0 {
                        *error.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(format!("frame read failed: {e}"));
                    }
                    eof = true;
                    break;
                }
            }
        }
        if eof {
            if index == 0 && filled == 0 {
                *error.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some("FFmpeg produced no video frames (unsupported codec?)".to_string());
            }
            break;
        }
        if tx.send(RawFrame { index, rgba: buf }).is_err() {
            break;
        }
        index += 1;
    }
    done.store(true, Ordering::Release);
}

fn read_audio(mut stdout: std::process::ChildStdout, buffer: Arc<StreamBuffer>) {
    let mut chunk = vec![0u8; 64 * 1024];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let n = match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        carry.extend_from_slice(&chunk[..n]);
        let frame_bytes = buffer.channels as usize * 2;
        let usable = carry.len() / frame_bytes * frame_bytes;
        if usable > 0 {
            let samples: Vec<i16> = carry[..usable]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            buffer.push(&samples);
            carry.drain(..usable);
        }
    }
    buffer.finish();
}

fn build_info(probe: &ffmpeg::ProbeInfo, file_size: u64) -> VideoInfo {
    let video = probe.video.clone().unwrap_or_default();
    let audio = probe.audio.clone();
    let rotation = video.rotation.rem_euclid(360);
    let (mut w, mut h) = (video.width, video.height);
    if rotation == 90 || rotation == 270 {
        std::mem::swap(&mut w, &mut h);
    }
    let (dw, dh) = decode_dimensions(w, h, MAX_DECODE_DIMENSION);
    let fps = if video.fps > 0.0 && video.fps.is_finite() {
        video.fps.min(240.0)
    } else if w > 0 {
        30.0
    } else {
        0.0
    };
    VideoInfo {
        width: w,
        height: h,
        decode_width: dw,
        decode_height: dh,
        fps,
        duration_secs: probe.duration_secs.unwrap_or(0.0).max(0.0),
        codec: video.codec.clone(),
        codec_long: video.codec_long.clone(),
        pix_fmt: video.pix_fmt.clone(),
        container: probe.container.clone(),
        container_long: probe.container_long.clone(),
        bit_rate: probe.bit_rate,
        video_bit_rate: video.bit_rate,
        frame_count: video.frame_count,
        rotation,
        has_audio: audio.is_some(),
        audio_codec: audio
            .as_ref()
            .map(|a| {
                if a.codec_long.is_empty() {
                    a.codec.clone()
                } else {
                    format!("{} ({})", a.codec_long, a.codec)
                }
            })
            .unwrap_or_default(),
        audio_sample_rate: audio.as_ref().map(|a| a.sample_rate).unwrap_or(0),
        audio_channels: audio.as_ref().map(|a| a.channels).unwrap_or(0),
        audio_bit_rate: audio.as_ref().and_then(|a| a.bit_rate),
        file_size,
        tags: probe
            .tags
            .iter()
            .filter(|(k, _)| !k.starts_with("com.") && k.len() <= 40)
            .map(|(k, v)| (k.clone(), v.chars().take(200).collect()))
            .collect(),
    }
}

fn decode_dimensions(w: u32, h: u32, max_dim: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (0, 0);
    }
    if w <= max_dim && h <= max_dim {
        return (w, h);
    }
    let scale = max_dim as f64 / w.max(h) as f64;
    let dw = ((w as f64 * scale).round() as u32).max(2);
    let dh = ((h as f64 * scale).round() as u32).max(2);
    // Even dimensions keep every swscale path happy.
    (dw & !1, dh & !1)
}

#[allow(dead_code)]
fn _assert_send_sync(p: &Path) -> &Path {
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_dimensions_cap_longest_side_and_keep_even() {
        assert_eq!(decode_dimensions(1280, 720, 1920), (1280, 720));
        assert_eq!(decode_dimensions(3840, 2160, 1920), (1920, 1080));
        assert_eq!(decode_dimensions(2160, 3840, 1920), (1080, 1920));
        let (w, h) = decode_dimensions(4001, 3001, 1920);
        assert!(w % 2 == 0 && h % 2 == 0);
        assert_eq!(decode_dimensions(0, 10, 1920), (0, 0));
    }

    #[test]
    fn build_info_applies_rotation_and_defaults() {
        let probe = ffmpeg::ProbeInfo {
            container: "mov".into(),
            duration_secs: Some(12.5),
            video: Some(ffmpeg::VideoStreamInfo {
                codec: "h264".into(),
                width: 1920,
                height: 1080,
                fps: 0.0,
                rotation: 90,
                ..Default::default()
            }),
            audio: Some(ffmpeg::AudioStreamInfo {
                codec: "aac".into(),
                sample_rate: 44100,
                channels: 6,
                ..Default::default()
            }),
            ..Default::default()
        };
        let info = build_info(&probe, 1234);
        assert_eq!((info.width, info.height), (1080, 1920));
        assert_eq!(info.fps, 30.0);
        assert!(info.has_audio);
        assert_eq!(info.audio_channels, 6);
        assert!(info
            .properties()
            .iter()
            .any(|(k, v)| k == "Rotation" && v.starts_with("90")));
        assert!(info
            .properties()
            .iter()
            .any(|(k, v)| k == "Audio" && v.contains("6 channels")));
    }

    #[test]
    fn missing_source_cannot_open() {
        let err = VideoPlayer::open(&MediaSource::Missing, "a.mp4").unwrap_err();
        assert!(matches!(err, MediaError::Missing | MediaError::FfmpegMissing));
    }
}

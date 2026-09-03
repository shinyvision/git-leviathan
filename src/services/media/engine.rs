//! Process-wide audio output. One cpal stream mixes every active `Voice`
//! (audio clips and the audio tracks of playing videos) with per-voice
//! resampling, gain ramps (no clicks on play/pause/seek), looping and
//! volume. The device stream lives on its own thread and is opened lazily
//! on first use; when no output device exists the engine reports that and
//! voices fall back to a silent wall-clock so transport UI still works.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::audio::AudioClip;

const STATE_STOPPED: u8 = 0;
const STATE_PLAYING: u8 = 1;
const STATE_STOPPING: u8 = 2;

/// Gain ramp length for play/pause transitions.
const RAMP_SECONDS: f32 = 0.006;

#[derive(Debug, Clone, PartialEq)]
pub enum EngineStatus {
    /// Not opened yet (no voice has asked for output).
    Idle,
    Starting,
    Ready {
        device: String,
        sample_rate: u32,
        channels: u16,
    },
    Unavailable(String),
}

/// A growable PCM buffer fed by a decoder thread and drained by the mixer —
/// used for the audio track of streaming video.
pub struct StreamBuffer {
    data: Mutex<Vec<i16>>,
    frames: AtomicU64,
    complete: AtomicBool,
    pub channels: u16,
    pub sample_rate: u32,
}

impl StreamBuffer {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            data: Mutex::new(Vec::new()),
            frames: AtomicU64::new(0),
            complete: AtomicBool::new(false),
            channels: channels.max(1),
            sample_rate: sample_rate.max(1),
        }
    }

    pub fn push(&self, samples: &[i16]) {
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        data.extend_from_slice(samples);
        let frames = data.len() / self.channels as usize;
        self.frames.store(frames as u64, Ordering::Release);
    }

    pub fn finish(&self) {
        self.complete.store(true, Ordering::Release);
    }

    pub fn is_complete(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }

    pub fn frames_available(&self) -> u64 {
        self.frames.load(Ordering::Acquire)
    }

    pub fn seconds_available(&self) -> f64 {
        self.frames_available() as f64 / self.sample_rate as f64
    }
}

impl std::fmt::Debug for StreamBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamBuffer")
            .field("frames", &self.frames_available())
            .field("complete", &self.is_complete())
            .finish()
    }
}

#[derive(Clone)]
pub enum VoiceSource {
    Clip(Arc<AudioClip>),
    Stream(Arc<StreamBuffer>),
    /// A voice with nothing to play yet (e.g. video before the first seek
    /// finished). Keeps the transport state alive.
    Silent { sample_rate: u32 },
}

impl VoiceSource {
    fn sample_rate(&self) -> u32 {
        match self {
            VoiceSource::Clip(c) => c.sample_rate,
            VoiceSource::Stream(s) => s.sample_rate,
            VoiceSource::Silent { sample_rate } => *sample_rate,
        }
        .max(1)
    }

    /// Total frames if known.
    fn frame_count(&self) -> Option<u64> {
        match self {
            VoiceSource::Clip(c) => Some(c.frame_count() as u64),
            VoiceSource::Stream(s) => s.is_complete().then(|| s.frames_available()),
            VoiceSource::Silent { .. } => Some(0),
        }
    }
}

struct VoiceShared {
    source: Mutex<VoiceSource>,
    /// Playhead in source frames, stored as `f64` bits.
    position: AtomicU64,
    state: AtomicU8,
    /// Current ramp gain (f32 bits), owned by the mixer callback.
    gain: AtomicU32,
    volume: AtomicU32,
    muted: AtomicBool,
    looping: AtomicBool,
    rate: AtomicU32,
    ended: AtomicBool,
    starved: AtomicBool,
    /// Silent fallback clock when no output device is available:
    /// `(started_at, position_at_start)`.
    wall: Mutex<Option<(Instant, f64)>>,
}

impl VoiceShared {
    fn position_frames(&self) -> f64 {
        f64::from_bits(self.position.load(Ordering::Acquire))
    }

    fn set_position_frames(&self, frames: f64) {
        self.position
            .store(frames.max(0.0).to_bits(), Ordering::Release);
    }

    fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    fn rate(&self) -> f32 {
        f32::from_bits(self.rate.load(Ordering::Relaxed))
    }
}

/// Handle to one playing source. Dropping it removes it from the mix.
pub struct Voice {
    shared: Arc<VoiceShared>,
}

impl Voice {
    pub fn new(source: VoiceSource) -> Voice {
        let shared = Arc::new(VoiceShared {
            source: Mutex::new(source),
            position: AtomicU64::new(0f64.to_bits()),
            state: AtomicU8::new(STATE_STOPPED),
            gain: AtomicU32::new(0f32.to_bits()),
            volume: AtomicU32::new(1f32.to_bits()),
            muted: AtomicBool::new(false),
            looping: AtomicBool::new(false),
            rate: AtomicU32::new(1f32.to_bits()),
            ended: AtomicBool::new(false),
            starved: AtomicBool::new(false),
            wall: Mutex::new(None),
        });
        engine().register(&shared);
        Voice { shared }
    }

    pub fn play(&self) {
        engine().ensure_started();
        self.shared.ended.store(false, Ordering::Release);
        if self.uses_wall_clock() {
            let pos = self.position_frames();
            *self.shared.wall.lock().unwrap_or_else(|e| e.into_inner()) =
                Some((Instant::now(), pos));
        }
        self.shared.state.store(STATE_PLAYING, Ordering::Release);
    }

    pub fn pause(&self) {
        self.freeze_wall_clock();
        let state = self.shared.state.load(Ordering::Acquire);
        if state == STATE_PLAYING {
            if engine().is_ready() {
                self.shared
                    .state
                    .store(STATE_STOPPING, Ordering::Release);
            } else {
                self.shared.state.store(STATE_STOPPED, Ordering::Release);
            }
        }
    }

    pub fn toggle(&self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.shared.state.load(Ordering::Acquire) == STATE_PLAYING
    }

    fn uses_wall_clock(&self) -> bool {
        !engine().is_ready()
    }

    fn freeze_wall_clock(&self) {
        let mut wall = self.shared.wall.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((started, start_pos)) = wall.take() {
            let rate = self.source_sample_rate() as f64 * self.shared.rate() as f64;
            let pos = start_pos + started.elapsed().as_secs_f64() * rate;
            let pos = self.clamp_to_duration(pos);
            self.shared.set_position_frames(pos);
        }
    }

    fn clamp_to_duration(&self, frames: f64) -> f64 {
        match self.duration_frames() {
            Some(total) if frames >= total as f64 => {
                if self.shared.looping.load(Ordering::Relaxed) && total > 0 {
                    frames % total as f64
                } else {
                    self.shared.ended.store(true, Ordering::Release);
                    self.shared.state.store(STATE_STOPPED, Ordering::Release);
                    total as f64
                }
            }
            _ => frames,
        }
    }

    pub fn position_frames(&self) -> f64 {
        let wall = self.shared.wall.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((started, start_pos)) = *wall {
            if self.is_playing() {
                let rate = self.source_sample_rate() as f64 * self.shared.rate() as f64;
                let pos = start_pos + started.elapsed().as_secs_f64() * rate;
                drop(wall);
                return self.clamp_to_duration(pos);
            }
        }
        self.shared.position_frames()
    }

    pub fn position_secs(&self) -> f64 {
        self.position_frames() / self.source_sample_rate() as f64
    }

    pub fn source_sample_rate(&self) -> u32 {
        self.shared
            .source
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sample_rate()
    }

    pub fn duration_frames(&self) -> Option<u64> {
        self.shared
            .source
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frame_count()
    }

    pub fn seek_secs(&self, secs: f64) {
        let frames = secs.max(0.0) * self.source_sample_rate() as f64;
        self.seek_frames(frames);
    }

    pub fn seek_frames(&self, frames: f64) {
        let frames = match self.duration_frames() {
            Some(total) => frames.min(total as f64),
            None => frames,
        };
        self.shared.ended.store(false, Ordering::Release);
        self.shared.set_position_frames(frames.max(0.0));
        let mut wall = self.shared.wall.lock().unwrap_or_else(|e| e.into_inner());
        if wall.is_some() {
            *wall = Some((Instant::now(), frames.max(0.0)));
        }
    }

    /// Swap the underlying source (video seeks restart the audio stream).
    /// Playback state is preserved; the playhead restarts at 0 frames of the
    /// new source.
    pub fn set_source(&self, source: VoiceSource) {
        {
            let mut current = self.shared.source.lock().unwrap_or_else(|e| e.into_inner());
            *current = source;
        }
        self.shared.set_position_frames(0.0);
        self.shared.ended.store(false, Ordering::Release);
        self.shared.starved.store(false, Ordering::Release);
        let mut wall = self.shared.wall.lock().unwrap_or_else(|e| e.into_inner());
        if wall.is_some() {
            *wall = Some((Instant::now(), 0.0));
        }
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Relaxed);
    }

    pub fn set_looping(&self, looping: bool) {
        self.shared.looping.store(looping, Ordering::Relaxed);
    }

    pub fn has_ended(&self) -> bool {
        // Poll the wall clock so headless playback also reports its end.
        let _ = self.position_frames();
        self.shared.ended.load(Ordering::Acquire)
    }

    /// Stream underrun: the decoder hasn't delivered the samples the playhead
    /// needs yet.
    pub fn is_starved(&self) -> bool {
        self.shared.starved.load(Ordering::Relaxed)
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        self.shared.state.store(STATE_STOPPED, Ordering::Release);
    }
}

impl std::fmt::Debug for Voice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Voice")
            .field("playing", &self.is_playing())
            .field("position_secs", &self.position_secs())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

pub struct AudioEngine {
    voices: Arc<Mutex<Vec<Weak<VoiceShared>>>>,
    status: Mutex<EngineStatus>,
    ready: AtomicBool,
    start_once: OnceLock<()>,
}

static ENGINE: OnceLock<AudioEngine> = OnceLock::new();

pub fn engine() -> &'static AudioEngine {
    ENGINE.get_or_init(|| AudioEngine {
        voices: Arc::new(Mutex::new(Vec::new())),
        status: Mutex::new(EngineStatus::Idle),
        ready: AtomicBool::new(false),
        start_once: OnceLock::new(),
    })
}

impl AudioEngine {
    fn register(&self, voice: &Arc<VoiceShared>) {
        let mut voices = self.voices.lock().unwrap_or_else(|e| e.into_inner());
        voices.retain(|w| w.strong_count() > 0);
        voices.push(Arc::downgrade(voice));
    }

    pub fn status(&self) -> EngineStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// Open the output stream (once). Safe to call from any thread; the
    /// stream itself is owned by a dedicated parked thread. Blocks until the
    /// device reports ready or unavailable so callers can immediately pick
    /// the right clock.
    pub fn ensure_started(&self) {
        self.start_once.get_or_init(|| {
            *self.status.lock().unwrap_or_else(|e| e.into_inner()) = EngineStatus::Starting;
            let voices = self.voices.clone();
            let (tx, rx) = std::sync::mpsc::channel::<Result<(String, u32, u16), String>>();
            let spawned = std::thread::Builder::new()
                .name("leviathan-audio".into())
                .spawn(move || run_output_thread(voices, tx));
            let outcome = match spawned {
                Ok(_) => rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap_or_else(|_| Err("audio device did not respond".to_string())),
                Err(e) => Err(format!("could not start audio thread: {e}")),
            };
            let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
            match outcome {
                Ok((device, sample_rate, channels)) => {
                    *status = EngineStatus::Ready {
                        device,
                        sample_rate,
                        channels,
                    };
                    self.ready.store(true, Ordering::Release);
                }
                Err(message) => {
                    *status = EngineStatus::Unavailable(message);
                    self.ready.store(false, Ordering::Release);
                }
            }
        });
    }

    fn mark_failed(&self, message: String) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) =
            EngineStatus::Unavailable(message);
        self.ready.store(false, Ordering::Release);
    }
}

fn run_output_thread(
    voices: Arc<Mutex<Vec<Weak<VoiceShared>>>>,
    ready_tx: std::sync::mpsc::Sender<Result<(String, u32, u16), String>>,
) {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        let _ = ready_tx.send(Err("no audio output device".to_string()));
        return;
    };
    let device_name = device
        .description()
        .map(|d| d.to_string())
        .unwrap_or_else(|_| "default output".to_string());
    let supported = match device.default_output_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("no usable output configuration: {e}")));
            return;
        }
    };
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.config();
    let sample_rate: u32 = config.sample_rate;
    let channels = config.channels;

    let mixer = Mixer {
        voices,
        device_rate: sample_rate.max(1),
        device_channels: channels.max(1),
        scratch: Vec::new(),
    };
    let error_callback = |err: cpal::StreamError| {
        engine().mark_failed(format!("audio stream error: {err}"));
    };
    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config, mixer, error_callback),
        cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config, mixer, error_callback),
        cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config, mixer, error_callback),
        cpal::SampleFormat::I32 => build_stream::<i32>(&device, &config, mixer, error_callback),
        cpal::SampleFormat::U8 => build_stream::<u8>(&device, &config, mixer, error_callback),
        cpal::SampleFormat::F64 => build_stream::<f64>(&device, &config, mixer, error_callback),
        other => Err(format!("unsupported output sample format {other:?}")),
    };
    let stream = match stream {
        Ok(stream) => stream,
        Err(message) => {
            let _ = ready_tx.send(Err(message));
            return;
        }
    };
    if let Err(e) = stream.play() {
        let _ = ready_tx.send(Err(format!("could not start audio stream: {e}")));
        return;
    }
    let _ = ready_tx.send(Ok((device_name, sample_rate, channels)));
    // Keep the stream alive for the life of the process.
    loop {
        std::thread::park();
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut mixer: Mixer,
    error_callback: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let frames = data.len() / mixer.device_channels as usize;
                mixer.render(frames);
                for (dst, src) in data.iter_mut().zip(mixer.scratch.iter()) {
                    *dst = T::from_sample(*src);
                }
            },
            error_callback,
            None,
        )
        .map_err(|e| format!("could not open audio output: {e}"))
}

struct Mixer {
    voices: Arc<Mutex<Vec<Weak<VoiceShared>>>>,
    device_rate: u32,
    device_channels: u16,
    scratch: Vec<f32>,
}

impl Mixer {
    fn render(&mut self, frames: usize) {
        let dc = self.device_channels as usize;
        self.scratch.clear();
        self.scratch.resize(frames * dc, 0.0);

        // Never block the audio thread: skip this buffer if the UI thread is
        // registering a voice right now.
        let Ok(voices) = self.voices.try_lock() else {
            return;
        };
        for weak in voices.iter() {
            let Some(voice) = weak.upgrade() else {
                continue;
            };
            let state = voice.state.load(Ordering::Acquire);
            let gain = f32::from_bits(voice.gain.load(Ordering::Relaxed));
            if state == STATE_STOPPED && gain <= 0.0 {
                continue;
            }
            let Ok(source) = voice.source.try_lock() else {
                continue;
            };
            mix_voice(
                &voice,
                &source,
                &mut self.scratch,
                frames,
                dc,
                self.device_rate,
            );
        }
        for s in self.scratch.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }
}

/// Fetches one interpolated frame worth of samples from a source.
enum Sampler<'a> {
    Pcm {
        samples: &'a [i16],
        channels: usize,
        frames: u64,
    },
    Silent,
}

fn mix_voice(
    voice: &VoiceShared,
    source: &VoiceSource,
    out: &mut [f32],
    frames: usize,
    dc: usize,
    device_rate: u32,
) {
    let stream_guard;
    let sampler = match source {
        VoiceSource::Clip(clip) => Sampler::Pcm {
            samples: &clip.samples,
            channels: clip.channels.max(1) as usize,
            frames: clip.frame_count() as u64,
        },
        VoiceSource::Stream(stream) => {
            let Ok(guard) = stream.data.try_lock() else {
                return;
            };
            stream_guard = guard;
            let channels = stream.channels.max(1) as usize;
            Sampler::Pcm {
                samples: &stream_guard,
                channels,
                frames: (stream_guard.len() / channels) as u64,
            }
        }
        VoiceSource::Silent { .. } => Sampler::Silent,
    };
    let stream_complete = match source {
        VoiceSource::Stream(s) => s.is_complete(),
        _ => true,
    };

    let src_rate = source.sample_rate() as f64;
    let rate = voice.rate() as f64;
    let step = src_rate * rate / device_rate.max(1) as f64;
    let ramp = 1.0 / (device_rate as f32 * RAMP_SECONDS).max(1.0);
    let target_gain = if voice.state.load(Ordering::Acquire) == STATE_PLAYING {
        1.0
    } else {
        0.0
    };
    let volume = if voice.muted.load(Ordering::Relaxed) {
        0.0
    } else {
        voice.volume()
    };
    let looping = voice.looping.load(Ordering::Relaxed);

    let mut gain = f32::from_bits(voice.gain.load(Ordering::Relaxed));
    let mut pos = voice.position_frames();
    let mut starved = false;

    let (samples, sch, total) = match sampler {
        Sampler::Pcm {
            samples,
            channels,
            frames,
        } => (samples, channels, frames),
        Sampler::Silent => (&[][..], 2usize, 0u64),
    };

    for frame in 0..frames {
        // Gain ramp toward target; when we've faded out fully, stop.
        if (gain - target_gain).abs() <= ramp {
            gain = target_gain;
        } else if gain < target_gain {
            gain += ramp;
        } else {
            gain -= ramp;
        }
        if gain <= 0.0 && target_gain <= 0.0 {
            if voice.state.load(Ordering::Acquire) == STATE_STOPPING {
                voice.state.store(STATE_STOPPED, Ordering::Release);
            }
            break;
        }
        if voice.state.load(Ordering::Acquire) != STATE_PLAYING && gain <= 0.0 {
            break;
        }

        let ipos = pos.floor();
        let idx = ipos as u64;
        if total == 0 || idx + 1 >= total {
            if !stream_complete {
                // Waiting on the decoder: hold position, output silence.
                starved = true;
                break;
            }
            if looping && total > 0 {
                pos -= total as f64;
                if pos < 0.0 {
                    pos = 0.0;
                }
                continue;
            }
            voice.ended.store(true, Ordering::Release);
            voice.state.store(STATE_STOPPED, Ordering::Release);
            pos = total as f64;
            gain = 0.0;
            break;
        }
        let t = (pos - ipos) as f32;
        let level = gain * volume;
        let mut mixed = [0.0f32; 2];
        for (c, value) in mixed.iter_mut().enumerate().take(sch.min(2)) {
            let fetch = |i: i64| -> f32 {
                let i = i.clamp(0, total as i64 - 1) as usize;
                samples[i * sch + c] as f32 / 32768.0
            };
            let i = idx as i64;
            *value = hermite(fetch(i - 1), fetch(i), fetch(i + 1), fetch(i + 2), t) * level;
        }
        if sch == 1 {
            mixed[1] = mixed[0];
        }
        let base = frame * dc;
        match dc {
            1 => out[base] += (mixed[0] + mixed[1]) * 0.5,
            _ => {
                out[base] += mixed[0];
                out[base + 1] += mixed[1];
            }
        }
        pos += step;
    }

    voice.gain.store(gain.to_bits(), Ordering::Relaxed);
    voice.starved.store(starved, Ordering::Relaxed);
    voice.set_position_frames(pos);
}

/// 4-point, 3rd-order Hermite interpolation (Catmull-Rom).
fn hermite(xm1: f32, x0: f32, x1: f32, x2: f32, t: f32) -> f32 {
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b_neg = w + a;
    (((a * t) - b_neg) * t + c) * t + x0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::media::audio::{build_waveform, AudioClip, AudioInfo};

    fn clip(frames: usize, rate: u32) -> Arc<AudioClip> {
        let samples: Vec<i16> = (0..frames * 2)
            .map(|i| if i % 2 == 0 { 10_000 } else { -10_000 })
            .collect();
        Arc::new(AudioClip {
            sample_rate: rate,
            channels: 2,
            waveform: build_waveform(&samples, 2, 16),
            samples: Arc::from(samples),
            truncated: false,
            info: AudioInfo::default(),
        })
    }

    fn shared(source: VoiceSource) -> Arc<VoiceShared> {
        Arc::new(VoiceShared {
            source: Mutex::new(source),
            position: AtomicU64::new(0f64.to_bits()),
            state: AtomicU8::new(STATE_PLAYING),
            gain: AtomicU32::new(1f32.to_bits()),
            volume: AtomicU32::new(1f32.to_bits()),
            muted: AtomicBool::new(false),
            looping: AtomicBool::new(false),
            rate: AtomicU32::new(1f32.to_bits()),
            ended: AtomicBool::new(false),
            starved: AtomicBool::new(false),
            wall: Mutex::new(None),
        })
    }

    #[test]
    fn hermite_interpolates_endpoints_exactly() {
        assert!((hermite(0.0, 1.0, 2.0, 3.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((hermite(0.0, 1.0, 2.0, 3.0, 1.0) - 2.0).abs() < 1e-6);
        assert!((hermite(0.0, 1.0, 2.0, 3.0, 0.5) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn mixing_advances_position_and_mixes_channels() {
        let voice = shared(VoiceSource::Clip(clip(1000, 48_000)));
        let source = voice.source.lock().unwrap().clone();
        let mut out = vec![0.0f32; 64 * 2];
        mix_voice(&voice, &source, &mut out, 64, 2, 48_000);
        assert!((voice.position_frames() - 64.0).abs() < 1e-6);
        assert!(out[0] > 0.2 && out[1] < -0.2);
        assert!(!voice.ended.load(Ordering::Relaxed));
    }

    #[test]
    fn mixing_resamples_by_rate_ratio() {
        let voice = shared(VoiceSource::Clip(clip(1000, 24_000)));
        let source = voice.source.lock().unwrap().clone();
        let mut out = vec![0.0f32; 100 * 2];
        mix_voice(&voice, &source, &mut out, 100, 2, 48_000);
        assert!((voice.position_frames() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn clip_end_sets_ended_and_stops() {
        let voice = shared(VoiceSource::Clip(clip(10, 48_000)));
        let source = voice.source.lock().unwrap().clone();
        let mut out = vec![0.0f32; 64 * 2];
        mix_voice(&voice, &source, &mut out, 64, 2, 48_000);
        assert!(voice.ended.load(Ordering::Relaxed));
        assert_eq!(voice.state.load(Ordering::Relaxed), STATE_STOPPED);
        assert!((voice.position_frames() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn looping_wraps_instead_of_ending() {
        let voice = shared(VoiceSource::Clip(clip(10, 48_000)));
        voice.looping.store(true, Ordering::Relaxed);
        let source = voice.source.lock().unwrap().clone();
        let mut out = vec![0.0f32; 25 * 2];
        mix_voice(&voice, &source, &mut out, 25, 2, 48_000);
        assert!(!voice.ended.load(Ordering::Relaxed));
        assert!(voice.position_frames() < 10.0);
    }

    #[test]
    fn stream_underrun_holds_position_and_flags_starved() {
        let stream = Arc::new(StreamBuffer::new(48_000, 2));
        stream.push(&[100; 2 * 8]);
        let voice = shared(VoiceSource::Stream(stream.clone()));
        let source = voice.source.lock().unwrap().clone();
        let mut out = vec![0.0f32; 64 * 2];
        mix_voice(&voice, &source, &mut out, 64, 2, 48_000);
        assert!(voice.starved.load(Ordering::Relaxed));
        assert!(voice.position_frames() <= 8.0);
        assert!(!voice.ended.load(Ordering::Relaxed));
        stream.finish();
        let source = voice.source.lock().unwrap().clone();
        mix_voice(&voice, &source, &mut out, 64, 2, 48_000);
        assert!(voice.ended.load(Ordering::Relaxed));
    }

    #[test]
    fn stopping_state_fades_out_then_stops() {
        let voice = shared(VoiceSource::Clip(clip(10_000, 48_000)));
        voice.state.store(STATE_STOPPING, Ordering::Relaxed);
        let source = voice.source.lock().unwrap().clone();
        let mut out = vec![0.0f32; 2048 * 2];
        mix_voice(&voice, &source, &mut out, 2048, 2, 48_000);
        assert_eq!(voice.state.load(Ordering::Relaxed), STATE_STOPPED);
        assert_eq!(f32::from_bits(voice.gain.load(Ordering::Relaxed)), 0.0);
        // Faded, not cut: the first sample is near full level.
        assert!(out[0].abs() > 0.25);
    }

    #[test]
    fn muted_voice_still_advances() {
        let voice = shared(VoiceSource::Clip(clip(1000, 48_000)));
        voice.muted.store(true, Ordering::Relaxed);
        let source = voice.source.lock().unwrap().clone();
        let mut out = vec![0.0f32; 32 * 2];
        mix_voice(&voice, &source, &mut out, 32, 2, 48_000);
        assert!(out.iter().all(|s| *s == 0.0));
        assert!((voice.position_frames() - 32.0).abs() < 1e-6);
    }
}

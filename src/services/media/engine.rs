//! Process-wide audio output. One cpal stream mixes every active `Voice`
//! (audio clips and the audio tracks of playing videos) with per-voice
//! resampling, gain ramps (no clicks on play/pause/seek), looping and
//! volume. The device stream lives on its own thread and is opened lazily
//! on first use; when no output device exists the engine reports that and
//! voices fall back to a silent wall-clock so transport UI still works.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{Duration, Instant};

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
    Silent {
        sample_rate: u32,
    },
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
    /// While output is unavailable, `position` is the frame at this instant.
    wall: Mutex<Option<Instant>>,
}

impl VoiceShared {
    fn position_frames(&self) -> f64 {
        f64::from_bits(self.position.load(Ordering::Acquire))
    }

    fn set_position_frames(&self, frames: f64) {
        self.position
            .store(frames.max(0.0).to_bits(), Ordering::Release);
    }

    fn wall_position(&self, started: Instant) -> f64 {
        let source = self.source.lock().unwrap_or_else(|e| e.into_inner());
        let rate = source.sample_rate() as f64 * self.rate() as f64;
        let frames = self.position_frames() + started.elapsed().as_secs_f64() * rate;
        match source.frame_count() {
            Some(total) if frames >= total as f64 => {
                if self.looping.load(Ordering::Relaxed) && total > 0 {
                    frames % total as f64
                } else {
                    self.ended.store(true, Ordering::Release);
                    self.state.store(STATE_STOPPED, Ordering::Release);
                    total as f64
                }
            }
            _ => frames,
        }
    }

    fn save_wall_position(&self, started: Instant) {
        self.set_position_frames(self.wall_position(started));
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
        let engine = engine();
        {
            let output = engine.output();
            // Keep the first sample queued while a new device is opening.
            if matches!(output.status, EngineStatus::Unavailable(_)) {
                let mut wall = self.shared.wall.lock().unwrap_or_else(|e| e.into_inner());
                wall.get_or_insert_with(Instant::now);
            }
            self.shared.ended.store(false, Ordering::Release);
            self.shared.state.store(STATE_PLAYING, Ordering::Release);
        }
        engine.ensure_started();
    }

    pub fn pause(&self) {
        let mut wall = self.shared.wall.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_playing() {
            let state = if let Some(started) = wall.take() {
                self.shared.save_wall_position(started);
                STATE_STOPPED
            } else {
                STATE_STOPPING
            };
            self.shared.state.store(state, Ordering::Release);
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

    pub fn position_frames(&self) -> f64 {
        let mut wall = self.shared.wall.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(started) = *wall {
            let pos = self.shared.wall_position(started);
            if self.shared.ended.load(Ordering::Acquire) {
                self.shared.set_position_frames(pos);
                *wall = None;
            }
            return pos;
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
        let mut wall = self.shared.wall.lock().unwrap_or_else(|e| e.into_inner());
        let frames = match self.duration_frames() {
            Some(total) => frames.min(total as f64),
            None => frames,
        };
        self.shared.ended.store(false, Ordering::Release);
        self.shared.set_position_frames(frames.max(0.0));
        if wall.is_some() {
            *wall = Some(Instant::now());
        }
    }

    /// Swap the underlying source (video seeks restart the audio stream).
    /// Playback state is preserved; the playhead restarts at 0 frames of the
    /// new source.
    pub fn set_source(&self, source: VoiceSource) {
        let mut wall = self.shared.wall.lock().unwrap_or_else(|e| e.into_inner());
        {
            let mut current = self.shared.source.lock().unwrap_or_else(|e| e.into_inner());
            *current = source;
        }
        self.shared.set_position_frames(0.0);
        self.shared.ended.store(false, Ordering::Release);
        self.shared.starved.store(false, Ordering::Release);
        if wall.is_some() {
            *wall = Some(Instant::now());
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
    output: Mutex<Output>,
}

struct Output {
    status: EngineStatus,
    voices: Vec<Weak<VoiceShared>>,
    retry: Option<mpsc::SyncSender<()>>,
}

impl Output {
    fn ready(&mut self, device: String, sample_rate: u32, channels: u16) {
        // The mixer takes the same lock, so it starts from the exact position
        // reached during silent playback, including any seeks or loop wraps.
        for voice in self.voices.iter().filter_map(Weak::upgrade) {
            let mut wall = voice.wall.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(started) = wall.take() {
                voice.save_wall_position(started);
            }
        }
        self.status = EngineStatus::Ready {
            device,
            sample_rate,
            channels,
        };
    }

    fn unavailable(&mut self, reason: String) -> bool {
        let mut playing = false;
        for voice in self.voices.iter().filter_map(Weak::upgrade) {
            let mut wall = voice.wall.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(started) = wall.take() {
                // The clip may have ended since the last retry, even when
                // its preview is no longer polling the playhead.
                voice.save_wall_position(started);
            }
            if voice.state.load(Ordering::Acquire) == STATE_PLAYING {
                *wall = Some(Instant::now());
                playing = true;
            } else {
                voice.state.store(STATE_STOPPED, Ordering::Release);
            }
            voice.starved.store(false, Ordering::Relaxed);
            voice.gain.store(0f32.to_bits(), Ordering::Relaxed);
        }
        self.status = EngineStatus::Unavailable(reason);
        playing
    }
}

static ENGINE: OnceLock<AudioEngine> = OnceLock::new();

pub fn engine() -> &'static AudioEngine {
    ENGINE.get_or_init(|| AudioEngine {
        output: Mutex::new(Output {
            status: EngineStatus::Idle,
            voices: Vec::new(),
            retry: None,
        }),
    })
}

impl AudioEngine {
    fn output(&self) -> MutexGuard<'_, Output> {
        self.output.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn register(&self, voice: &Arc<VoiceShared>) {
        let mut output = self.output();
        output.voices.retain(|w| w.strong_count() > 0);
        output.voices.push(Arc::downgrade(voice));
    }

    pub fn status(&self) -> EngineStatus {
        self.output().status.clone()
    }

    /// Request the current default output without waiting on the device.
    pub fn ensure_started(&'static self) {
        let mut output = self.output();
        if matches!(
            output.status,
            EngineStatus::Ready { .. } | EngineStatus::Starting
        ) {
            return;
        }
        if let Some(retry) = &output.retry {
            let _ = retry.try_send(());
        } else {
            let (tx, rx) = mpsc::sync_channel(1);
            match std::thread::Builder::new()
                .name("leviathan-audio".into())
                .spawn(move || self.run(rx))
            {
                Ok(_) => output.retry = Some(tx),
                Err(e) => {
                    output.unavailable(format!("could not start audio thread: {e}"));
                    return;
                }
            }
        }
        output.status = EngineStatus::Starting;
    }

    fn run(&'static self, retry: mpsc::Receiver<()>) {
        loop {
            let reason = match open_output(self) {
                Ok((stream, errors)) => {
                    let error = errors.recv();
                    drop(stream);
                    match error {
                        Ok(err) => format!("audio stream error: {err}"),
                        Err(_) => "audio stream closed".to_string(),
                    }
                }
                Err(err) => err,
            };
            let playing = self.output().unavailable(reason);
            // Recover during playback. While idle, wait for another preview
            // or Play request before probing the device again.
            if playing {
                let _ = retry.recv_timeout(Duration::from_secs(1));
            } else {
                let _ = retry.recv();
            }
            self.output().status = EngineStatus::Starting;
        }
    }
}

fn open_output(
    engine: &'static AudioEngine,
) -> Result<(cpal::Stream, mpsc::Receiver<cpal::StreamError>), String> {
    // CPAL can panic here if the platform backend itself fails to initialize.
    let host = std::panic::catch_unwind(cpal::default_host)
        .map_err(|_| "could not initialize the default audio host")?;
    let device = host
        .default_output_device()
        .ok_or("no audio output device")?;
    let device_name = device
        .description()
        .map(|d| d.to_string())
        .unwrap_or_else(|_| "default output".to_string());
    let supported = device
        .default_output_config()
        .map_err(|e| format!("no usable output configuration: {e}"))?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.config();
    let sample_rate: u32 = config.sample_rate;
    let channels = config.channels;

    let mixer = Mixer {
        engine,
        device_rate: sample_rate.max(1),
        device_channels: channels.max(1),
        scratch: Vec::new(),
    };
    let (failed, errors) = mpsc::sync_channel(1);
    let error_callback = move |err| {
        if err != cpal::StreamError::BufferUnderrun {
            let _ = failed.try_send(err);
        }
    };
    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config, mixer, error_callback),
        cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config, mixer, error_callback),
        cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config, mixer, error_callback),
        cpal::SampleFormat::I32 => build_stream::<i32>(&device, &config, mixer, error_callback),
        cpal::SampleFormat::U8 => build_stream::<u8>(&device, &config, mixer, error_callback),
        cpal::SampleFormat::F64 => build_stream::<f64>(&device, &config, mixer, error_callback),
        other => Err(format!("unsupported output sample format {other:?}")),
    }?;
    stream
        .play()
        .map_err(|e| format!("could not start audio stream: {e}"))?;
    engine.output().ready(device_name, sample_rate, channels);
    Ok((stream, errors))
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
            // A disconnected ALSA/PipeWire stream can stop requesting data
            // indefinitely. Have the backend report that stall to its owner.
            Some(Duration::from_secs(2)),
        )
        .map_err(|e| format!("could not open audio output: {e}"))
}

struct Mixer {
    engine: &'static AudioEngine,
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
        let Ok(output) = self.engine.output.try_lock() else {
            return;
        };
        if !matches!(output.status, EngineStatus::Ready { .. }) {
            return;
        }
        for weak in &output.voices {
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
    fn reconnect_hands_the_current_playhead_back_to_the_mixer() {
        let source = VoiceSource::Clip(clip(10_000, 1_000));
        let playing = Voice {
            shared: shared(source.clone()),
        };
        let paused = Voice {
            shared: shared(source),
        };
        paused.shared.state.store(STATE_STOPPING, Ordering::Release);
        paused.seek_frames(750.0);
        let mut output = Output {
            status: EngineStatus::Idle,
            voices: vec![
                Arc::downgrade(&playing.shared),
                Arc::downgrade(&paused.shared),
            ],
            retry: None,
        };

        playing.seek_frames(300.0);
        assert!(output.unavailable("device lost".into()));
        assert!(playing.shared.wall.lock().unwrap().is_some());
        assert_eq!(paused.shared.state.load(Ordering::Acquire), STATE_STOPPED);
        assert!(paused.shared.wall.lock().unwrap().is_none());

        playing.seek_frames(2_000.0);
        *playing.shared.wall.lock().unwrap() = Some(Instant::now() - Duration::from_millis(500));
        assert!(output.unavailable("still disconnected".into()));
        let silent_position = playing.position_frames();
        assert!(silent_position >= 2_500.0);
        output.ready("new device".into(), 48_000, 2);
        assert!(playing.shared.wall.lock().unwrap().is_none());
        assert!(playing.position_frames() >= silent_position);
        assert!(playing.is_playing());
        assert_eq!(paused.position_frames(), 750.0);
        assert!(!paused.is_playing());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn play_reopens_output_after_a_failed_preview() {
        // ALSA configuration and the engine are process-wide. Give this test
        // its own process so the virtual device is isolated from other tests.
        const CHILD: &str = "GIT_LEVIATHAN_TEST_AUDIO_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let dir = tempfile::tempdir().unwrap();
            let config = dir.path().join("asound.conf");
            std::fs::write(&config, "# No default PCM yet\n").unwrap();
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    std::thread::current().name().unwrap(),
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env("ALSA_CONFIG_PATH", &config)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }

        fn wait_for(check: impl Fn(&EngineStatus) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let status = engine().status();
                if check(&status) {
                    return;
                }
                assert!(Instant::now() < deadline, "timed out with {status:?}");
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        engine().ensure_started();
        wait_for(|s| matches!(s, EngineStatus::Unavailable(_)));
        let config = std::env::var_os("ALSA_CONFIG_PATH").unwrap();
        std::fs::write(config, "pcm.default { type null }\n").unwrap();
        let voice = Voice::new(VoiceSource::Clip(clip(4_800, 48_000)));
        voice.play();
        wait_for(|s| matches!(s, EngineStatus::Ready { .. }));
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

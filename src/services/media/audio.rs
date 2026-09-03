//! Audio decoding for the media diff. Clips are decoded fully into 16-bit
//! interleaved PCM (≤ 2 channels) so scrubbing is instant and the waveform
//! can be drawn from real samples. Symphonia handles the common codecs in
//! pure Rust; anything it can't (Opus, WMA, AMR, APE…) falls back to FFmpeg.

use std::path::Path;
use std::sync::Arc;

use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey};
use symphonia::core::probe::Hint;

use super::{format_bytes, MediaError};

/// Preview ceiling: 20 minutes of stereo 48 kHz is ~230 MB of PCM.
pub const MAX_CLIP_SECONDS: u64 = 20 * 60;
/// Peak/RMS buckets kept per channel for waveform drawing.
pub const WAVEFORM_BUCKETS: usize = 8192;

#[derive(Debug, Clone, Default)]
pub struct AudioInfo {
    pub codec: String,
    pub container: String,
    pub source_channels: u16,
    pub source_sample_rate: u32,
    pub bits_per_sample: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub file_size: u64,
    pub decoder: &'static str,
    /// Standard tags (title, artist, album…), then anything else.
    pub tags: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct WaveformBucket {
    pub min: f32,
    pub max: f32,
    pub rms: f32,
}

#[derive(Debug, Clone)]
pub struct Waveform {
    /// One vector of `WAVEFORM_BUCKETS` (or fewer for very short clips) per
    /// channel.
    pub channels: Vec<Vec<WaveformBucket>>,
    pub peak: f32,
}

pub struct AudioClip {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved 16-bit PCM.
    pub samples: Arc<[i16]>,
    pub truncated: bool,
    pub info: AudioInfo,
    pub waveform: Waveform,
}

impl AudioClip {
    pub fn frame_count(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn duration_secs(&self) -> f64 {
        self.frame_count() as f64 / self.sample_rate.max(1) as f64
    }

    pub fn properties(&self) -> Vec<(String, String)> {
        let info = &self.info;
        let mut rows = vec![
            (
                "Duration".to_string(),
                if self.truncated {
                    format!(
                        "{} (preview limited to {} min)",
                        super::format_timecode(self.duration_secs()),
                        MAX_CLIP_SECONDS / 60
                    )
                } else {
                    super::format_timecode(self.duration_secs())
                },
            ),
            ("Codec".to_string(), info.codec.clone()),
            ("Container".to_string(), info.container.clone()),
            (
                "Sample rate".to_string(),
                format!("{} Hz", group_thousands(info.source_sample_rate as u64)),
            ),
            (
                "Channels".to_string(),
                match info.source_channels {
                    1 => "Mono".to_string(),
                    2 => "Stereo".to_string(),
                    n => format!("{n} channels (downmixed to stereo)"),
                },
            ),
        ];
        if let Some(bits) = info.bits_per_sample {
            rows.push(("Bit depth".to_string(), format!("{bits}-bit")));
        }
        if let Some(kbps) = info.bitrate_kbps {
            rows.push(("Bitrate".to_string(), format!("{kbps} kb/s")));
        }
        rows.push(("File size".to_string(), format_bytes(info.file_size)));
        rows.push((
            "Peak level".to_string(),
            format!("{:.1} dBFS", 20.0 * self.waveform.peak.max(1e-6).log10()),
        ));
        rows.push(("Decoder".to_string(), info.decoder.to_string()));
        for (k, v) in &info.tags {
            rows.push((k.clone(), v.clone()));
        }
        rows
    }
}

impl std::fmt::Debug for AudioClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioClip")
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("frames", &self.frame_count())
            .field("truncated", &self.truncated)
            .field("info", &self.info)
            .finish()
    }
}

fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Decode `bytes` (whose file name is `file_path`, used for the format hint).
/// `on_disk` is the workdir path when the bytes came straight from disk, so
/// the FFmpeg fallback can skip writing a temp file.
pub fn decode_audio(
    bytes: &[u8],
    file_path: &str,
    on_disk: Option<&Path>,
) -> Result<AudioClip, MediaError> {
    let file_size = bytes.len() as u64;
    match decode_with_symphonia(bytes, file_path) {
        Ok(mut clip) => {
            clip.info.file_size = file_size;
            fill_bitrate(&mut clip);
            Ok(clip)
        }
        Err(primary) => {
            if !super::ffmpeg::is_available() {
                return Err(primary);
            }
            match decode_with_ffmpeg(bytes, file_path, on_disk) {
                Ok(mut clip) => {
                    clip.info.file_size = file_size;
                    fill_bitrate(&mut clip);
                    Ok(clip)
                }
                // Keep the more specific primary error when FFmpeg is equally lost.
                Err(MediaError::Unrecognized) => Err(primary),
                Err(other) => Err(other),
            }
        }
    }
}

fn fill_bitrate(clip: &mut AudioClip) {
    if clip.info.bitrate_kbps.is_none() && !clip.truncated {
        let secs = clip.duration_secs();
        if secs > 0.0 {
            clip.info.bitrate_kbps =
                Some(((clip.info.file_size as f64 * 8.0) / secs / 1000.0).round() as u32);
        }
    }
}

// ---------------------------------------------------------------------------
// Symphonia
// ---------------------------------------------------------------------------

struct OwnedBytes(Arc<[u8]>);

impl AsRef<[u8]> for OwnedBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

fn decode_with_symphonia(bytes: &[u8], file_path: &str) -> Result<AudioClip, MediaError> {
    let owned: Arc<[u8]> = Arc::from(bytes);
    let cursor = std::io::Cursor::new(OwnedBytes(owned));
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = file_path.rsplit('.').next() {
        if ext.len() <= 5 {
            hint.with_extension(ext);
        }
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| match e {
            SymphoniaError::Unsupported(what) => MediaError::Unsupported(what.to_string()),
            SymphoniaError::IoError(_) => MediaError::Unrecognized,
            _ => MediaError::Unrecognized,
        })?;
    let mut probed = probed;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .cloned()
        .ok_or_else(|| MediaError::Unsupported("no decodable audio track".to_string()))?;
    let track_id = track.id;
    let params = &track.codec_params;
    let source_rate = params
        .sample_rate
        .ok_or_else(|| MediaError::Unsupported("unknown sample rate".to_string()))?;
    let source_channels = params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(0);

    let codec_desc = symphonia::default::get_codecs().get_codec(params.codec);
    let codec_name = codec_desc
        .map(|d| d.long_name.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let mut decoder = symphonia::default::get_codecs()
        .make(params, &DecoderOptions::default())
        .map_err(|e| match e {
            SymphoniaError::Unsupported(what) => MediaError::Unsupported(what.to_string()),
            other => MediaError::Corrupt(other.to_string()),
        })?;

    let max_frames = (source_rate as u64 * MAX_CLIP_SECONDS) as usize;
    let mut out: Vec<i16> = Vec::new();
    let mut out_channels: u16 = 0;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut truncated = false;
    let mut decode_errors = 0usize;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(SymphoniaError::IoError(_)) => break,
            Err(SymphoniaError::DecodeError(_)) => {
                decode_errors += 1;
                if decode_errors > 64 {
                    break;
                }
                continue;
            }
            Err(other) => {
                if out.is_empty() {
                    return Err(MediaError::Corrupt(other.to_string()));
                }
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => {
                decode_errors += 1;
                if decode_errors > 64 {
                    break;
                }
                continue;
            }
            Err(SymphoniaError::IoError(_)) => break,
            Err(other) => {
                if out.is_empty() {
                    return Err(MediaError::Corrupt(other.to_string()));
                }
                break;
            }
        };

        let spec = *decoded.spec();
        let channels = spec.channels.count() as u16;
        if out_channels == 0 {
            out_channels = channels.clamp(1, 2);
        }
        let frames = decoded.frames();
        if frames == 0 {
            continue;
        }
        let buf = sample_buf.get_or_insert_with(|| {
            SampleBuffer::<f32>::new(decoded.capacity() as u64, spec)
        });
        if buf.capacity() < frames * channels as usize {
            *buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        }
        copy_interleaved(&decoded, buf);
        let samples = buf.samples();

        let remaining = max_frames.saturating_sub(out.len() / out_channels as usize);
        let take_frames = frames.min(remaining);
        downmix_append(&mut out, samples, channels, out_channels, take_frames);
        if take_frames < frames {
            truncated = true;
            break;
        }
    }

    if out.is_empty() {
        return Err(MediaError::Corrupt(
            "no audio samples could be decoded".to_string(),
        ));
    }

    // Tags: container-level first, then any revision embedded in the stream.
    let mut tags = Vec::new();
    if let Some(meta) = probed.metadata.get() {
        if let Some(rev) = meta.current() {
            collect_tags(rev.tags(), &mut tags);
        }
    }
    if let Some(rev) = format.metadata().current() {
        collect_tags(rev.tags(), &mut tags);
    }
    tags.dedup();

    let container = container_label(file_path, bytes);
    let info = AudioInfo {
        codec: codec_name,
        container,
        source_channels: if source_channels == 0 {
            out_channels
        } else {
            source_channels
        },
        source_sample_rate: source_rate,
        bits_per_sample: params.bits_per_sample,
        bitrate_kbps: None,
        file_size: bytes.len() as u64,
        decoder: "symphonia",
        tags,
    };

    let waveform = build_waveform(&out, out_channels, WAVEFORM_BUCKETS);
    Ok(AudioClip {
        sample_rate: source_rate,
        channels: out_channels,
        samples: Arc::from(out),
        truncated,
        info,
        waveform,
    })
}

fn copy_interleaved(decoded: &AudioBufferRef<'_>, buf: &mut SampleBuffer<f32>) {
    buf.copy_interleaved_ref(decoded.clone());
}

fn collect_tags(tags: &[symphonia::core::meta::Tag], out: &mut Vec<(String, String)>) {
    let mut standard = Vec::new();
    let mut other = Vec::new();
    for tag in tags {
        let value = tag.value.to_string();
        let value = value.trim().to_string();
        if value.is_empty() || value.len() > 200 {
            continue;
        }
        match tag.std_key {
            Some(key) => standard.push((standard_tag_label(key).to_string(), value)),
            None => {
                let key = tag.key.trim();
                if !key.is_empty() && key.len() <= 40 && !key.starts_with("----") {
                    other.push((prettify_key(key), value));
                }
            }
        }
    }
    standard.sort_by_key(|(k, _)| standard_tag_order(k));
    other.sort();
    out.extend(standard);
    out.extend(other);
}

fn standard_tag_label(key: StandardTagKey) -> &'static str {
    match key {
        StandardTagKey::TrackTitle => "Title",
        StandardTagKey::Artist => "Artist",
        StandardTagKey::AlbumArtist => "Album artist",
        StandardTagKey::Album => "Album",
        StandardTagKey::Date => "Date",
        StandardTagKey::OriginalDate => "Original date",
        StandardTagKey::Genre => "Genre",
        StandardTagKey::TrackNumber => "Track",
        StandardTagKey::TrackTotal => "Track total",
        StandardTagKey::DiscNumber => "Disc",
        StandardTagKey::Composer => "Composer",
        StandardTagKey::Comment => "Comment",
        StandardTagKey::Encoder => "Encoder",
        StandardTagKey::EncodedBy => "Encoded by",
        StandardTagKey::Copyright => "Copyright",
        StandardTagKey::Bpm => "BPM",
        StandardTagKey::Lyrics => "Lyrics",
        StandardTagKey::Language => "Language",
        StandardTagKey::Label => "Label",
        StandardTagKey::ReplayGainTrackGain => "ReplayGain (track)",
        StandardTagKey::ReplayGainAlbumGain => "ReplayGain (album)",
        StandardTagKey::Description => "Description",
        StandardTagKey::Producer => "Producer",
        StandardTagKey::Performer => "Performer",
        StandardTagKey::Writer => "Writer",
        StandardTagKey::Version => "Version",
        StandardTagKey::License => "License",
        StandardTagKey::Url => "URL",
        StandardTagKey::Rating => "Rating",
        StandardTagKey::Mood => "Mood",
        _ => "Tag",
    }
}

fn standard_tag_order(label: &str) -> usize {
    const ORDER: &[&str] = &[
        "Title",
        "Artist",
        "Album artist",
        "Album",
        "Track",
        "Disc",
        "Date",
        "Genre",
        "Composer",
        "Comment",
        "Encoder",
    ];
    ORDER
        .iter()
        .position(|l| *l == label)
        .unwrap_or(ORDER.len())
}

fn prettify_key(key: &str) -> String {
    let mut out = String::new();
    for (i, part) in key
        .split(['_', '-', ' '])
        .filter(|p| !p.is_empty())
        .enumerate()
    {
        if i > 0 {
            out.push(' ');
        }
        let lower = part.to_ascii_lowercase();
        if i == 0 {
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                out.push(first.to_ascii_uppercase());
                out.push_str(chars.as_str());
            }
        } else {
            out.push_str(&lower);
        }
    }
    out
}

fn container_label(file_path: &str, bytes: &[u8]) -> String {
    let starts = |sig: &[u8]| bytes.len() >= sig.len() && &bytes[..sig.len()] == sig;
    if starts(b"RIFF") {
        "WAVE (RIFF)".to_string()
    } else if starts(b"RF64") {
        "RF64".to_string()
    } else if starts(b"fLaC") {
        "FLAC".to_string()
    } else if starts(b"OggS") {
        "Ogg".to_string()
    } else if starts(b"ID3") || starts(b"\xFF\xFB") || starts(b"\xFF\xF3") || starts(b"\xFF\xF2") {
        "MPEG audio".to_string()
    } else if starts(b"FORM") {
        "AIFF".to_string()
    } else if starts(b"caff") {
        "CAF".to_string()
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        "MPEG-4".to_string()
    } else if starts(b"\x1A\x45\xDF\xA3") {
        "Matroska".to_string()
    } else {
        file_path
            .rsplit('.')
            .next()
            .map(|e| e.to_ascii_uppercase())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

/// Append `frames` frames from interleaved `f32` samples with `src_channels`
/// channels, folding down to `dst_channels` (1 or 2) as 16-bit PCM.
fn downmix_append(
    out: &mut Vec<i16>,
    samples: &[f32],
    src_channels: u16,
    dst_channels: u16,
    frames: usize,
) {
    let sc = src_channels.max(1) as usize;
    out.reserve(frames * dst_channels as usize);
    for frame in samples.chunks_exact(sc).take(frames) {
        match (sc, dst_channels) {
            (1, 1) => out.push(to_i16(frame[0])),
            (1, 2) => {
                let s = to_i16(frame[0]);
                out.push(s);
                out.push(s);
            }
            (2, 1) => out.push(to_i16((frame[0] + frame[1]) * 0.5)),
            (_, 1) => {
                let sum: f32 = frame.iter().sum();
                out.push(to_i16(sum / sc as f32));
            }
            (2, _) => {
                out.push(to_i16(frame[0]));
                out.push(to_i16(frame[1]));
            }
            _ => {
                // Generic surround fold: L/R plus centre at -3 dB plus the
                // remaining channels at -6 dB split by parity.
                let mut l = frame[0];
                let mut r = frame[1];
                if sc >= 3 {
                    l += frame[2] * std::f32::consts::FRAC_1_SQRT_2;
                    r += frame[2] * std::f32::consts::FRAC_1_SQRT_2;
                }
                for (i, s) in frame.iter().enumerate().skip(3) {
                    if i % 2 == 0 {
                        l += s * 0.5;
                    } else {
                        r += s * 0.5;
                    }
                }
                out.push(to_i16(l));
                out.push(to_i16(r));
            }
        }
    }
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

// ---------------------------------------------------------------------------
// FFmpeg fallback
// ---------------------------------------------------------------------------

fn decode_with_ffmpeg(
    bytes: &[u8],
    file_path: &str,
    on_disk: Option<&Path>,
) -> Result<AudioClip, MediaError> {
    let temp;
    let path: &Path = match on_disk {
        Some(path) => path,
        None => {
            let ext = file_path
                .rsplit('.')
                .next()
                .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
                .unwrap_or("bin");
            temp = tempfile::Builder::new()
                .prefix("git-leviathan-audio-")
                .suffix(&format!(".{ext}"))
                .tempfile()
                .map_err(|e| MediaError::Io(e.to_string()))?;
            std::fs::write(temp.path(), bytes).map_err(|e| MediaError::Io(e.to_string()))?;
            temp.path()
        }
    };

    let probe = super::ffmpeg::probe(path)?;
    let stream = probe
        .audio
        .clone()
        .ok_or_else(|| MediaError::Unsupported("no audio stream".to_string()))?;
    let sample_rate = if stream.sample_rate > 0 {
        stream.sample_rate
    } else {
        48_000
    };
    let channels: u16 = stream.channels.clamp(1, 2);
    let max_frames = (sample_rate as u64 * MAX_CLIP_SECONDS) as usize;
    let (samples, truncated) =
        super::ffmpeg::decode_audio_pcm(path, sample_rate, channels, max_frames)?;
    if samples.is_empty() {
        return Err(MediaError::Corrupt("no audio samples decoded".to_string()));
    }

    let mut tags: Vec<(String, String)> = probe
        .tags
        .iter()
        .map(|(k, v)| (prettify_key(k), v.clone()))
        .collect();
    tags.sort_by_key(|(k, _)| standard_tag_order(k));

    let info = AudioInfo {
        codec: if stream.codec_long.is_empty() {
            stream.codec.clone()
        } else {
            stream.codec_long.clone()
        },
        container: if probe.container_long.is_empty() {
            probe.container.clone()
        } else {
            probe.container_long.clone()
        },
        source_channels: stream.channels.max(channels),
        source_sample_rate: sample_rate,
        bits_per_sample: None,
        bitrate_kbps: stream
            .bit_rate
            .or(probe.bit_rate)
            .map(|b| (b / 1000) as u32),
        file_size: bytes.len() as u64,
        decoder: "ffmpeg",
        tags,
    };
    let waveform = build_waveform(&samples, channels, WAVEFORM_BUCKETS);
    Ok(AudioClip {
        sample_rate,
        channels,
        samples: Arc::from(samples),
        truncated,
        info,
        waveform,
    })
}

// ---------------------------------------------------------------------------
// Waveform
// ---------------------------------------------------------------------------

pub fn build_waveform(samples: &[i16], channels: u16, buckets: usize) -> Waveform {
    let ch = channels.max(1) as usize;
    let frames = samples.len() / ch;
    let buckets = buckets.min(frames.max(1));
    let mut out: Vec<Vec<WaveformBucket>> = (0..ch).map(|_| Vec::with_capacity(buckets)).collect();
    let mut peak = 0.0f32;
    if frames == 0 {
        return Waveform {
            channels: out,
            peak,
        };
    }
    for b in 0..buckets {
        let start = b * frames / buckets;
        let end = ((b + 1) * frames / buckets).max(start + 1).min(frames);
        for (c, channel) in out.iter_mut().enumerate() {
            let mut min = i16::MAX;
            let mut max = i16::MIN;
            let mut sq = 0.0f64;
            for frame in start..end {
                let s = samples[frame * ch + c];
                min = min.min(s);
                max = max.max(s);
                let f = s as f64 / 32768.0;
                sq += f * f;
            }
            let n = (end - start) as f64;
            let bucket = WaveformBucket {
                min: min as f32 / 32768.0,
                max: max as f32 / 32768.0,
                rms: (sq / n).sqrt() as f32,
            };
            peak = peak.max(bucket.max.abs()).max(bucket.min.abs());
            channel.push(bucket);
        }
    }
    Waveform {
        channels: out,
        peak,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_bytes(sample_rate: u32, channels: u16, frames: usize, f: impl Fn(usize) -> i16) -> Vec<u8> {
        let data_len = (frames * channels as usize * 2) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            for _ in 0..channels {
                out.extend_from_slice(&f(i).to_le_bytes());
            }
        }
        out
    }

    #[test]
    fn decodes_wav_via_symphonia() {
        let bytes = wav_bytes(8000, 2, 4000, |i| {
            ((i as f64 * 0.1).sin() * 16000.0) as i16
        });
        let clip = decode_audio(&bytes, "tone.wav", None).unwrap();
        assert_eq!(clip.sample_rate, 8000);
        assert_eq!(clip.channels, 2);
        assert_eq!(clip.frame_count(), 4000);
        assert!((clip.duration_secs() - 0.5).abs() < 1e-6);
        assert_eq!(clip.info.decoder, "symphonia");
        assert_eq!(clip.info.source_channels, 2);
        assert_eq!(clip.waveform.channels.len(), 2);
        assert!(clip.waveform.peak > 0.4);
        assert!(clip.properties().iter().any(|(k, _)| k == "Duration"));
    }

    #[test]
    fn garbage_is_unrecognized() {
        let err = decode_audio(b"this is not audio at all, really it is not", "x.wav", None)
            .unwrap_err();
        assert!(
            matches!(err, MediaError::Unrecognized | MediaError::Corrupt(_) | MediaError::Unsupported(_)),
            "{err:?}"
        );
    }

    #[test]
    fn waveform_buckets_track_extremes_and_rms() {
        let samples: Vec<i16> = vec![0, 0, 16384, -16384, 32767, 0, 0, 0];
        let wf = build_waveform(&samples, 1, 4);
        assert_eq!(wf.channels.len(), 1);
        assert_eq!(wf.channels[0].len(), 4);
        assert!((wf.channels[0][1].max - 0.5).abs() < 1e-3);
        assert!((wf.channels[0][1].min + 0.5).abs() < 1e-3);
        assert!((wf.channels[0][2].max - 1.0).abs() < 1e-3);
        assert!(wf.channels[0][2].rms > 0.5);
        assert!((wf.peak - 1.0).abs() < 1e-3);
    }

    #[test]
    fn waveform_of_empty_clip_is_empty() {
        let wf = build_waveform(&[], 2, 100);
        assert_eq!(wf.channels.len(), 2);
        assert!(wf.channels[0].is_empty());
    }

    #[test]
    fn downmix_folds_surround_to_stereo() {
        let mut out = Vec::new();
        // 6 channels: L R C LFE Ls Rs
        let frame = [1.0f32, -1.0, 0.5, 0.0, 0.2, -0.2];
        downmix_append(&mut out, &frame, 6, 2, 1);
        assert_eq!(out.len(), 2);
        assert!(out[0] > 0 && out[1] < 0);
        assert_eq!(out[0], 32767); // clamped
    }

    #[test]
    fn downmix_mono_to_stereo_duplicates() {
        let mut out = Vec::new();
        downmix_append(&mut out, &[0.25, 0.5], 1, 2, 2);
        assert_eq!(out, vec![8192, 8192, 16384, 16384]);
    }

    #[test]
    fn prettifies_tag_keys() {
        assert_eq!(prettify_key("ENCODER_OPTIONS"), "Encoder options");
        assert_eq!(prettify_key("album-artist"), "Album artist");
        assert_eq!(group_thousands(44100), "44,100");
        assert_eq!(group_thousands(999), "999");
    }
}

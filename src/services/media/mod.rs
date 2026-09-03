//! Media support for the diff view: detection, decoding and playback of
//! images, audio and video files coming out of git blobs or the working tree.
//!
//! Layout:
//! - `detect`  — extension + magic-byte classification (`MediaKind`).
//! - `image`   — raster/vector decoding, EXIF, animated frames, pixel diffs.
//! - `audio`   — full-clip PCM decoding (symphonia, FFmpeg fallback) + waveform.
//! - `engine`  — the process-wide audio output mixer (cpal).
//! - `video`   — FFmpeg-backed streaming video player with A/V sync.
//! - `ffmpeg`  — locating the FFmpeg tools and probing container metadata.
//!
//! Everything CPU-heavy is meant to run off the UI thread through
//! `presentation_work`; the types handed back to the UI are cheap `Arc`s.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

pub mod audio;
pub mod detect;
pub mod engine;
pub mod ffmpeg;
pub mod image;
pub mod video;

pub use detect::{media_kind_from_path, sniff_media_kind, MediaKind};

/// Hard ceilings so a stray multi-gigabyte asset can't take the process down.
pub const MAX_IMAGE_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_AUDIO_FILE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_VIDEO_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Which side of the change a payload belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaSide {
    Old,
    New,
}

impl MediaSide {
    pub fn label(self) -> &'static str {
        match self {
            MediaSide::Old => "Old",
            MediaSide::New => "New",
        }
    }

    pub fn other(self) -> MediaSide {
        match self {
            MediaSide::Old => MediaSide::New,
            MediaSide::New => MediaSide::Old,
        }
    }
}

/// Where the bytes of one side of a media diff come from. Produced by the
/// git layer; consumed by the decoders.
#[derive(Debug, Clone)]
pub enum MediaSource {
    /// The path does not exist on this side (new file, or deleted file).
    Missing,
    /// Present but over the size ceiling for its kind.
    TooLarge { bytes: u64, max: u64 },
    /// Bytes of a git blob (HEAD / index / commit tree).
    Blob { bytes: Arc<[u8]>, oid: String },
    /// A file in the working tree — read lazily so video can be streamed
    /// straight from disk without copying it through memory.
    WorkdirFile { path: PathBuf, size: u64 },
}

impl MediaSource {
    pub fn is_missing(&self) -> bool {
        matches!(self, MediaSource::Missing)
    }

    pub fn byte_len(&self) -> Option<u64> {
        match self {
            MediaSource::Missing => None,
            MediaSource::TooLarge { bytes, .. } => Some(*bytes),
            MediaSource::Blob { bytes, .. } => Some(bytes.len() as u64),
            MediaSource::WorkdirFile { size, .. } => Some(*size),
        }
    }

    /// Materialize the bytes (reads the workdir file when necessary).
    pub fn read_bytes(&self) -> Result<Arc<[u8]>, MediaError> {
        match self {
            MediaSource::Missing => Err(MediaError::Missing),
            MediaSource::TooLarge { bytes, max } => Err(MediaError::TooLarge {
                bytes: *bytes,
                max: *max,
            }),
            MediaSource::Blob { bytes, .. } => Ok(bytes.clone()),
            MediaSource::WorkdirFile { path, .. } => std::fs::read(path)
                .map(Arc::from)
                .map_err(|e| MediaError::Io(format!("{}: {e}", path.display()))),
        }
    }
}

/// Result of fetching both sides of a media change from git.
#[derive(Debug, Clone)]
pub struct MediaDiffSources {
    pub file_path: String,
    pub old: MediaSource,
    pub new: MediaSource,
}

/// Failure modes surfaced to the UI as text on the affected side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    Missing,
    TooLarge { bytes: u64, max: u64 },
    Io(String),
    /// The bytes are not a recognizable media container (wrong extension,
    /// text masquerading as media, or a truncated header).
    Unrecognized,
    /// The container was recognized but the codec/format has no decoder.
    Unsupported(String),
    /// Decoding failed part-way (corruption).
    Corrupt(String),
    /// Video/exotic-format support needs FFmpeg on `PATH`.
    FfmpegMissing,
    /// FFmpeg ran but failed.
    Ffmpeg(String),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaError::Missing => write!(f, "No content on this side."),
            MediaError::TooLarge { bytes, max } => write!(
                f,
                "File is {} which exceeds the {} preview limit.",
                format_bytes(*bytes),
                format_bytes(*max)
            ),
            MediaError::Io(msg) => write!(f, "Could not read file: {msg}"),
            MediaError::Unrecognized => write!(
                f,
                "The content is not a recognized media format (it may be corrupted or mislabelled)."
            ),
            MediaError::Unsupported(what) => write!(f, "Unsupported media: {what}."),
            MediaError::Corrupt(msg) => write!(f, "The file could not be decoded: {msg}"),
            MediaError::FfmpegMissing => write!(
                f,
                "Previewing this format requires FFmpeg. Install `ffmpeg` and `ffprobe` and reopen the file."
            ),
            MediaError::Ffmpeg(msg) => write!(f, "FFmpeg failed: {msg}"),
        }
    }
}

impl std::error::Error for MediaError {}

/// Fully decoded, display-ready media for one side of the diff.
#[derive(Clone)]
pub enum DecodedMedia {
    Image(Arc<image::DecodedImage>),
    Audio(Arc<audio::AudioClip>),
    Video(Arc<video::VideoPlayer>),
}

impl DecodedMedia {
    pub fn kind(&self) -> MediaKind {
        match self {
            DecodedMedia::Image(_) => MediaKind::Image,
            DecodedMedia::Audio(_) => MediaKind::Audio,
            DecodedMedia::Video(_) => MediaKind::Video,
        }
    }
}

impl fmt::Debug for DecodedMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodedMedia::Image(img) => f
                .debug_struct("DecodedMedia::Image")
                .field("width", &img.width)
                .field("height", &img.height)
                .field("frames", &img.frames.len())
                .finish(),
            DecodedMedia::Audio(clip) => f
                .debug_struct("DecodedMedia::Audio")
                .field("duration", &clip.duration_secs())
                .field("channels", &clip.channels)
                .field("sample_rate", &clip.sample_rate)
                .finish(),
            DecodedMedia::Video(player) => f
                .debug_struct("DecodedMedia::Video")
                .field("duration", &player.info().duration_secs)
                .field("size", &(player.info().width, player.info().height))
                .finish(),
        }
    }
}

/// Decode one side. `hint_kind` is the classification derived from the file
/// name; the bytes are sniffed too so a `.png` that is really a JPEG (or a
/// `.mp4` that is actually audio-only) still lands in the right decoder.
pub fn decode_side(
    hint_kind: MediaKind,
    source: &MediaSource,
    file_path: &str,
) -> Result<DecodedMedia, MediaError> {
    let span = crate::perf::Span::new("cpu.media_decode")
        .field("path", file_path)
        .field("hint", hint_kind.label());
    let result = decode_side_inner(hint_kind, source, file_path);
    match &result {
        Ok(decoded) => span.finish_with("kind", decoded.kind().label()),
        Err(err) => span.finish_with("error", err.to_string()),
    }
    result
}

fn decode_side_inner(
    hint_kind: MediaKind,
    source: &MediaSource,
    file_path: &str,
) -> Result<DecodedMedia, MediaError> {
    match source {
        MediaSource::Missing => return Err(MediaError::Missing),
        MediaSource::TooLarge { bytes, max } => {
            return Err(MediaError::TooLarge {
                bytes: *bytes,
                max: *max,
            })
        }
        _ => {}
    }

    // Peek at the header without pulling a whole video into memory.
    let header = source_header(source, 64 * 1024)?;
    let sniffed = sniff_media_kind(&header);
    let kind = sniffed.unwrap_or(hint_kind);

    match kind {
        MediaKind::Image => {
            let bytes = read_bounded(source, MAX_IMAGE_FILE_BYTES)?;
            image::decode_image(&bytes, file_path).map(|img| DecodedMedia::Image(Arc::new(img)))
        }
        MediaKind::Audio => {
            let bytes = read_bounded(source, MAX_AUDIO_FILE_BYTES)?;
            let on_disk = match source {
                MediaSource::WorkdirFile { path, .. } => Some(path.as_path()),
                _ => None,
            };
            audio::decode_audio(&bytes, file_path, on_disk)
                .map(|clip| DecodedMedia::Audio(Arc::new(clip)))
        }
        MediaKind::Video => {
            if let Some(size) = source.byte_len() {
                if size > MAX_VIDEO_FILE_BYTES {
                    return Err(MediaError::TooLarge {
                        bytes: size,
                        max: MAX_VIDEO_FILE_BYTES,
                    });
                }
            }
            let player = video::VideoPlayer::open(source, file_path)?;
            // Containers like MP4/MKV/OGG/WebM can be audio-only: route those
            // to the waveform player so the user gets scrubbable audio instead
            // of a black video surface.
            if player.info().width == 0 || player.info().height == 0 {
                if player.info().has_audio {
                    let bytes = read_bounded(source, MAX_AUDIO_FILE_BYTES)?;
                    let on_disk = match source {
                        MediaSource::WorkdirFile { path, .. } => Some(path.as_path()),
                        _ => None,
                    };
                    return audio::decode_audio(&bytes, file_path, on_disk)
                        .map(|clip| DecodedMedia::Audio(Arc::new(clip)));
                }
                return Err(MediaError::Unsupported(
                    "container has no video or audio streams".to_string(),
                ));
            }
            Ok(DecodedMedia::Video(Arc::new(player)))
        }
    }
}

fn source_header(source: &MediaSource, max: usize) -> Result<Vec<u8>, MediaError> {
    match source {
        MediaSource::Blob { bytes, .. } => Ok(bytes[..bytes.len().min(max)].to_vec()),
        MediaSource::WorkdirFile { path, .. } => {
            use std::io::Read;
            let mut file = std::fs::File::open(path)
                .map_err(|e| MediaError::Io(format!("{}: {e}", path.display())))?;
            let mut buf = vec![0u8; max];
            let mut filled = 0;
            while filled < max {
                let n = file
                    .read(&mut buf[filled..])
                    .map_err(|e| MediaError::Io(format!("{}: {e}", path.display())))?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            buf.truncate(filled);
            Ok(buf)
        }
        MediaSource::Missing => Err(MediaError::Missing),
        MediaSource::TooLarge { bytes, max } => Err(MediaError::TooLarge {
            bytes: *bytes,
            max: *max,
        }),
    }
}

fn read_bounded(source: &MediaSource, max: u64) -> Result<Arc<[u8]>, MediaError> {
    if let Some(len) = source.byte_len() {
        if len > max {
            return Err(MediaError::TooLarge { bytes: len, max });
        }
    }
    source.read_bytes()
}

/// `1.2 MB`, `845 KB`, `12 B` — binary units, one decimal for MB and up.
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// `m:ss.cc`, or `h:mm:ss.cc` once an hour is reached.
pub fn format_timecode(secs: f64) -> String {
    let secs = if secs.is_finite() { secs.max(0.0) } else { 0.0 };
    let total_cs = (secs * 100.0).round() as u64;
    let cs = total_cs % 100;
    let total_s = total_cs / 100;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{cs:02}")
    } else {
        format!("{m}:{s:02}.{cs:02}")
    }
}

/// Short form without centiseconds, for compact labels: `1:05`, `1:02:03`.
pub fn format_timecode_short(secs: f64) -> String {
    let secs = if secs.is_finite() { secs.max(0.0) } else { 0.0 };
    let total_s = secs.round() as u64;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_formatting_uses_binary_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn timecode_formatting_rolls_over_units() {
        assert_eq!(format_timecode(0.0), "0:00.00");
        assert_eq!(format_timecode(65.5), "1:05.50");
        assert_eq!(format_timecode(3661.25), "1:01:01.25");
        assert_eq!(format_timecode(-4.0), "0:00.00");
        assert_eq!(format_timecode(f64::NAN), "0:00.00");
        assert_eq!(format_timecode_short(3599.6), "1:00:00");
    }

    #[test]
    fn missing_source_reports_missing() {
        let err = decode_side(MediaKind::Image, &MediaSource::Missing, "a.png").unwrap_err();
        assert_eq!(err, MediaError::Missing);
    }

    #[test]
    fn too_large_source_is_rejected_before_decoding() {
        let src = MediaSource::TooLarge {
            bytes: 10,
            max: 5,
        };
        let err = decode_side(MediaKind::Image, &src, "a.png").unwrap_err();
        assert!(matches!(err, MediaError::TooLarge { bytes: 10, max: 5 }));
    }

    /// Runs `ffmpeg` to synthesize a fixture; `None` when FFmpeg is absent
    /// (the test then passes vacuously so CI without FFmpeg stays green).
    fn synth(dir: &std::path::Path, name: &str, args: &[&str]) -> Option<std::path::PathBuf> {
        let tools = ffmpeg::tools()?;
        let out = dir.join(name);
        let status = std::process::Command::new(&tools.ffmpeg)
            .args(["-v", "error", "-y"])
            .args(args)
            .arg(&out)
            .status()
            .ok()?;
        status.success().then_some(out)
    }

    fn workdir_source(path: &std::path::Path) -> MediaSource {
        MediaSource::WorkdirFile {
            path: path.to_path_buf(),
            size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        }
    }

    #[test]
    fn video_pipeline_decodes_frames_and_seeks() {
        let dir = tempfile::tempdir().unwrap();
        let Some(path) = synth(
            dir.path(),
            "clip.mp4",
            &[
                "-f", "lavfi", "-i", "testsrc=size=320x180:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=440:duration=3",
                "-t", "3", "-pix_fmt", "yuv420p", "-c:v", "libx264", "-c:a", "aac", "-shortest",
            ],
        ) else {
            eprintln!("ffmpeg not available; skipping video pipeline test");
            return;
        };
        let decoded = decode_side(MediaKind::Video, &workdir_source(&path), "clip.mp4").unwrap();
        let DecodedMedia::Video(player) = decoded else {
            panic!("expected video, got {decoded:?}");
        };
        let info = player.info();
        assert_eq!((info.width, info.height), (320, 180));
        assert!((info.fps - 25.0).abs() < 0.01, "fps {}", info.fps);
        assert!(info.has_audio);
        assert!((info.duration_secs - 3.0).abs() < 0.2, "duration {}", info.duration_secs);

        // Poster frame arrives without playing.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while player.current_frame().is_none() && std::time::Instant::now() < deadline {
            player.advance(std::time::Instant::now());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let poster = player.current_frame().expect("poster frame");
        assert_eq!((poster.width, poster.height), (320, 180));
        assert!(poster.pts.abs() < 0.05);

        // Playback advances the clock and swaps frames.
        player.play();
        assert!(player.is_playing());
        let start = std::time::Instant::now();
        let first_serial = player.frame_serial();
        while std::time::Instant::now() - start < std::time::Duration::from_millis(1500) {
            player.advance(std::time::Instant::now());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(player.frame_serial() > first_serial, "frames should advance while playing");
        assert!(player.position_secs() > 0.3, "position {}", player.position_secs());
        player.pause();
        assert!(!player.is_playing());

        // Seeking restarts the pipeline near the target.
        player.seek(2.0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let before = player.frame_serial();
        while player.frame_serial() == before && std::time::Instant::now() < deadline {
            player.advance(std::time::Instant::now());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let frame = player.current_frame().unwrap();
        assert!((frame.pts - 2.0).abs() < 0.2, "seek landed at {}", frame.pts);
        assert!((player.position_secs() - 2.0).abs() < 0.2);

        // Frame stepping moves exactly one frame.
        let pts_before = player.current_frame().unwrap().pts;
        player.step_frame(1);
        let pts_after = player.current_frame().unwrap().pts;
        assert!((pts_after - pts_before - 1.0 / 25.0).abs() < 0.005, "{pts_before} -> {pts_after}");
    }

    #[test]
    fn opus_audio_falls_back_to_ffmpeg() {
        let dir = tempfile::tempdir().unwrap();
        let Some(path) = synth(
            dir.path(),
            "voice.opus",
            &["-f", "lavfi", "-i", "sine=frequency=660:duration=2", "-c:a", "libopus"],
        ) else {
            return;
        };
        let decoded = decode_side(MediaKind::Audio, &workdir_source(&path), "voice.opus").unwrap();
        let DecodedMedia::Audio(clip) = decoded else {
            panic!("expected audio");
        };
        assert_eq!(clip.info.decoder, "ffmpeg");
        assert!((clip.duration_secs() - 2.0).abs() < 0.2, "{}", clip.duration_secs());
        // ffmpeg's `sine` source peaks around -18 dBFS.
        assert!(clip.waveform.peak > 0.05, "peak {}", clip.waveform.peak);
    }

    #[test]
    fn audio_only_mp4_routes_to_audio_player() {
        let dir = tempfile::tempdir().unwrap();
        let Some(path) = synth(
            dir.path(),
            "song.m4a",
            &["-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:a", "aac"],
        ) else {
            return;
        };
        let bytes: Arc<[u8]> = Arc::from(std::fs::read(&path).unwrap());
        let source = MediaSource::Blob {
            bytes,
            oid: "abc".into(),
        };
        // Hinted as video (e.g. a `.mp4` extension) but really audio-only.
        let decoded = decode_side(MediaKind::Video, &source, "song.mp4").unwrap();
        assert!(matches!(decoded, DecodedMedia::Audio(_)), "{decoded:?}");
    }

    #[test]
    fn exotic_still_image_transcodes_through_ffmpeg() {
        let dir = tempfile::tempdir().unwrap();
        // PPM is handled natively; use a format `image` can't read: JPEG 2000
        // may not be built in, so fall back to a raw YUV-tagged y4m? Use
        // PSD-like via ffmpeg's "xbm"? Simplest cross-build choice: SGI RGB.
        let Some(path) = synth(
            dir.path(),
            "frame.sgi",
            &["-f", "lavfi", "-i", "testsrc=size=64x48:rate=1", "-frames:v", "1"],
        ) else {
            return;
        };
        let decoded = decode_side(MediaKind::Image, &workdir_source(&path), "frame.sgi").unwrap();
        let DecodedMedia::Image(img) = decoded else {
            panic!("expected image");
        };
        assert_eq!((img.width, img.height), (64, 48));
        assert_eq!(img.info.decoder, "ffmpeg");
    }

    #[test]
    fn garbage_bytes_with_image_extension_are_unrecognized() {
        let src = MediaSource::Blob {
            bytes: Arc::from(b"definitely not an image".as_slice()),
            oid: "x".into(),
        };
        let err = decode_side(MediaKind::Image, &src, "a.png").unwrap_err();
        assert!(matches!(
            err,
            MediaError::Unrecognized | MediaError::Corrupt(_) | MediaError::Unsupported(_)
        ));
    }
}

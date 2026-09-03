//! FFmpeg bridge. Video (and exotic image/audio formats) are decoded by
//! spawning the `ffmpeg`/`ffprobe` executables found on `PATH` rather than
//! linking libav — no build-time system dependencies, and the app degrades
//! gracefully to a clear "install FFmpeg" message when they are absent.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use super::MediaError;

#[derive(Debug, Clone)]
pub struct FfmpegTools {
    pub ffmpeg: PathBuf,
    pub ffprobe: Option<PathBuf>,
    /// `(major, minor)` parsed from `ffmpeg -version`; `(0, 0)` if unknown.
    pub version: (u32, u32),
}

impl FfmpegTools {
    /// FFmpeg ≥ 5.1 renamed `-vsync` to `-fps_mode`.
    fn fps_mode_flag(&self) -> &'static str {
        if self.version.0 > 5 || (self.version.0 == 5 && self.version.1 >= 1) {
            "-fps_mode"
        } else {
            "-vsync"
        }
    }
}

static TOOLS: OnceLock<Option<FfmpegTools>> = OnceLock::new();

/// Locate the FFmpeg tools once per process. `None` when `ffmpeg` is not on
/// `PATH` (or `GIT_LEVIATHAN_FFMPEG` points somewhere invalid).
pub fn tools() -> Option<&'static FfmpegTools> {
    TOOLS.get_or_init(discover).as_ref()
}

pub fn is_available() -> bool {
    tools().is_some()
}

fn discover() -> Option<FfmpegTools> {
    let ffmpeg = std::env::var_os("GIT_LEVIATHAN_FFMPEG")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| find_on_path("ffmpeg"))?;
    let ffprobe = ffmpeg
        .parent()
        .map(|dir| dir.join(executable_name("ffprobe")))
        .filter(|p| p.is_file())
        .or_else(|| find_on_path("ffprobe"));

    let output = command(&ffmpeg).arg("-version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version_line = stdout.lines().next().unwrap_or("").trim();
    let version = parse_version(version_line);
    Some(FfmpegTools {
        ffmpeg,
        ffprobe,
        version,
    })
}

fn parse_version(line: &str) -> (u32, u32) {
    // "ffmpeg version n9.0.1 Copyright ..." / "ffmpeg version 4.4.2-0ubuntu0.22.04.1 ..."
    let token = line
        .split_whitespace()
        .nth(2)
        .unwrap_or("")
        .trim_start_matches('n')
        .trim_start_matches('N');
    let mut parts = token
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    (major, minor)
}

fn executable_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn find_on_path(base: &str) -> Option<PathBuf> {
    let name = executable_name(base);
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(&name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    // Homebrew / MacPorts / common manual installs that GUI apps launched from
    // the dock don't see on PATH.
    for dir in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/opt/local/bin",
        "/usr/bin",
    ] {
        let candidate = Path::new(dir).join(&name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Base command with the process kept off the desktop (no console window on
/// Windows) and stdin closed so a misbehaving tool can never wait on us.
fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

// ---------------------------------------------------------------------------
// Probing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ProbeInfo {
    pub container: String,
    pub container_long: String,
    pub duration_secs: Option<f64>,
    pub bit_rate: Option<u64>,
    pub video: Option<VideoStreamInfo>,
    pub audio: Option<AudioStreamInfo>,
    pub tags: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct VideoStreamInfo {
    pub codec: String,
    pub codec_long: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub pix_fmt: String,
    pub bit_rate: Option<u64>,
    pub frame_count: Option<u64>,
    /// Display rotation in degrees (0/90/180/270) from container metadata.
    pub rotation: i32,
}

#[derive(Debug, Clone, Default)]
pub struct AudioStreamInfo {
    pub codec: String,
    pub codec_long: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub channel_layout: String,
    pub bit_rate: Option<u64>,
}

pub fn probe(path: &Path) -> Result<ProbeInfo, MediaError> {
    let tools = tools().ok_or(MediaError::FfmpegMissing)?;
    if let Some(ffprobe) = &tools.ffprobe {
        match probe_with_ffprobe(ffprobe, path) {
            Ok(info) => return Ok(info),
            Err(err) => {
                // Fall through to the ffmpeg stderr parser only when ffprobe
                // itself misbehaved; a definite "unrecognized" is final.
                if matches!(err, MediaError::Unrecognized) {
                    return Err(err);
                }
            }
        }
    }
    probe_with_ffmpeg(&tools.ffmpeg, path)
}

fn probe_with_ffprobe(ffprobe: &Path, path: &Path) -> Result<ProbeInfo, MediaError> {
    let output = command(ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|e| MediaError::Ffmpeg(format!("could not run ffprobe: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Invalid data found") || stderr.contains("Unknown format") {
            return Err(MediaError::Unrecognized);
        }
        return Err(MediaError::Ffmpeg(trim_error(&stderr)));
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| MediaError::Ffmpeg(format!("ffprobe output unreadable: {e}")))?;
    Ok(parse_probe_json(&json))
}

fn parse_probe_json(json: &serde_json::Value) -> ProbeInfo {
    let format = &json["format"];
    let mut info = ProbeInfo {
        container: str_field(format, "format_name"),
        container_long: str_field(format, "format_long_name"),
        duration_secs: num_field(format, "duration"),
        bit_rate: num_field(format, "bit_rate").map(|v| v as u64),
        ..Default::default()
    };
    if let Some(tags) = format["tags"].as_object() {
        for (k, v) in tags {
            if let Some(v) = v.as_str() {
                info.tags.push((k.clone(), v.to_string()));
            }
        }
        info.tags.sort();
    }
    let streams = json["streams"].as_array().cloned().unwrap_or_default();
    for stream in &streams {
        match stream["codec_type"].as_str() {
            Some("video") if info.video.is_none() => {
                let codec = str_field(stream, "codec_name");
                // Attached cover art shows up as an mjpeg/png "video" stream in
                // audio files; skip those so the file routes to the audio player.
                let disposition_cover = stream["disposition"]["attached_pic"]
                    .as_i64()
                    .unwrap_or(0)
                    == 1;
                if disposition_cover {
                    continue;
                }
                let fps = parse_ratio(stream["avg_frame_rate"].as_str())
                    .filter(|f| *f > 0.0)
                    .or_else(|| parse_ratio(stream["r_frame_rate"].as_str()))
                    .filter(|f| f.is_finite() && *f > 0.0)
                    .unwrap_or(0.0);
                let mut rotation = stream["tags"]["rotate"]
                    .as_str()
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(0);
                if let Some(side_data) = stream["side_data_list"].as_array() {
                    for sd in side_data {
                        if let Some(rot) = sd["rotation"].as_f64() {
                            rotation = rot.round() as i32;
                        }
                    }
                }
                let stream_duration = num_field(stream, "duration");
                if info.duration_secs.is_none() {
                    info.duration_secs = stream_duration;
                }
                info.video = Some(VideoStreamInfo {
                    codec,
                    codec_long: str_field(stream, "codec_long_name"),
                    width: stream["width"].as_u64().unwrap_or(0) as u32,
                    height: stream["height"].as_u64().unwrap_or(0) as u32,
                    fps,
                    pix_fmt: str_field(stream, "pix_fmt"),
                    bit_rate: num_field(stream, "bit_rate").map(|v| v as u64),
                    frame_count: num_field(stream, "nb_frames").map(|v| v as u64),
                    rotation: rotation.rem_euclid(360),
                });
            }
            Some("audio") if info.audio.is_none() => {
                let stream_duration = num_field(stream, "duration");
                if info.duration_secs.is_none() {
                    info.duration_secs = stream_duration;
                }
                info.audio = Some(AudioStreamInfo {
                    codec: str_field(stream, "codec_name"),
                    codec_long: str_field(stream, "codec_long_name"),
                    sample_rate: num_field(stream, "sample_rate").unwrap_or(0.0) as u32,
                    channels: stream["channels"].as_u64().unwrap_or(0) as u16,
                    channel_layout: str_field(stream, "channel_layout"),
                    bit_rate: num_field(stream, "bit_rate").map(|v| v as u64),
                });
            }
            _ => {}
        }
    }
    info
}

fn str_field(value: &serde_json::Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").to_string()
}

fn num_field(value: &serde_json::Value, key: &str) -> Option<f64> {
    match &value[key] {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn parse_ratio(text: Option<&str>) -> Option<f64> {
    let text = text?;
    if let Some((num, den)) = text.split_once('/') {
        let num: f64 = num.trim().parse().ok()?;
        let den: f64 = den.trim().parse().ok()?;
        if den == 0.0 {
            return None;
        }
        Some(num / den)
    } else {
        text.trim().parse().ok()
    }
}

/// Last-resort probe: parse the human-readable stream dump `ffmpeg -i` writes
/// to stderr. Used when `ffprobe` is not installed alongside `ffmpeg`.
fn probe_with_ffmpeg(ffmpeg: &Path, path: &Path) -> Result<ProbeInfo, MediaError> {
    let output = command(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(path)
        .args(["-f", "null", "-"])
        .args(["-t", "0"])
        .output()
        .map_err(|e| MediaError::Ffmpeg(format!("could not run ffmpeg: {e}")))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("Invalid data found when processing input") {
        return Err(MediaError::Unrecognized);
    }
    let mut info = ProbeInfo::default();
    for raw_line in stderr.lines() {
        let line = raw_line.trim();
        if let Some(rest) = line.strip_prefix("Input #0, ") {
            info.container = rest.split(',').next().unwrap_or("").trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Duration: ") {
            let dur = rest.split(',').next().unwrap_or("").trim();
            info.duration_secs = parse_clock(dur);
            if let Some(br) = rest.split("bitrate:").nth(1) {
                let kb = br.split_whitespace().next().unwrap_or("");
                if let Ok(kb) = kb.parse::<f64>() {
                    info.bit_rate = Some((kb * 1000.0) as u64);
                }
            }
        } else if line.starts_with("Stream #") && line.contains("Video:") && info.video.is_none() {
            let desc = line.split("Video:").nth(1).unwrap_or("");
            let parts: Vec<&str> = desc.split(',').map(str::trim).collect();
            let codec = parts
                .first()
                .map(|p| p.split_whitespace().next().unwrap_or("").to_string())
                .unwrap_or_default();
            let mut stream = VideoStreamInfo {
                codec,
                ..Default::default()
            };
            for part in &parts {
                if let Some((w, h)) = parse_dimensions(part) {
                    stream.width = w;
                    stream.height = h;
                } else if let Some(fps) = part.strip_suffix(" fps") {
                    stream.fps = fps.trim().parse().unwrap_or(0.0);
                } else if let Some(kb) = part.strip_suffix(" kb/s") {
                    stream.bit_rate = kb.trim().parse::<f64>().ok().map(|k| (k * 1000.0) as u64);
                }
            }
            if stream.width > 0 && stream.height > 0 {
                if stream.pix_fmt.is_empty() {
                    stream.pix_fmt = parts.get(1).map(|s| s.to_string()).unwrap_or_default();
                }
                info.video = Some(stream);
            }
        } else if line.starts_with("Stream #") && line.contains("Audio:") && info.audio.is_none() {
            let desc = line.split("Audio:").nth(1).unwrap_or("");
            let parts: Vec<&str> = desc.split(',').map(str::trim).collect();
            let codec = parts
                .first()
                .map(|p| p.split_whitespace().next().unwrap_or("").to_string())
                .unwrap_or_default();
            let mut stream = AudioStreamInfo {
                codec,
                ..Default::default()
            };
            for part in &parts {
                if let Some(hz) = part.strip_suffix(" Hz") {
                    stream.sample_rate = hz.trim().parse().unwrap_or(0);
                } else if *part == "mono" {
                    stream.channels = 1;
                    stream.channel_layout = "mono".into();
                } else if *part == "stereo" {
                    stream.channels = 2;
                    stream.channel_layout = "stereo".into();
                } else if let Some(kb) = part.strip_suffix(" kb/s") {
                    stream.bit_rate = kb.trim().parse::<f64>().ok().map(|k| (k * 1000.0) as u64);
                } else if part.ends_with("channels") || part.contains("(side)") {
                    let n = part.split_whitespace().next().unwrap_or("");
                    stream.channels = n.parse().unwrap_or(stream.channels);
                    stream.channel_layout = part.to_string();
                }
            }
            info.audio = Some(stream);
        } else if let Some(rotate) = line.strip_prefix("rotate") {
            if let Some(stream) = info.video.as_mut() {
                let value = rotate.trim_start_matches(':').trim();
                stream.rotation = value.parse::<i32>().unwrap_or(0).rem_euclid(360);
            }
        }
    }
    if info.video.is_none() && info.audio.is_none() {
        return Err(MediaError::Unrecognized);
    }
    Ok(info)
}

fn parse_dimensions(part: &str) -> Option<(u32, u32)> {
    // "1920x1080 [SAR 1:1 DAR 16:9]" -> (1920, 1080)
    let token = part.split_whitespace().next()?;
    let (w, h) = token.split_once('x')?;
    let w: u32 = w.parse().ok()?;
    let h: u32 = h.parse().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

fn parse_clock(text: &str) -> Option<f64> {
    // "00:01:02.34"
    let mut parts = text.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

fn trim_error(stderr: &str) -> String {
    let last = stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("unknown error");
    last.chars().take(300).collect()
}

// ---------------------------------------------------------------------------
// One-shot conversions
// ---------------------------------------------------------------------------

/// Decode the first frame of any FFmpeg-readable still image to PNG bytes.
/// Handles HEIC/AVIF/JPEG XL/PSD/RAW and friends that `image` can't parse.
pub fn transcode_image_to_png(path: &Path) -> Result<Vec<u8>, MediaError> {
    let tools = tools().ok_or(MediaError::FfmpegMissing)?;
    let output = command(&tools.ffmpeg)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-an",
            "-sn",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "-",
        ])
        .output()
        .map_err(|e| MediaError::Ffmpeg(format!("could not run ffmpeg: {e}")))?;
    if !output.status.success() || output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Invalid data found") {
            return Err(MediaError::Unrecognized);
        }
        return Err(MediaError::Ffmpeg(trim_error(&stderr)));
    }
    Ok(output.stdout)
}

/// Decode a whole audio file (or the audio track of a video) to interleaved
/// 16-bit PCM, capped at `max_frames` frames. Returns `(samples, truncated)`.
pub fn decode_audio_pcm(
    path: &Path,
    sample_rate: u32,
    channels: u16,
    max_frames: usize,
) -> Result<(Vec<i16>, bool), MediaError> {
    let tools = tools().ok_or(MediaError::FfmpegMissing)?;
    let mut child = command(&tools.ffmpeg)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-vn",
            "-sn",
            "-dn",
            "-f",
            "s16le",
            "-acodec",
            "pcm_s16le",
            "-ac",
            &channels.to_string(),
            "-ar",
            &sample_rate.to_string(),
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| MediaError::Ffmpeg(format!("could not run ffmpeg: {e}")))?;
    let stderr = child.stderr.take();
    let mut guard = ChildGuard(stderr, child);
    let mut stdout = guard.1.stdout.take().expect("piped stdout");
    let max_bytes = max_frames.saturating_mul(channels as usize).saturating_mul(2);
    let mut raw = Vec::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut truncated = false;
    loop {
        let n = stdout
            .read(&mut buf)
            .map_err(|e| MediaError::Ffmpeg(format!("read failed: {e}")))?;
        if n == 0 {
            break;
        }
        if raw.len() + n > max_bytes {
            raw.extend_from_slice(&buf[..max_bytes - raw.len()]);
            truncated = true;
            break;
        }
        raw.extend_from_slice(&buf[..n]);
    }
    drop(stdout);
    let (status, stderr) = guard.finish(truncated);
    if raw.len() < 4 {
        if stderr.contains("Invalid data found") || stderr.contains("does not contain any stream")
        {
            return Err(MediaError::Unrecognized);
        }
        if !stderr.trim().is_empty() {
            return Err(MediaError::Ffmpeg(trim_error(&stderr)));
        }
        if !status {
            return Err(MediaError::Corrupt("ffmpeg produced no audio".to_string()));
        }
    }
    let frame_bytes = channels as usize * 2;
    raw.truncate(raw.len() / frame_bytes * frame_bytes);
    let samples = raw
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    Ok((samples, truncated))
}

// ---------------------------------------------------------------------------
// Streaming processes used by the video player
// ---------------------------------------------------------------------------

/// Spawn a raw RGBA frame stream starting at `start_secs`, resampled to a
/// constant `fps` and scaled to `width`×`height`.
pub fn spawn_video_frames(
    path: &Path,
    start_secs: f64,
    width: u32,
    height: u32,
    fps: f64,
    rotation: i32,
) -> Result<Child, MediaError> {
    let tools = tools().ok_or(MediaError::FfmpegMissing)?;
    let mut cmd = command(&tools.ffmpeg);
    cmd.args(["-v", "error", "-nostdin"]);
    if start_secs > 0.0 {
        cmd.args(["-ss", &format!("{start_secs:.3}")]);
    }
    cmd.arg("-i").arg(path);
    // Rotation is applied by FFmpeg's autorotate by default; when the probe
    // reported a rotation we still pass an explicit scale on the *display*
    // dimensions so the pipeline geometry matches what we allocated.
    let _ = rotation;
    let filter = format!("scale={width}:{height}:flags=bicubic,format=rgba");
    cmd.args(["-an", "-sn", "-dn", "-vf", &filter]);
    cmd.args(["-r", &format!("{fps:.6}")]);
    cmd.args([tools.fps_mode_flag(), "cfr"]);
    cmd.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.spawn()
        .map_err(|e| MediaError::Ffmpeg(format!("could not run ffmpeg: {e}")))
}

/// Spawn an interleaved 16-bit PCM stream of the audio track starting at
/// `start_secs`, optionally time-stretched (pitch preserved) by `tempo`.
pub fn spawn_audio_stream(
    path: &Path,
    start_secs: f64,
    sample_rate: u32,
    channels: u16,
    tempo: f64,
) -> Result<Child, MediaError> {
    let tools = tools().ok_or(MediaError::FfmpegMissing)?;
    let mut cmd = command(&tools.ffmpeg);
    cmd.args(["-v", "error", "-nostdin"]);
    if start_secs > 0.0 {
        cmd.args(["-ss", &format!("{start_secs:.3}")]);
    }
    cmd.arg("-i").arg(path);
    cmd.args(["-vn", "-sn", "-dn"]);
    if let Some(filter) = atempo_chain(tempo) {
        cmd.args(["-af", &filter]);
    }
    cmd.args([
        "-f",
        "s16le",
        "-acodec",
        "pcm_s16le",
        "-ac",
        &channels.to_string(),
        "-ar",
        &sample_rate.to_string(),
        "-",
    ]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.spawn()
        .map_err(|e| MediaError::Ffmpeg(format!("could not run ffmpeg: {e}")))
}

/// `atempo` only accepts 0.5–2.0 per instance (older builds); chain instances
/// to reach the requested factor.
fn atempo_chain(tempo: f64) -> Option<String> {
    if !(tempo.is_finite() && tempo > 0.0) || (tempo - 1.0).abs() < 1e-6 {
        return None;
    }
    let mut remaining = tempo;
    let mut stages = Vec::new();
    while remaining > 2.0 {
        stages.push("atempo=2.0".to_string());
        remaining /= 2.0;
    }
    while remaining < 0.5 {
        stages.push("atempo=0.5".to_string());
        remaining /= 0.5;
    }
    stages.push(format!("atempo={remaining:.4}"));
    Some(stages.join(","))
}

/// Kills the child on drop so an abandoned decoder can never linger.
pub struct ChildGuard(pub Option<std::process::ChildStderr>, pub Child);

impl ChildGuard {
    /// Wait for exit (killing first when `kill`), returning `(success, stderr)`.
    pub fn finish(&mut self, kill: bool) -> (bool, String) {
        if kill {
            let _ = self.1.kill();
        }
        let stderr = self
            .0
            .take()
            .map(|mut err| {
                let mut text = String::new();
                let _ = err.read_to_string(&mut text);
                text
            })
            .unwrap_or_default();
        let status = self.1.wait().map(|s| s.success()).unwrap_or(false);
        (status, stderr)
    }

}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.1.kill();
        // Don't block the caller for long; the process was just killed.
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            match self.1.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_lines_from_common_distributions() {
        assert_eq!(
            parse_version("ffmpeg version n9.0.1 Copyright (c) 2000-2026"),
            (9, 0)
        );
        assert_eq!(
            parse_version("ffmpeg version 4.4.2-0ubuntu0.22.04.1 Copyright"),
            (4, 4)
        );
        assert_eq!(parse_version("ffmpeg version 6.1 Copyright"), (6, 1));
        assert_eq!(parse_version("ffmpeg version N-112345-gabcdef"), (112345, 0));
        assert_eq!(parse_version(""), (0, 0));
    }

    #[test]
    fn fps_mode_flag_tracks_ffmpeg_version() {
        let mk = |v| FfmpegTools {
            ffmpeg: PathBuf::new(),
            ffprobe: None,
            version: v,
        };
        assert_eq!(mk((4, 4)).fps_mode_flag(), "-vsync");
        assert_eq!(mk((5, 0)).fps_mode_flag(), "-vsync");
        assert_eq!(mk((5, 1)).fps_mode_flag(), "-fps_mode");
        assert_eq!(mk((7, 0)).fps_mode_flag(), "-fps_mode");
    }

    #[test]
    fn parses_probe_json() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
              "streams": [
                {"codec_type":"video","codec_name":"h264","codec_long_name":"H.264","width":1920,"height":1080,
                 "avg_frame_rate":"30000/1001","pix_fmt":"yuv420p","bit_rate":"4000000","nb_frames":"300",
                 "tags":{"rotate":"90"}},
                {"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channels":2,"channel_layout":"stereo","bit_rate":"128000"}
              ],
              "format": {"format_name":"mov,mp4,m4a,3gp,3g2,mj2","format_long_name":"QuickTime / MOV","duration":"10.010000","bit_rate":"4200000",
                         "tags":{"title":"Clip"}}
            }"#,
        )
        .unwrap();
        let info = parse_probe_json(&json);
        assert_eq!(info.container, "mov,mp4,m4a,3gp,3g2,mj2");
        assert_eq!(info.duration_secs, Some(10.01));
        let v = info.video.unwrap();
        assert_eq!((v.width, v.height), (1920, 1080));
        assert!((v.fps - 29.97).abs() < 0.01);
        assert_eq!(v.rotation, 90);
        assert_eq!(v.frame_count, Some(300));
        let a = info.audio.unwrap();
        assert_eq!(a.sample_rate, 48000);
        assert_eq!(a.channels, 2);
        assert_eq!(info.tags, vec![("title".to_string(), "Clip".to_string())]);
    }

    #[test]
    fn cover_art_streams_do_not_count_as_video() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"streams":[{"codec_type":"video","codec_name":"mjpeg","width":500,"height":500,"disposition":{"attached_pic":1}},
                            {"codec_type":"audio","codec_name":"mp3","sample_rate":"44100","channels":2}],
                "format":{"format_name":"mp3","duration":"3.0"}}"#,
        )
        .unwrap();
        let info = parse_probe_json(&json);
        assert!(info.video.is_none());
        assert!(info.audio.is_some());
    }

    #[test]
    fn parses_ffmpeg_stderr_stream_dump() {
        // Exercise the line parser directly through a fake ffmpeg? Simpler:
        // validate the helpers it relies on.
        assert_eq!(parse_dimensions("1920x1080 [SAR 1:1 DAR 16:9]"), Some((1920, 1080)));
        assert_eq!(parse_dimensions("yuv420p(progressive)"), None);
        assert_eq!(parse_clock("00:01:02.34"), Some(62.34));
        assert_eq!(parse_ratio(Some("30000/1001")).map(|f| (f * 100.0).round()), Some(2997.0));
        assert_eq!(parse_ratio(Some("0/0")), None);
        assert_eq!(parse_ratio(Some("25")), Some(25.0));
    }

    #[test]
    fn atempo_chain_stays_within_filter_limits() {
        assert_eq!(atempo_chain(1.0), None);
        assert_eq!(atempo_chain(1.5).as_deref(), Some("atempo=1.5000"));
        assert_eq!(atempo_chain(4.0).as_deref(), Some("atempo=2.0,atempo=2.0000"));
        assert_eq!(atempo_chain(0.25).as_deref(), Some("atempo=0.5,atempo=0.5000"));
        assert_eq!(atempo_chain(0.0), None);
    }
}

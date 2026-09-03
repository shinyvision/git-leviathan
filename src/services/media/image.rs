//! Image decoding for the media diff: raster formats through the `image`
//! crate (with animation for GIF / APNG / WebP), SVG through `usvg`/`resvg`,
//! and everything else (HEIC, AVIF, JPEG XL, PSD, camera RAW…) through an
//! FFmpeg transcode to PNG. EXIF is read where present and the orientation
//! tag is baked into the displayed pixels.

use std::io::Cursor;
use bytes::Bytes;
use iced::widget::image::Handle;
use image::{AnimationDecoder, DynamicImage, GenericImageView, ImageDecoder, ImageFormat};

use super::{format_bytes, MediaError};

/// Longest side we hand to the GPU. Anything larger is downscaled for display
/// (the original dimensions are still reported).
pub const MAX_DISPLAY_DIMENSION: u32 = 8192;
/// Animated images above this per-frame pixel count are decoded at reduced
/// resolution to keep hundreds of frames affordable.
const MAX_ANIMATION_FRAME_DIMENSION: u32 = 2048;
/// Total RGBA bytes kept for an animation before we stop decoding frames.
const MAX_ANIMATION_BYTES: usize = 768 * 1024 * 1024;
const MAX_ANIMATION_FRAMES: usize = 4000;
/// SVG rasterization size used for the pixel-difference mode.
const SVG_RASTER_MAX_DIMENSION: u32 = 4096;

#[derive(Clone)]
pub struct ImageFrame {
    pub handle: Handle,
    /// Shared RGBA pixels (same buffer the handle references).
    pub rgba: Bytes,
    pub delay_ms: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ImageInfo {
    pub format: String,
    pub color: String,
    pub bits_per_channel: Option<u16>,
    pub has_alpha: bool,
    pub file_size: u64,
    pub original_width: u32,
    pub original_height: u32,
    pub icc_profile: bool,
    pub orientation: Option<String>,
    pub downscaled: bool,
    pub frame_count: usize,
    pub frames_truncated: bool,
    pub decoder: &'static str,
    pub dpi: Option<(f64, f64)>,
    /// Curated EXIF fields first (camera, exposure…), then the rest.
    pub exif: Vec<(String, String)>,
}

pub struct DecodedImage {
    /// Display dimensions (after orientation and any downscale).
    pub width: u32,
    pub height: u32,
    pub frames: Vec<ImageFrame>,
    /// Sum of frame delays; 0 for stills.
    pub total_duration_ms: u32,
    /// Vector source, drawn natively at any zoom. `frames[0]` still holds a
    /// rasterization for pixel comparisons.
    pub svg: Option<iced::widget::svg::Handle>,
    pub info: ImageInfo,
}

impl DecodedImage {
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Frame shown at `phase_ms` into the loop.
    pub fn frame_index_at(&self, phase_ms: u32) -> usize {
        let phase = if self.total_duration_ms == 0 {
            0
        } else {
            phase_ms % self.total_duration_ms
        };
        let mut acc = 0u32;
        for (idx, frame) in self.frames.iter().enumerate() {
            acc = acc.saturating_add(frame.delay_ms);
            if phase < acc {
                return idx;
            }
        }
        self.frames.len().saturating_sub(1)
    }

    /// Loop time at which frame `idx` starts.
    pub fn frame_start_ms(&self, idx: usize) -> u32 {
        self.frames
            .iter()
            .take(idx)
            .fold(0u32, |acc, f| acc.saturating_add(f.delay_ms))
    }

    /// Milliseconds until the next frame boundary after `phase_ms`.
    pub fn ms_until_next_frame(&self, phase_ms: u32) -> u32 {
        if self.total_duration_ms == 0 {
            return u32::MAX;
        }
        let phase = phase_ms % self.total_duration_ms;
        let mut acc = 0u32;
        for frame in &self.frames {
            acc = acc.saturating_add(frame.delay_ms);
            if phase < acc {
                return (acc - phase).max(1);
            }
        }
        1
    }

    pub fn megapixels(&self) -> f64 {
        (self.info.original_width as f64 * self.info.original_height as f64) / 1_000_000.0
    }

    /// Key/value rows for the info panel. Order is stable so the two sides
    /// of a diff line up.
    pub fn properties(&self) -> Vec<(String, String)> {
        let info = &self.info;
        let mut rows = vec![
            (
                "Dimensions".to_string(),
                format!("{} × {}", info.original_width, info.original_height),
            ),
            ("Megapixels".to_string(), format!("{:.2} MP", self.megapixels())),
            (
                "Aspect ratio".to_string(),
                aspect_ratio_label(info.original_width, info.original_height),
            ),
            ("Format".to_string(), info.format.clone()),
            ("Color".to_string(), info.color.clone()),
        ];
        if let Some(bits) = info.bits_per_channel {
            rows.push(("Bit depth".to_string(), format!("{bits}-bit per channel")));
        }
        rows.push((
            "Alpha channel".to_string(),
            if info.has_alpha { "Yes" } else { "No" }.to_string(),
        ));
        rows.push(("File size".to_string(), format_bytes(info.file_size)));
        if self.frames.len() > 1 || info.frames_truncated {
            rows.push((
                "Frames".to_string(),
                if info.frames_truncated {
                    format!("{}+ (preview truncated)", self.frames.len())
                } else {
                    self.frames.len().to_string()
                },
            ));
            rows.push((
                "Loop duration".to_string(),
                format!("{:.2} s", self.total_duration_ms as f64 / 1000.0),
            ));
        }
        if let Some((x, y)) = info.dpi {
            if (x - y).abs() < 0.01 {
                rows.push(("Resolution".to_string(), format!("{x:.0} dpi")));
            } else {
                rows.push(("Resolution".to_string(), format!("{x:.0} × {y:.0} dpi")));
            }
        }
        rows.push((
            "ICC profile".to_string(),
            if info.icc_profile { "Embedded" } else { "None" }.to_string(),
        ));
        if let Some(orientation) = &info.orientation {
            rows.push(("Orientation".to_string(), orientation.clone()));
        }
        if info.downscaled {
            rows.push((
                "Preview".to_string(),
                format!("Downscaled to {} × {}", self.width, self.height),
            ));
        }
        rows.push(("Decoder".to_string(), info.decoder.to_string()));
        for (k, v) in &info.exif {
            rows.push((k.clone(), v.clone()));
        }
        rows
    }
}

impl std::fmt::Debug for DecodedImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodedImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("frames", &self.frames.len())
            .field("svg", &self.svg.is_some())
            .field("info", &self.info)
            .finish()
    }
}

fn aspect_ratio_label(w: u32, h: u32) -> String {
    if w == 0 || h == 0 {
        return "—".to_string();
    }
    let g = gcd(w, h);
    let (rw, rh) = (w / g, h / g);
    if rw <= 64 && rh <= 64 {
        format!("{rw}:{rh}")
    } else {
        format!("{:.3}:1", w as f64 / h as f64)
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a.max(1)
    } else {
        gcd(b, a % b)
    }
}

// ---------------------------------------------------------------------------
// Decoding entry point
// ---------------------------------------------------------------------------

pub fn decode_image(bytes: &[u8], file_path: &str) -> Result<DecodedImage, MediaError> {
    if super::detect::sniff_media_kind(bytes) == Some(super::MediaKind::Image)
        && looks_like_svg_bytes(bytes)
    {
        return decode_svg(bytes);
    }

    let format = image::guess_format(bytes).ok().or_else(|| {
        ImageFormat::from_path(file_path)
            .ok()
            .filter(|f| f.can_read())
    });

    match format {
        Some(format) => match decode_with_image_crate(bytes, format, file_path) {
            Ok(decoded) => Ok(decoded),
            Err(err) => {
                // A recognized-but-broken container is only "corrupt" if
                // FFmpeg can't salvage it either (e.g. odd TIFF/HEIF variants).
                match decode_with_ffmpeg(bytes, file_path) {
                    Ok(decoded) => Ok(decoded),
                    Err(MediaError::FfmpegMissing) | Err(MediaError::Unrecognized) => Err(err),
                    Err(other) => Err(other),
                }
            }
        },
        None => {
            if bytes.is_empty() {
                return Err(MediaError::Corrupt("file is empty".to_string()));
            }
            match decode_with_ffmpeg(bytes, file_path) {
                Ok(decoded) => Ok(decoded),
                Err(MediaError::FfmpegMissing) => {
                    if super::detect::sniff_media_kind(bytes) == Some(super::MediaKind::Image) {
                        Err(MediaError::FfmpegMissing)
                    } else {
                        Err(MediaError::Unrecognized)
                    }
                }
                Err(err) => Err(err),
            }
        }
    }
}

fn looks_like_svg_bytes(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    trimmed.starts_with('<') && trimmed.contains("<svg")
}

fn decode_with_image_crate(
    bytes: &[u8],
    format: ImageFormat,
    _file_path: &str,
) -> Result<DecodedImage, MediaError> {
    let map_err = |e: image::ImageError| match e {
        image::ImageError::Unsupported(u) => MediaError::Unsupported(u.to_string()),
        image::ImageError::Decoding(d) => MediaError::Corrupt(d.to_string()),
        image::ImageError::Limits(l) => MediaError::Corrupt(l.to_string()),
        other => MediaError::Corrupt(other.to_string()),
    };

    // Header-only pass for metadata (orientation, ICC, native color type).
    let mut meta_decoder = image::ImageReader::with_format(Cursor::new(bytes), format)
        .into_decoder()
        .map_err(map_err)?;
    let (orig_w, orig_h) = meta_decoder.dimensions();
    let orientation = meta_decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let icc_profile = meta_decoder
        .icc_profile()
        .ok()
        .flatten()
        .is_some_and(|p| !p.is_empty());
    let native_color = meta_decoder.original_color_type();
    let (color_label, bits, has_alpha) = describe_color(native_color);
    drop(meta_decoder);

    let mut info = ImageInfo {
        format: format_label(format),
        color: color_label,
        bits_per_channel: bits,
        has_alpha,
        file_size: bytes.len() as u64,
        original_width: orig_w,
        original_height: orig_h,
        icc_profile,
        orientation: describe_orientation(orientation),
        decoder: "image",
        ..Default::default()
    };
    let exif = read_exif(bytes);
    info.dpi = exif.dpi;
    info.exif = exif.fields;

    let animated_frames = decode_animation_frames(bytes, format).map_err(map_err)?;
    let (frames, frames_truncated, w, h, downscaled) = match animated_frames {
        Some(raw_frames) if raw_frames.frames.len() > 1 => {
            let mut frames = Vec::with_capacity(raw_frames.frames.len());
            let mut w = 0;
            let mut h = 0;
            for (img, delay_ms) in raw_frames.frames {
                let (img, _) = fit_for_display(img, MAX_ANIMATION_FRAME_DIMENSION);
                w = img.width();
                h = img.height();
                frames.push(frame_from_rgba(img, delay_ms));
            }
            (
                frames,
                raw_frames.truncated,
                w,
                h,
                w < orig_w || h < orig_h,
            )
        }
        _ => {
            let decoder = image::ImageReader::with_format(Cursor::new(bytes), format)
                .into_decoder()
                .map_err(map_err)?;
            let mut img = DynamicImage::from_decoder(decoder).map_err(map_err)?;
            img.apply_orientation(orientation);
            let (img, downscaled) = fit_for_display(img, MAX_DISPLAY_DIMENSION);
            let (w, h) = img.dimensions();
            (vec![frame_from_rgba(img, 0)], false, w, h, downscaled)
        }
    };

    // Orientation swaps reported dimensions for 90°/270° rotations.
    if orientation_swaps_axes(orientation) && frames.len() == 1 {
        info.original_width = orig_h;
        info.original_height = orig_w;
    }
    info.downscaled = downscaled;
    info.frame_count = frames.len();
    info.frames_truncated = frames_truncated;
    let total_duration_ms = if frames.len() > 1 {
        frames.iter().map(|f| f.delay_ms).sum::<u32>().max(1)
    } else {
        0
    };

    Ok(DecodedImage {
        width: w,
        height: h,
        frames,
        total_duration_ms,
        svg: None,
        info,
    })
}

struct AnimationFrames {
    frames: Vec<(DynamicImage, u32)>,
    truncated: bool,
}

/// Returns `Some` for animated containers (GIF, APNG, animated WebP) and
/// `None` for stills so the caller takes the single-frame path.
fn decode_animation_frames(
    bytes: &[u8],
    format: ImageFormat,
) -> image::ImageResult<Option<AnimationFrames>> {
    let cursor = Cursor::new(bytes);
    let frames: image::Frames<'_> = match format {
        ImageFormat::Gif => {
            let decoder = image::codecs::gif::GifDecoder::new(cursor)?;
            decoder.into_frames()
        }
        ImageFormat::WebP => {
            let decoder = image::codecs::webp::WebPDecoder::new(cursor)?;
            if !decoder.has_animation() {
                return Ok(None);
            }
            decoder.into_frames()
        }
        ImageFormat::Png => {
            let decoder = image::codecs::png::PngDecoder::new(cursor)?;
            if !decoder.is_apng()? {
                return Ok(None);
            }
            decoder.apng()?.into_frames()
        }
        _ => return Ok(None),
    };

    let mut out = Vec::new();
    let mut bytes_used = 0usize;
    let mut truncated = false;
    for frame in frames {
        let frame = match frame {
            Ok(frame) => frame,
            Err(err) => {
                // A truncated animation still shows what decoded so far.
                if out.is_empty() {
                    return Err(err);
                }
                truncated = true;
                break;
            }
        };
        let (num, den) = frame.delay().numer_denom_ms();
        let delay_ms = ((num as f64 / den.max(1) as f64).round() as u32).max(10);
        let buffer = frame.into_buffer();
        let frame_bytes = (buffer.width() as usize) * (buffer.height() as usize) * 4;
        if out.len() >= MAX_ANIMATION_FRAMES || bytes_used + frame_bytes > MAX_ANIMATION_BYTES {
            truncated = true;
            break;
        }
        bytes_used += frame_bytes;
        out.push((DynamicImage::ImageRgba8(buffer), delay_ms));
    }
    if out.is_empty() {
        return Ok(None);
    }
    Ok(Some(AnimationFrames {
        frames: out,
        truncated,
    }))
}

fn fit_for_display(img: DynamicImage, max_dim: u32) -> (DynamicImage, bool) {
    let (w, h) = img.dimensions();
    if w <= max_dim && h <= max_dim {
        return (img, false);
    }
    let scale = max_dim as f64 / w.max(h) as f64;
    let nw = ((w as f64 * scale).round() as u32).max(1);
    let nh = ((h as f64 * scale).round() as u32).max(1);
    (
        img.resize_exact(nw, nh, image::imageops::FilterType::Triangle),
        true,
    )
}

fn frame_from_rgba(img: DynamicImage, delay_ms: u32) -> ImageFrame {
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    let bytes = Bytes::from(rgba.into_raw());
    ImageFrame {
        handle: Handle::from_rgba(w, h, bytes.clone()),
        rgba: bytes,
        delay_ms,
    }
}

fn format_label(format: ImageFormat) -> String {
    match format {
        ImageFormat::Png => "PNG".into(),
        ImageFormat::Jpeg => "JPEG".into(),
        ImageFormat::Gif => "GIF".into(),
        ImageFormat::WebP => "WebP".into(),
        ImageFormat::Pnm => "PNM".into(),
        ImageFormat::Tiff => "TIFF".into(),
        ImageFormat::Tga => "TGA".into(),
        ImageFormat::Dds => "DDS".into(),
        ImageFormat::Bmp => "BMP".into(),
        ImageFormat::Ico => "ICO".into(),
        ImageFormat::Hdr => "Radiance HDR".into(),
        ImageFormat::OpenExr => "OpenEXR".into(),
        ImageFormat::Farbfeld => "Farbfeld".into(),
        ImageFormat::Avif => "AVIF".into(),
        ImageFormat::Qoi => "QOI".into(),
        other => format!("{other:?}").to_uppercase(),
    }
}

fn describe_color(color: image::ExtendedColorType) -> (String, Option<u16>, bool) {
    use image::ExtendedColorType as C;
    let (label, bits, alpha) = match color {
        C::L1 => ("Grayscale", 1, false),
        C::La1 => ("Grayscale + alpha", 1, true),
        C::Rgb1 => ("RGB", 1, false),
        C::Rgba1 => ("RGBA", 1, true),
        C::L2 => ("Grayscale", 2, false),
        C::La2 => ("Grayscale + alpha", 2, true),
        C::Rgb2 => ("RGB", 2, false),
        C::Rgba2 => ("RGBA", 2, true),
        C::L4 => ("Grayscale", 4, false),
        C::La4 => ("Grayscale + alpha", 4, true),
        C::Rgb4 => ("RGB", 4, false),
        C::Rgba4 => ("RGBA", 4, true),
        C::L8 => ("Grayscale", 8, false),
        C::La8 => ("Grayscale + alpha", 8, true),
        C::Rgb8 => ("RGB", 8, false),
        C::Rgba8 => ("RGBA", 8, true),
        C::L16 => ("Grayscale", 16, false),
        C::La16 => ("Grayscale + alpha", 16, true),
        C::Rgb16 => ("RGB", 16, false),
        C::Rgba16 => ("RGBA", 16, true),
        C::Bgr8 => ("BGR", 8, false),
        C::Bgra8 => ("BGRA", 8, true),
        C::Rgb32F => ("RGB (float)", 32, false),
        C::Rgba32F => ("RGBA (float)", 32, true),
        C::Cmyk8 => ("CMYK", 8, false),
        C::A8 => ("Alpha only", 8, true),
        C::Unknown(bits) => return (format!("Unknown ({bits}-bit)"), None, false),
        _ => return ("Unknown".to_string(), None, false),
    };
    (label.to_string(), Some(bits), alpha)
}

fn describe_orientation(orientation: image::metadata::Orientation) -> Option<String> {
    use image::metadata::Orientation as O;
    let label = match orientation {
        O::NoTransforms => return None,
        O::Rotate90 => "Rotated 90° clockwise (applied)",
        O::Rotate180 => "Rotated 180° (applied)",
        O::Rotate270 => "Rotated 270° clockwise (applied)",
        O::FlipHorizontal => "Flipped horizontally (applied)",
        O::FlipVertical => "Flipped vertically (applied)",
        O::Rotate90FlipH => "Rotated 90° + flipped (applied)",
        O::Rotate270FlipH => "Rotated 270° + flipped (applied)",
    };
    Some(label.to_string())
}

fn orientation_swaps_axes(orientation: image::metadata::Orientation) -> bool {
    use image::metadata::Orientation as O;
    matches!(
        orientation,
        O::Rotate90 | O::Rotate270 | O::Rotate90FlipH | O::Rotate270FlipH
    )
}

// ---------------------------------------------------------------------------
// SVG
// ---------------------------------------------------------------------------

fn decode_svg(bytes: &[u8]) -> Result<DecodedImage, MediaError> {
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(bytes, &options)
        .map_err(|e| MediaError::Corrupt(format!("invalid SVG: {e}")))?;
    let size = tree.size();
    let (w, h) = (size.width().max(1.0), size.height().max(1.0));

    // Rasterize once (bounded) so difference mode and thumbnails have pixels.
    let scale = (SVG_RASTER_MAX_DIMENSION as f32 / w.max(h)).clamp(0.01, 4.0);
    let raster_w = ((w * scale).round() as u32).max(1);
    let raster_h = ((h * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(raster_w, raster_h)
        .ok_or_else(|| MediaError::Corrupt("SVG canvas too large to rasterize".to_string()))?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    // tiny-skia stores premultiplied RGBA; un-premultiply for the GPU path.
    let mut rgba = pixmap.take();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            px[0] = ((px[0] as u32 * 255 + a / 2) / a).min(255) as u8;
            px[1] = ((px[1] as u32 * 255 + a / 2) / a).min(255) as u8;
            px[2] = ((px[2] as u32 * 255 + a / 2) / a).min(255) as u8;
        }
    }
    let rgba = Bytes::from(rgba);
    let frame = ImageFrame {
        handle: Handle::from_rgba(raster_w, raster_h, rgba.clone()),
        rgba,
        delay_ms: 0,
    };

    let info = ImageInfo {
        format: "SVG".into(),
        color: "Vector".into(),
        bits_per_channel: None,
        has_alpha: true,
        file_size: bytes.len() as u64,
        original_width: w.round() as u32,
        original_height: h.round() as u32,
        icc_profile: false,
        orientation: None,
        downscaled: false,
        frame_count: 1,
        frames_truncated: false,
        decoder: "resvg",
        dpi: None,
        exif: Vec::new(),
    };

    Ok(DecodedImage {
        width: w.round() as u32,
        height: h.round() as u32,
        frames: vec![frame],
        total_duration_ms: 0,
        svg: Some(iced::widget::svg::Handle::from_memory(bytes.to_vec())),
        info,
    })
}

// ---------------------------------------------------------------------------
// FFmpeg fallback for exotic stills
// ---------------------------------------------------------------------------

fn decode_with_ffmpeg(bytes: &[u8], file_path: &str) -> Result<DecodedImage, MediaError> {
    if !super::ffmpeg::is_available() {
        return Err(MediaError::FfmpegMissing);
    }
    let ext = file_path
        .rsplit('.')
        .next()
        .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("bin");
    let temp = tempfile::Builder::new()
        .prefix("git-leviathan-img-")
        .suffix(&format!(".{ext}"))
        .tempfile()
        .map_err(|e| MediaError::Io(e.to_string()))?;
    std::fs::write(temp.path(), bytes).map_err(|e| MediaError::Io(e.to_string()))?;
    let png = super::ffmpeg::transcode_image_to_png(temp.path())?;
    let mut decoded = decode_with_image_crate(&png, ImageFormat::Png, file_path)?;
    decoded.info.decoder = "ffmpeg";
    decoded.info.file_size = bytes.len() as u64;
    decoded.info.format = ext.to_ascii_uppercase();
    // FFmpeg's PNG loses the source color description; keep what we can.
    let exif = read_exif(bytes);
    if !exif.fields.is_empty() {
        decoded.info.exif = exif.fields;
        decoded.info.dpi = exif.dpi;
    }
    Ok(decoded)
}

// ---------------------------------------------------------------------------
// EXIF
// ---------------------------------------------------------------------------

struct ExifSummary {
    fields: Vec<(String, String)>,
    dpi: Option<(f64, f64)>,
}

/// Curated EXIF tags in display order: (tag, label).
const CURATED_EXIF: &[(exif::Tag, &str)] = &[
    (exif::Tag::Make, "Camera make"),
    (exif::Tag::Model, "Camera model"),
    (exif::Tag::LensMake, "Lens make"),
    (exif::Tag::LensModel, "Lens"),
    (exif::Tag::DateTimeOriginal, "Date taken"),
    (exif::Tag::DateTime, "Date modified"),
    (exif::Tag::ExposureTime, "Exposure"),
    (exif::Tag::FNumber, "Aperture"),
    (exif::Tag::PhotographicSensitivity, "ISO"),
    (exif::Tag::FocalLength, "Focal length"),
    (exif::Tag::FocalLengthIn35mmFilm, "Focal length (35mm)"),
    (exif::Tag::ExposureProgram, "Exposure program"),
    (exif::Tag::ExposureBiasValue, "Exposure bias"),
    (exif::Tag::MeteringMode, "Metering"),
    (exif::Tag::Flash, "Flash"),
    (exif::Tag::WhiteBalance, "White balance"),
    (exif::Tag::ColorSpace, "Color space"),
    (exif::Tag::Orientation, "EXIF orientation"),
    (exif::Tag::Software, "Software"),
    (exif::Tag::Artist, "Artist"),
    (exif::Tag::Copyright, "Copyright"),
    (exif::Tag::ImageDescription, "Description"),
    (exif::Tag::GPSLatitude, "GPS latitude"),
    (exif::Tag::GPSLongitude, "GPS longitude"),
    (exif::Tag::GPSAltitude, "GPS altitude"),
];

fn read_exif(bytes: &[u8]) -> ExifSummary {
    let mut summary = ExifSummary {
        fields: Vec::new(),
        dpi: None,
    };
    let reader = exif::Reader::new();
    let Ok(exif) = reader.read_from_container(&mut Cursor::new(bytes)) else {
        return summary;
    };

    let primary = exif::In::PRIMARY;
    let value_of = |tag: exif::Tag| -> Option<String> {
        let field = exif.get_field(tag, primary)?;
        let text = field.display_value().with_unit(&exif).to_string();
        let text = text.trim().trim_matches('"').trim().to_string();
        (!text.is_empty()).then_some(text)
    };

    for (tag, label) in CURATED_EXIF {
        if let Some(mut value) = value_of(*tag) {
            if *tag == exif::Tag::GPSLatitude {
                if let Some(r) = value_of(exif::Tag::GPSLatitudeRef) {
                    value = format!("{value} {r}");
                }
            } else if *tag == exif::Tag::GPSLongitude {
                if let Some(r) = value_of(exif::Tag::GPSLongitudeRef) {
                    value = format!("{value} {r}");
                }
            }
            summary.fields.push(((*label).to_string(), value));
        }
    }

    // Resolution → DPI (unit 2 = inch, 3 = cm).
    let res = |tag: exif::Tag| -> Option<f64> {
        let field = exif.get_field(tag, primary)?;
        match &field.value {
            exif::Value::Rational(v) if !v.is_empty() && v[0].denom != 0 => Some(v[0].to_f64()),
            _ => None,
        }
    };
    if let (Some(x), Some(y)) = (res(exif::Tag::XResolution), res(exif::Tag::YResolution)) {
        let unit = exif
            .get_field(exif::Tag::ResolutionUnit, primary)
            .and_then(|f| f.value.get_uint(0))
            .unwrap_or(2);
        let (x, y) = if unit == 3 { (x * 2.54, y * 2.54) } else { (x, y) };
        if x > 0.0 && y > 0.0 {
            summary.dpi = Some((x, y));
        }
    }

    // Everything else, alphabetically, excluding maker notes / binary blobs.
    let curated: std::collections::HashSet<exif::Tag> =
        CURATED_EXIF.iter().map(|(t, _)| *t).collect();
    let mut extra: Vec<(String, String)> = exif
        .fields()
        .filter(|f| f.ifd_num == primary && !curated.contains(&f.tag))
        .filter(|f| {
            !matches!(
                f.tag,
                exif::Tag::MakerNote
                    | exif::Tag::UserComment
                    | exif::Tag::InteroperabilityIndex
                    | exif::Tag::ExifVersion
                    | exif::Tag::FlashpixVersion
                    | exif::Tag::ComponentsConfiguration
                    | exif::Tag::XResolution
                    | exif::Tag::YResolution
                    | exif::Tag::ResolutionUnit
                    | exif::Tag::GPSLatitudeRef
                    | exif::Tag::GPSLongitudeRef
                    | exif::Tag::JPEGInterchangeFormat
                    | exif::Tag::JPEGInterchangeFormatLength
            )
        })
        .filter_map(|f| {
            let label = f.tag.to_string();
            if label.starts_with("Tag(") {
                return None; // unknown/private tags
            }
            let value = f.display_value().with_unit(&exif).to_string();
            let value = value.trim().trim_matches('"').trim().to_string();
            if value.is_empty() || value.len() > 200 {
                return None;
            }
            Some((label, value))
        })
        .collect();
    extra.sort();
    extra.dedup();
    summary.fields.extend(extra);
    summary
}

// ---------------------------------------------------------------------------
// Pixel difference
// ---------------------------------------------------------------------------

pub struct DifferenceImage {
    pub handle: Handle,
    pub width: u32,
    pub height: u32,
    pub changed_pixels: u64,
    pub total_pixels: u64,
    /// Largest per-channel delta seen (0-255).
    pub max_delta: u8,
    /// Bounding box of all changed pixels (x, y, w, h) in image space.
    pub changed_bounds: Option<(u32, u32, u32, u32)>,
}

impl DifferenceImage {
    pub fn changed_ratio(&self) -> f64 {
        if self.total_pixels == 0 {
            0.0
        } else {
            self.changed_pixels as f64 / self.total_pixels as f64
        }
    }
}

impl std::fmt::Debug for DifferenceImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DifferenceImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("changed_pixels", &self.changed_pixels)
            .field("total_pixels", &self.total_pixels)
            .finish()
    }
}

/// Build a heat-map of changed pixels: unchanged pixels become a dim
/// grayscale of the new image, changed pixels ramp yellow → red with delta.
/// Both images must have identical display dimensions.
pub fn difference_image(old: &DecodedImage, new: &DecodedImage) -> Result<DifferenceImage, String> {
    let (Some(a), Some(b)) = (old.frames.first(), new.frames.first()) else {
        return Err("Nothing to compare.".to_string());
    };
    let (aw, ah) = raster_dims(old, a);
    let (bw, bh) = raster_dims(new, b);
    if aw != bw || ah != bh {
        return Err(format!(
            "Dimensions differ ({aw} × {ah} vs {bw} × {bh}); pixel difference needs matching sizes."
        ));
    }
    let w = aw;
    let h = ah;
    let total = (w as u64) * (h as u64);
    let mut out = vec![0u8; (total * 4) as usize];
    let mut changed = 0u64;
    let mut max_delta = 0u8;
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;

    for (i, (pa, pb)) in a
        .rgba
        .chunks_exact(4)
        .zip(b.rgba.chunks_exact(4))
        .enumerate()
    {
        let d = pa
            .iter()
            .zip(pb)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        let o = &mut out[i * 4..i * 4 + 4];
        if d == 0 {
            // Dim grayscale of the new pixel, alpha-weighted so transparent
            // regions stay transparent against the checkerboard.
            let lum = (0.2126 * pb[0] as f32 + 0.7152 * pb[1] as f32 + 0.0722 * pb[2] as f32)
                * 0.30
                + 24.0;
            let lum = lum.min(255.0) as u8;
            o[0] = lum;
            o[1] = lum;
            o[2] = lum;
            o[3] = pb[3].max(pa[3]).max(40);
        } else {
            changed += 1;
            max_delta = max_delta.max(d);
            let x = (i as u32) % w;
            let y = (i as u32) / w;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            // Ramp: small deltas yellow, large deltas red.
            let t = (d as f32 / 255.0).sqrt();
            o[0] = 255;
            o[1] = (220.0 * (1.0 - t) + 40.0 * t) as u8;
            o[2] = (40.0 * (1.0 - t)) as u8;
            o[3] = 255;
        }
    }

    let changed_bounds =
        (changed > 0).then(|| (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1));
    Ok(DifferenceImage {
        handle: Handle::from_rgba(w, h, out),
        width: w,
        height: h,
        changed_pixels: changed,
        total_pixels: total,
        max_delta,
        changed_bounds,
    })
}

fn raster_dims(img: &DecodedImage, frame: &ImageFrame) -> (u32, u32) {
    if img.svg.is_some() {
        // Rasterized SVG frame may be scaled; recover its dims from the buffer.
        let pixels = (frame.rgba.len() / 4) as u64;
        if img.width > 0 {
            let scale = (pixels as f64 / (img.width as f64 * img.height as f64)).sqrt();
            let w = (img.width as f64 * scale).round() as u32;
            let h = if w > 0 { (pixels / w as u64) as u32 } else { 0 };
            return (w, h);
        }
    }
    (img.width, img.height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn png_bytes(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        let img = ImageBuffer::from_pixel(w, h, Rgba(color));
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn decodes_png_with_metadata() {
        let bytes = png_bytes(8, 4, [10, 20, 30, 255]);
        let img = decode_image(&bytes, "a.png").unwrap();
        assert_eq!((img.width, img.height), (8, 4));
        assert_eq!(img.frames.len(), 1);
        assert!(!img.is_animated());
        assert_eq!(img.info.format, "PNG");
        assert_eq!(img.info.color, "RGBA");
        assert_eq!(img.info.bits_per_channel, Some(8));
        assert!(img.info.has_alpha);
        assert_eq!(img.info.file_size, bytes.len() as u64);
        let props = img.properties();
        assert!(props.iter().any(|(k, v)| k == "Dimensions" && v == "8 × 4"));
        assert!(props.iter().any(|(k, v)| k == "Aspect ratio" && v == "2:1"));
    }

    #[test]
    fn decodes_gif_animation_frames() {
        use image::codecs::gif::GifEncoder;
        use image::{Delay, Frame};
        let mut buf = Cursor::new(Vec::new());
        {
            let mut enc = GifEncoder::new(&mut buf);
            enc.set_repeat(image::codecs::gif::Repeat::Infinite).unwrap();
            for c in [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]] {
                let img = ImageBuffer::from_pixel(4, 4, Rgba(c));
                enc.encode_frame(Frame::from_parts(
                    img,
                    0,
                    0,
                    Delay::from_numer_denom_ms(100, 1),
                ))
                .unwrap();
            }
        }
        let bytes = buf.into_inner();
        let img = decode_image(&bytes, "anim.gif").unwrap();
        assert!(img.is_animated());
        assert_eq!(img.frames.len(), 3);
        assert_eq!(img.total_duration_ms, 300);
        assert_eq!(img.frame_index_at(0), 0);
        assert_eq!(img.frame_index_at(150), 1);
        assert_eq!(img.frame_index_at(299), 2);
        assert_eq!(img.frame_index_at(300), 0);
        assert_eq!(img.frame_start_ms(2), 200);
        assert_eq!(img.ms_until_next_frame(150), 50);
    }

    #[test]
    fn decodes_svg_with_intrinsic_size() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="60"><rect width="120" height="60" fill="red"/></svg>"#;
        let img = decode_image(svg, "shape.svg").unwrap();
        assert_eq!((img.width, img.height), (120, 60));
        assert!(img.svg.is_some());
        assert_eq!(img.info.format, "SVG");
        let (rw, rh) = raster_dims(&img, &img.frames[0]);
        assert!(rw >= 120 && rh >= 60);
    }

    #[test]
    fn broken_svg_is_reported_as_corrupt() {
        let err = decode_image(b"<svg xmlns='http://www.w3.org/2000/svg'><rect", "bad.svg").unwrap_err();
        assert!(matches!(err, MediaError::Corrupt(_)));
    }

    #[test]
    fn truncated_png_is_corrupt_not_unrecognized() {
        let bytes = png_bytes(32, 32, [1, 2, 3, 4]);
        let truncated = &bytes[..bytes.len() / 2];
        let err = decode_image(truncated, "cut.png").unwrap_err();
        assert!(matches!(err, MediaError::Corrupt(_)), "{err:?}");
    }

    #[test]
    fn difference_highlights_changed_pixels_and_bounds() {
        let a = decode_image(&png_bytes(4, 4, [0, 0, 0, 255]), "a.png").unwrap();
        let mut img = ImageBuffer::from_pixel(4, 4, Rgba([0, 0, 0, 255]));
        img.put_pixel(2, 1, Rgba([255, 0, 0, 255]));
        img.put_pixel(3, 3, Rgba([0, 10, 0, 255]));
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        let b = decode_image(&out.into_inner(), "b.png").unwrap();
        let diff = difference_image(&a, &b).unwrap();
        assert_eq!(diff.changed_pixels, 2);
        assert_eq!(diff.total_pixels, 16);
        assert_eq!(diff.max_delta, 255);
        assert_eq!(diff.changed_bounds, Some((2, 1, 2, 3)));
        assert!((diff.changed_ratio() - 0.125).abs() < 1e-9);
    }

    #[test]
    fn difference_requires_matching_dimensions() {
        let a = decode_image(&png_bytes(4, 4, [0, 0, 0, 255]), "a.png").unwrap();
        let b = decode_image(&png_bytes(5, 4, [0, 0, 0, 255]), "b.png").unwrap();
        let err = difference_image(&a, &b).unwrap_err();
        assert!(err.contains("Dimensions differ"));
    }

    #[test]
    fn oversized_images_are_downscaled_for_display() {
        let (img, downscaled) = fit_for_display(
            DynamicImage::new_rgba8(MAX_DISPLAY_DIMENSION * 2, 100),
            MAX_DISPLAY_DIMENSION,
        );
        assert!(downscaled);
        assert_eq!(img.width(), MAX_DISPLAY_DIMENSION);
        assert_eq!(img.height(), 50);
    }

    #[test]
    fn aspect_ratio_labels_reduce_fractions() {
        assert_eq!(aspect_ratio_label(1920, 1080), "16:9");
        assert_eq!(aspect_ratio_label(1000, 1000), "1:1");
        assert_eq!(aspect_ratio_label(1001, 1000), "1.001:1");
        assert_eq!(aspect_ratio_label(0, 10), "—");
    }
}

//! `{ kind = "image", path, size? }` — raster image from the plugin
//! sandbox root.
//!
//! `path` is a plugin-relative path (`is_safe_relative_path` enforces
//! containment). The file extension selects the codec:
//!
//! - `.gif` — decoded as an animation. All frames are decoded once on
//!   first use and cached as ready-to-use `image::Handle`s. Each render
//!   picks the frame corresponding to `Instant::now() % total_duration`.
//! - anything else — treated as a static image (`Handle::from_path`).
//!
//! The image-too-large limit is enforced at decode time via a stat call
//! before we try to load anything from disk; oversized files render an
//! error widget rather than blow out memory.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use iced::widget::image;
use iced::{Element, Length};

use crate::message::Message;
use crate::plugin::ui::widget_ast::{codes as widget_codes, ImageNode, WidgetLimits};

use super::common::{build_error_widget, error_text, is_safe_relative_path};
use super::BuildCtx;

struct AnimatedImage {
    frames: Vec<(image::Handle, u32)>,
    total_ms: u32,
}

enum CachedImage {
    Static(image::Handle),
    Animated(AnimatedImage),
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Arc<CachedImage>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<CachedImage>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load_cached(path: &Path) -> Option<Arc<CachedImage>> {
    {
        let lock = cache().lock().ok()?;
        if let Some(existing) = lock.get(path) {
            return Some(existing.clone());
        }
    }
    let decoded = Arc::new(decode(path)?);
    let mut lock = cache().lock().ok()?;
    Some(
        lock.entry(path.to_path_buf())
            .or_insert_with(|| decoded.clone())
            .clone(),
    )
}

fn decode(path: &Path) -> Option<CachedImage> {
    let is_gif = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.eq_ignore_ascii_case("gif"))
        .unwrap_or(false);
    if is_gif {
        decode_gif(path)
    } else {
        Some(CachedImage::Static(image::Handle::from_path(path)))
    }
}

fn decode_gif(path: &Path) -> Option<CachedImage> {
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
        let handle = image::Handle::from_rgba(w, h, buf.into_raw());
        out.push((handle, delay_ms));
        total_ms = total_ms.saturating_add(delay_ms);
    }
    if out.is_empty() {
        return None;
    }
    if total_ms == 0 {
        total_ms = 1;
    }
    Some(CachedImage::Animated(AnimatedImage {
        frames: out,
        total_ms,
    }))
}

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn pick_frame(anim: &AnimatedImage) -> &image::Handle {
    let elapsed_ms = (epoch().elapsed().as_millis() as u32) % anim.total_ms.max(1);
    let mut acc = 0u32;
    for (handle, delay) in &anim.frames {
        acc = acc.saturating_add(*delay);
        if elapsed_ms < acc {
            return handle;
        }
    }
    &anim.frames.last().expect("non-empty checked on decode").0
}

pub(super) fn build(node: &ImageNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    if !is_safe_relative_path(&node.path) {
        return error_text(format!("invalid image path: {:?}", node.path));
    }
    let size = node.size;
    let resolved = ctx.plugin_root.join(&node.path);

    let limits = WidgetLimits::DEFAULT;
    if let Ok(meta) = std::fs::metadata(&resolved) {
        if meta.len() > limits.max_image_size_bytes {
            return build_error_widget(
                widget_codes::IMAGE_TOO_LARGE,
                &format!(
                    "{:?} is {} bytes (max {})",
                    node.path,
                    meta.len(),
                    limits.max_image_size_bytes
                ),
            );
        }
    }

    let Some(cached) = load_cached(&resolved) else {
        return error_text(format!("image load failed: {:?}", node.path));
    };
    let handle = match cached.as_ref() {
        CachedImage::Static(h) => h.clone(),
        CachedImage::Animated(anim) => pick_frame(anim).clone(),
    };
    image(handle)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

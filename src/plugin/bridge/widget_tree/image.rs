//! `{ kind = "image", path, size? }` — raster image from the plugin
//! sandbox root.
//!
//! `path` is a plugin-relative path (`is_safe_relative_path` enforces
//! containment). The file extension selects the codec:
//!
//! - `.gif` — decoded as an animation by the shared media widget. The widget
//!   requests redraws at the source frame boundaries.
//! - anything else — treated as a static image (`Handle::from_path`).
//!
//! The image-too-large limit is enforced at decode time via a stat call
//! before we try to load anything from disk; oversized files render an
//! error widget rather than blow out memory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use iced::widget::image;
use iced::{Element, Length};

use crate::message::Message;
use crate::plugin::ui::widget_ast::{codes as widget_codes, ImageNode, WidgetLimits};
use crate::widgets::media::{animated_raster, load_animated_raster, AnimatedRaster};

use super::common::{build_error_widget, error_text, is_safe_relative_path};
use super::BuildCtx;

enum CachedImage {
    Static(image::Handle),
    Animated(Arc<AnimatedRaster>),
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
        load_animated_raster(path).map(CachedImage::Animated)
    } else {
        Some(CachedImage::Static(image::Handle::from_path(path)))
    }
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
    match cached.as_ref() {
        CachedImage::Static(handle) => image(handle.clone())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into(),
        CachedImage::Animated(raster) => animated_raster(raster.clone(), size),
    }
}

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
//! The image-too-large limit is enforced once at decode time via a stat
//! call before we try to load anything from disk; oversized files render
//! an error widget rather than blow out memory. The result (decoded
//! handle, oversized marker, or load failure) is memoised per resolved
//! path so the render hot path never re-stats or re-decodes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use iced::widget::image;
use iced::{Element, Length};

use crate::message::Message;
use crate::plugin::ui::widget_ast::{codes as widget_codes, ImageNode, WidgetLimits};
use crate::widgets::media::{animated_raster, load_animated_raster, AnimatedRaster};

use super::common::{build_error_widget, error_text, is_safe_relative_path};
use super::BuildCtx;

/// Most decoded images held in the process-global cache at once. Decoded
/// GIF frames are not cheap, so we evict the least-recently-used entry
/// once the cache is full rather than growing without bound.
const MAX_CACHE_ENTRIES: usize = 128;

#[derive(Clone)]
enum CachedImage {
    Static(image::Handle),
    Animated(Arc<AnimatedRaster>),
    Oversized { len: u64 },
    LoadFailed,
}

struct CacheEntry {
    image: Arc<CachedImage>,
    last_used: u64,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_tick() -> u64 {
    static TICK: AtomicU64 = AtomicU64::new(0);
    TICK.fetch_add(1, Ordering::Relaxed)
}

/// Drop every cached entry resolved under `plugin_root`. Called when a
/// plugin reloads or unloads so stale results — especially `LoadFailed`
/// / `Oversized` markers recorded while an asset was briefly missing or
/// being rewritten — don't outlive the generation that produced them.
/// Keys are absolute paths (`plugin_root.join(relative)`), so a prefix
/// match scopes the clear to the one plugin.
pub fn clear_for_root(plugin_root: &Path) {
    if let Ok(mut lock) = cache().lock() {
        lock.retain(|path, _| !path.starts_with(plugin_root));
    }
}

fn load_cached(path: &Path) -> Arc<CachedImage> {
    if let Ok(mut lock) = cache().lock() {
        if let Some(entry) = lock.get_mut(path) {
            entry.last_used = next_tick();
            return entry.image.clone();
        }
    }
    let decoded = Arc::new(decode(path));
    if let Ok(mut lock) = cache().lock() {
        if lock.len() >= MAX_CACHE_ENTRIES && !lock.contains_key(path) {
            if let Some(oldest) = lock
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone())
            {
                lock.remove(&oldest);
            }
        }
        return lock
            .entry(path.to_path_buf())
            .or_insert_with(|| CacheEntry {
                image: decoded.clone(),
                last_used: next_tick(),
            })
            .image
            .clone();
    }
    decoded
}

fn decode(path: &Path) -> CachedImage {
    let limits = WidgetLimits::DEFAULT;
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > limits.max_image_size_bytes {
            return CachedImage::Oversized { len: meta.len() };
        }
    }
    let is_gif = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.eq_ignore_ascii_case("gif"))
        .unwrap_or(false);
    if is_gif {
        match load_animated_raster(path) {
            Some(raster) => CachedImage::Animated(raster),
            None => CachedImage::LoadFailed,
        }
    } else {
        CachedImage::Static(image::Handle::from_path(path))
    }
}

pub(super) fn build(node: &ImageNode, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    if !is_safe_relative_path(&node.path) {
        return error_text(format!("invalid image path: {:?}", node.path));
    }
    let size = node.size;
    let resolved = ctx.plugin_root.join(&node.path);

    match load_cached(&resolved).as_ref() {
        CachedImage::Static(handle) => image(handle.clone())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into(),
        CachedImage::Animated(raster) => animated_raster(raster.clone(), size),
        CachedImage::Oversized { len } => build_error_widget(
            widget_codes::IMAGE_TOO_LARGE,
            &format!(
                "{:?} is {} bytes (max {})",
                node.path,
                len,
                WidgetLimits::DEFAULT.max_image_size_bytes
            ),
        ),
        CachedImage::LoadFailed => error_text(format!("image load failed: {:?}", node.path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_cached(path: &Path) -> bool {
        cache().lock().unwrap().contains_key(path)
    }

    #[test]
    fn clear_for_root_evicts_memoized_failure() {
        let root = std::env::temp_dir().join(format!(
            "leviathan-image-cache-{}-{}",
            std::process::id(),
            next_tick()
        ));
        let missing = root.join("missing.gif");

        let cached = load_cached(&missing);
        assert!(matches!(*cached, CachedImage::LoadFailed));
        assert!(is_cached(&missing));

        clear_for_root(&root);
        assert!(!is_cached(&missing));

        // An unrelated root's entries survive a scoped clear.
        let other = std::env::temp_dir().join(format!(
            "leviathan-image-cache-other-{}-{}",
            std::process::id(),
            next_tick()
        ));
        let other_missing = other.join("missing.gif");
        let _ = load_cached(&other_missing);
        assert!(is_cached(&other_missing));
        clear_for_root(&root);
        assert!(is_cached(&other_missing));
        clear_for_root(&other);
    }
}

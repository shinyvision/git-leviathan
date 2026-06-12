use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::{LazyLock, RwLock};

use iced::advanced::graphics::text::{self as graphics_text, cosmic_text};
use lru::LruCache;

pub struct TextMeasureRequest {
    pub text: String,
    pub font_family: FontFamily,
    pub font_size: f32,
    /// Defaults to `font_size * 1.2` when `None`.
    pub line_height: Option<f32>,
    /// `None` disables wrapping (single-line).
    pub max_width: Option<f32>,
}

impl TextMeasureRequest {
    pub fn single_line(text: impl Into<String>, font_family: FontFamily, font_size: f32) -> Self {
        Self {
            text: text.into(),
            font_family,
            font_size,
            line_height: None,
            max_width: None,
        }
    }

    #[cfg(test)]
    pub fn wrapped(
        text: impl Into<String>,
        font_family: FontFamily,
        font_size: f32,
        max_width: f32,
    ) -> Self {
        Self {
            text: text.into(),
            font_family,
            font_size,
            line_height: Some(font_size * 1.2),
            max_width: Some(max_width),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMeasureResult {
    pub width: f32,
    pub height: f32,
    /// Number of lines after wrapping (1 for single-line).
    pub line_count: usize,
    /// Baseline offset from top in pixels.
    pub baseline: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontFamily {
    Default,
}

impl FontFamily {
    fn to_iced_font(self) -> iced::Font {
        match self {
            FontFamily::Default => iced::Font::default(),
        }
    }
}

/// Exact pixel text measurement built on iced's `cosmic_text`. Calls acquire
/// the global font-system write lock; prefer `cached_measure_width` on hot
/// paths.
pub struct TextMeasurementService;

impl TextMeasurementService {
    pub fn new() -> Self {
        Self
    }

    pub fn measure(&self, request: TextMeasureRequest) -> TextMeasureResult {
        let mut font_system = graphics_text::font_system()
            .write()
            .expect("font system lock");

        let line_height = request
            .line_height
            .unwrap_or(request.font_size * 1.2)
            .max(f32::MIN_POSITIVE);

        let metrics = cosmic_text::Metrics::new(request.font_size, line_height);
        let mut buffer = cosmic_text::Buffer::new(font_system.raw(), metrics);

        let wrap_mode = if request.max_width.is_some() {
            graphics_text::to_wrap(iced::advanced::text::Wrapping::WordOrGlyph)
        } else {
            cosmic_text::Wrap::None
        };
        buffer.set_wrap(font_system.raw(), wrap_mode);
        buffer.set_size(font_system.raw(), request.max_width, None);

        let font = request.font_family.to_iced_font();
        let attrs = graphics_text::to_attributes(font);
        buffer.set_text(
            font_system.raw(),
            &request.text,
            &attrs,
            graphics_text::to_shaping(iced::advanced::text::Shaping::Advanced, &request.text),
            None,
        );

        let (size, _) = graphics_text::measure(&buffer);
        let line_count = buffer.layout_runs().count();
        // Approximate — typically 80% of font size from top.
        let baseline = request.font_size * 0.8;

        TextMeasureResult {
            width: size.width,
            height: size.height.max(line_height),
            line_count,
            baseline,
        }
    }

    pub fn measure_single_line(
        &self,
        text: impl Into<String>,
        font_family: FontFamily,
        font_size: f32,
    ) -> TextMeasureResult {
        self.measure(TextMeasureRequest::single_line(
            text,
            font_family,
            font_size,
        ))
    }

    #[cfg(test)]
    pub fn measure_wrapped(
        &self,
        text: impl Into<String>,
        font_family: FontFamily,
        font_size: f32,
        max_width: f32,
    ) -> TextMeasureResult {
        self.measure(TextMeasureRequest::wrapped(
            text,
            font_family,
            font_size,
            max_width,
        ))
    }
}

impl Default for TextMeasurementService {
    fn default() -> Self {
        Self::new()
    }
}

// Global measurement cache. Avoids acquiring the font system write lock on
// every frame for the same inputs. Keys are a single `u64` hash of the
// borrowed inputs so cache hits never allocate the lookup string; collisions
// are vanishingly unlikely for text measurement.

type WidthKey = u64; // hash of (text, font_family_discriminant, font_size_bits)
type TruncKey = u64; // hash of (text, max_width_bits, font_family_discriminant, font_size_bits)

const WIDTH_CACHE_CAPACITY: usize = 4096;
const TRUNC_CACHE_CAPACITY: usize = 2048;

fn nonzero_capacity(capacity: usize) -> NonZeroUsize {
    NonZeroUsize::new(capacity).expect("text measurement cache capacity is non-zero")
}

static WIDTH_CACHE: LazyLock<RwLock<LruCache<WidthKey, f32>>> =
    LazyLock::new(|| RwLock::new(LruCache::new(nonzero_capacity(WIDTH_CACHE_CAPACITY))));
static TRUNC_CACHE: LazyLock<RwLock<LruCache<TruncKey, String>>> =
    LazyLock::new(|| RwLock::new(LruCache::new(nonzero_capacity(TRUNC_CACHE_CAPACITY))));

/// Drop measurement LRUs. iced 0.14 pins cosmic-text to 0.15 with the
/// `shape-run-cache` feature off, so there is no shape-run cache to trim
/// on the iced FontSystem; the wgpu glyph atlas still releases on the
/// next frame once the diff widget tree drops.
pub fn release_text_caches() {
    if let Ok(mut cache) = WIDTH_CACHE.write() {
        cache.clear();
    }
    if let Ok(mut cache) = TRUNC_CACHE.write() {
        cache.clear();
    }
}

fn font_family_key(f: FontFamily) -> u8 {
    match f {
        FontFamily::Default => 0,
    }
}

fn width_key(text: &str, font_family: FontFamily, font_size: f32) -> WidthKey {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    font_family_key(font_family).hash(&mut h);
    font_size.to_bits().hash(&mut h);
    h.finish()
}

fn trunc_key(name: &str, max_width: f32, font_size: f32) -> TruncKey {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    max_width.to_bits().hash(&mut h);
    font_family_key(FontFamily::Default).hash(&mut h);
    font_size.to_bits().hash(&mut h);
    h.finish()
}

/// Measure a single line of text, using a global cache to avoid repeated
/// font-system write-lock acquisitions for the same inputs.
pub fn cached_measure_width(text: &str, font_family: FontFamily, font_size: f32) -> f32 {
    let key = width_key(text, font_family, font_size);
    if let Ok(mut cache) = WIDTH_CACHE.write() {
        if let Some(w) = cache.get(&key) {
            return *w;
        }
    }
    let service = TextMeasurementService::new();
    let result = service.measure_single_line(text, font_family, font_size);
    let w = result.width;
    if let Ok(mut cache) = WIDTH_CACHE.write() {
        cache.put(key, w);
    }
    w
}

/// Truncate `name` to fit within `max_width` pixels (with "…" if needed),
/// caching the result so we never recompute the same truncation.
pub fn cached_truncate_name(name: &str, max_width: f32) -> String {
    let font_size = crate::theme::FONT_SM;
    let key = trunc_key(name, max_width, font_size);
    if let Ok(mut cache) = TRUNC_CACHE.write() {
        if let Some(truncated) = cache.get(&key) {
            return truncated.clone();
        }
    }
    let truncated = truncate_name_uncached(name, max_width);
    if let Ok(mut cache) = TRUNC_CACHE.write() {
        cache.put(key, truncated.clone());
    }
    truncated
}

/// Uncached truncation logic (binary search with font measurement).
fn truncate_name_uncached(name: &str, max_width: f32) -> String {
    truncate_to_width(name, max_width, FontFamily::Default, crate::theme::FONT_SM)
}

/// Truncate `text` to fit within `max_width` pixels, appending "…" when it
/// doesn't fit. Binary-searches a byte boundary using cached pixel
/// measurements; returns the full string unchanged when it already fits.
pub fn truncate_to_width(
    text: &str,
    max_width: f32,
    font_family: FontFamily,
    font_size: f32,
) -> String {
    if max_width <= 0.0 {
        return String::new();
    }

    let full_width = cached_measure_width(text, font_family, font_size);
    if full_width <= max_width {
        return text.to_string();
    }

    match truncated_prefix(text, max_width, font_family, font_size) {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}…"),
        _ => "…".to_string(),
    }
}

/// Binary-search the longest byte-boundary prefix of `text` whose pixel width
/// plus the ellipsis fits within `max_width`. Returns `None` only when not
/// even a single character fits alongside the ellipsis.
fn truncated_prefix(
    text: &str,
    max_width: f32,
    font_family: FontFamily,
    font_size: f32,
) -> Option<&str> {
    let ellipsis_width = cached_measure_width("…", font_family, font_size);
    let available = max_width - ellipsis_width;
    if available <= 0.0 {
        return None;
    }

    let mut low = 0usize;
    let mut high = text.len();
    let mut best_byte = 0usize;

    while low < high {
        let mid = (low + high).div_ceil(2);
        let boundary = floor_char_boundary(text, mid);
        let w = cached_measure_width(&text[..boundary], font_family, font_size);
        if w <= available {
            best_byte = boundary;
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    if best_byte == 0 {
        None
    } else {
        Some(&text[..best_byte])
    }
}

pub fn floor_char_boundary(s: &str, index: usize) -> usize {
    let mut boundary = index.min(s.len());
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_measurement_has_height() {
        let service = TextMeasurementService::new();
        let result = service.measure_single_line("Hello", FontFamily::Default, 12.0);

        assert!(result.width > 0.0, "width should be positive");
        assert!(result.height > 0.0, "height should be positive");
        assert_eq!(
            result.line_count, 1,
            "single line should have line_count of 1"
        );
    }

    #[test]
    fn longer_text_has_greater_width() {
        let service = TextMeasurementService::new();
        let short = service.measure_single_line("Hi", FontFamily::Default, 12.0);
        let long = service.measure_single_line("Hello, World!", FontFamily::Default, 12.0);

        assert!(
            long.width > short.width,
            "longer text should have greater width"
        );
    }

    #[test]
    fn larger_font_has_greater_dimensions() {
        let service = TextMeasurementService::new();
        let small = service.measure_single_line("Test", FontFamily::Default, 10.0);
        let large = service.measure_single_line("Test", FontFamily::Default, 20.0);

        assert!(
            large.width > small.width,
            "larger font should have greater width"
        );
        assert!(
            large.height > small.height,
            "larger font should have greater height"
        );
    }

    #[test]
    fn wrapping_increases_line_count() {
        let service = TextMeasurementService::new();
        let single = service.measure_single_line(
            "This is a very long text that should wrap",
            FontFamily::Default,
            12.0,
        );
        let wrapped = service.measure_wrapped(
            "This is a very long text that should wrap",
            FontFamily::Default,
            12.0,
            50.0, // Very narrow to force wrapping
        );

        assert!(
            wrapped.line_count >= single.line_count,
            "wrapped text should have equal or more lines"
        );
        assert!(
            wrapped.width <= 50.0 + 1.0, // Allow small tolerance
            "wrapped text width should respect max_width"
        );
    }

    #[test]
    fn empty_text_has_zero_width() {
        let service = TextMeasurementService::new();
        let result = service.measure_single_line("", FontFamily::Default, 12.0);

        assert_eq!(result.width, 0.0, "empty text should have zero width");
        assert!(
            result.height > 0.0,
            "empty text should still have line height"
        );
    }
}

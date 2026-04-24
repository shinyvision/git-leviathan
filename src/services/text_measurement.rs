use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

use iced::advanced::graphics::text::{self as graphics_text, cosmic_text};

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
    pub line_count: usize,
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

        // Approximate: typical fonts sit the baseline ~80% down from the top.
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

// Float keys use `f32::to_bits()` so `HashMap` can hash them directly.
type WidthKey = (String, u8, u32);
type TruncKey = (String, u32, u8, u32);

static WIDTH_CACHE: LazyLock<RwLock<HashMap<WidthKey, f32>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static TRUNC_CACHE: LazyLock<RwLock<HashMap<TruncKey, String>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn font_family_key(f: FontFamily) -> u8 {
    match f {
        FontFamily::Default => 0,
    }
}

/// Cached single-line width measurement. Avoids re-acquiring the font-system
/// write lock for repeat inputs.
pub fn cached_measure_width(text: &str, font_family: FontFamily, font_size: f32) -> f32 {
    let key = (
        text.to_string(),
        font_family_key(font_family),
        font_size.to_bits(),
    );
    if let Ok(cache) = WIDTH_CACHE.read() {
        if let Some(&w) = cache.get(&key) {
            return w;
        }
    }
    let service = TextMeasurementService::new();
    let result = service.measure_single_line(text, font_family, font_size);
    let w = result.width;
    if let Ok(mut cache) = WIDTH_CACHE.write() {
        cache.insert(key, w);
    }
    w
}

/// Truncate `name` with a trailing ellipsis so it fits within `max_width`
/// pixels. Cached by `(name, max_width)`.
pub fn cached_truncate_name(name: &str, max_width: f32) -> String {
    let font_size = crate::theme::FONT_SM;
    let key = (
        name.to_string(),
        max_width.to_bits(),
        font_family_key(FontFamily::Default),
        font_size.to_bits(),
    );
    if let Ok(cache) = TRUNC_CACHE.read() {
        if let Some(truncated) = cache.get(&key) {
            return truncated.clone();
        }
    }
    let truncated = truncate_name_uncached(name, max_width);
    if let Ok(mut cache) = TRUNC_CACHE.write() {
        cache.insert(key, truncated.clone());
    }
    truncated
}

fn truncate_name_uncached(name: &str, max_width: f32) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    let font_size = crate::theme::FONT_SM;

    let full_width = cached_measure_width(name, FontFamily::Default, font_size);
    if full_width <= max_width {
        return name.to_string();
    }

    let ellipsis_width = cached_measure_width("…", FontFamily::Default, font_size);
    let available = max_width - ellipsis_width;
    if available <= 0.0 {
        return "…".to_string();
    }

    let mut low = 0usize;
    let mut high = name.len();
    let mut best_byte = 0usize;

    while low < high {
        let mid = (low + high).div_ceil(2);
        let substr = match name[..mid.min(name.len())].char_indices().last() {
            Some((idx, _)) => &name[..=idx],
            None => "",
        };
        let w = cached_measure_width(substr, FontFamily::Default, font_size);
        if w <= available {
            best_byte = substr.len();
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    if best_byte == 0 {
        "…".to_string()
    } else {
        let substr = match name[..best_byte.min(name.len())].char_indices().last() {
            Some((idx, _)) => &name[..=idx],
            None => "",
        };
        format!("{}…", substr)
    }
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
            50.0,
        );

        assert!(
            wrapped.line_count >= single.line_count,
            "wrapped text should have equal or more lines"
        );
        assert!(
            wrapped.width <= 50.0 + 1.0,
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

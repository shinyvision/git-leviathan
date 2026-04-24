//! `{kind = "icon", path, size?, color?}` — SVG glyph from the plugin's
//! sandbox root.
//!
//! `path` is a plugin-relative path (e.g. `"icons/file.svg"`); the plugin
//! chooses its own directory layout. `is_safe_relative_path` rejects any
//! input that could escape `plugin_root` via `..`, `.`, absolute prefixes,
//! drive letters, null bytes, or overlong strings. Missing files render as
//! blank — iced's svg widget handles that gracefully, so plugins are
//! expected to ship every icon they reference.

use iced::{widget::svg, Element, Length, Theme};
use serde_json::Value;

use crate::message::Message;
use crate::theme;

use super::common::{error_text, is_safe_relative_path, parse_color};
use super::BuildCtx;

pub(super) fn build(node: &Value, ctx: &BuildCtx<'_>) -> Element<'static, Message> {
    let rel = node.get("path").and_then(Value::as_str).unwrap_or("");
    if !is_safe_relative_path(rel) {
        return error_text(format!("invalid icon path: {rel:?}"));
    }
    let size = node.get("size").and_then(Value::as_f64).unwrap_or(16.0) as f32;
    let color = parse_color(node.get("color")).unwrap_or(theme::TEXT_PRIMARY);
    let resolved = ctx.plugin_root.join(rel);
    svg(svg::Handle::from_path(resolved))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_: &Theme, _: svg::Status| svg::Style { color: Some(color) })
        .into()
}

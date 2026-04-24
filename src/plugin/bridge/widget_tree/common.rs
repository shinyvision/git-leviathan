//! Shared DSL parsers + the error-text fallback used by every widget.
//!
//! Kept `pub(super)` so only sibling widget files can use them — nothing
//! outside `widget_tree` should care how a color/length/padding value is
//! parsed from the plugin-supplied JSON.

use std::path::Path;

use iced::{widget::text, Alignment, Border, Color, Element, Length, Theme};
use serde_json::Value;

use crate::message::Message;

pub(super) fn parse_length(value: Option<&Value>) -> Option<Length> {
    let v = value?;
    if let Some(n) = v.as_f64() {
        return Some(Length::Fixed(n as f32));
    }
    if let Some(s) = v.as_str() {
        return match s {
            "fill" => Some(Length::Fill),
            "shrink" => Some(Length::Shrink),
            _ => None,
        };
    }
    None
}

pub(super) fn parse_color(value: Option<&Value>) -> Option<Color> {
    let s = value?.as_str()?;
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
    Some(Color { r, g, b, a: 1.0 })
}

pub(super) fn parse_alignment_x(value: Option<&Value>) -> Option<Alignment> {
    let s = value?.as_str()?;
    match s {
        "start" | "left" => Some(Alignment::Start),
        "center" | "centre" => Some(Alignment::Center),
        "end" | "right" => Some(Alignment::End),
        _ => None,
    }
}

pub(super) fn parse_alignment_y(value: Option<&Value>) -> Option<Alignment> {
    let s = value?.as_str()?;
    match s {
        "start" | "top" => Some(Alignment::Start),
        "center" | "centre" => Some(Alignment::Center),
        "end" | "bottom" => Some(Alignment::End),
        _ => None,
    }
}

/// Parse a `{ width?, radius?, color? }` sub-table into an iced `Border`.
/// Missing fields default to 0 / transparent / no radius — matching
/// `Border::default()`.
pub(super) fn parse_border(value: Option<&Value>) -> Border {
    let Some(v) = value else { return Border::default() };
    let width = v.get("width").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let radius = v.get("radius").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let color = parse_color(v.get("color")).unwrap_or(Color::TRANSPARENT);
    Border {
        color,
        width,
        radius: radius.into(),
    }
}

/// Reject any plugin-supplied path that could escape the sandbox root via
/// `..`, `.`, absolute prefixes, drive letters, null bytes, or overlong
/// strings. Shared by every widget that resolves a plugin-bundled asset.
pub(super) fn is_safe_relative_path(s: &str) -> bool {
    if s.is_empty() || s.len() > 256 || s.contains('\0') {
        return false;
    }
    let p = Path::new(s);
    p.components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
}

pub(super) fn error_text(msg: String) -> Element<'static, Message> {
    text(msg)
        .size(12.0)
        .style(|_: &Theme| iced::widget::text::Style {
            color: Some(Color {
                r: 1.0,
                g: 0.3,
                b: 0.3,
                a: 1.0,
            }),
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_color_hex() {
        let c = parse_color(Some(&Value::String("#ff8800".into()))).unwrap();
        assert!((c.r - 1.0).abs() < 0.01);
        assert!((c.g - 0.533).abs() < 0.01);
        assert!((c.b - 0.0).abs() < 0.01);
    }

    #[test]
    fn parse_length_variants() {
        assert!(matches!(parse_length(Some(&serde_json::json!("fill"))), Some(Length::Fill)));
        assert!(matches!(parse_length(Some(&serde_json::json!("shrink"))), Some(Length::Shrink)));
        assert!(matches!(parse_length(Some(&serde_json::json!(42))), Some(Length::Fixed(_))));
        assert!(parse_length(Some(&serde_json::json!("bogus"))).is_none());
        assert!(parse_length(None).is_none());
    }

    #[test]
    fn parse_border_defaults_and_overrides() {
        let b = parse_border(None);
        assert!((b.width - 0.0).abs() < 0.01);
        let b = parse_border(Some(&serde_json::json!({
            "width": 1, "radius": 6, "color": "#ff0000"
        })));
        assert!((b.width - 1.0).abs() < 0.01);
        assert!((b.color.r - 1.0).abs() < 0.01);
    }

    #[test]
    fn safe_relative_path_rejects_escapes() {
        assert!(!is_safe_relative_path(""));
        assert!(!is_safe_relative_path(".."));
        assert!(!is_safe_relative_path("../etc/passwd"));
        assert!(!is_safe_relative_path("icons/../../../etc/passwd"));
        assert!(!is_safe_relative_path("/etc/passwd"));
        assert!(!is_safe_relative_path("./icons/file.svg"));
        assert!(!is_safe_relative_path("foo\0bar"));
        assert!(!is_safe_relative_path(&"a".repeat(257)));
    }

    #[test]
    fn safe_relative_path_accepts_relatives() {
        assert!(is_safe_relative_path("file.svg"));
        assert!(is_safe_relative_path("icons/file.svg"));
        assert!(is_safe_relative_path("assets/icons/folder.svg"));
        assert!(is_safe_relative_path("a/b/c/d.svg"));
    }

    #[test]
    fn parse_alignment_vocab() {
        assert!(matches!(parse_alignment_x(Some(&serde_json::json!("center"))), Some(Alignment::Center)));
        assert!(matches!(parse_alignment_x(Some(&serde_json::json!("end"))), Some(Alignment::End)));
        assert!(matches!(parse_alignment_y(Some(&serde_json::json!("top"))), Some(Alignment::Start)));
        assert!(parse_alignment_x(Some(&serde_json::json!("nope"))).is_none());
    }
}

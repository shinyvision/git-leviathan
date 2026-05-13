//! Shared DSL parsers + the error-text fallback used by every widget.
//!
//! The AST decoder normalises types upstream (widget AST) — these helpers
//! are the small "AST → iced" converters: turn an `AstColor` into an
//! `iced::Color`, an `AstLength` into a `Length`, etc. They never look
//! at raw JSON; that's the decoder's job.
//!
//! Kept `pub(super)` so only sibling widget files can use them — nothing
//! outside `widget_tree` should care how a color/length value gets
//! translated for iced.

use std::path::Path;

use iced::{widget::text, Alignment, Border, Color, Element, Length, Shadow, Theme, Vector};

use crate::message::Message;
use crate::plugin::ui::widget_ast::{
    AstAlignX, AstAlignY, AstBorder, AstColor, AstLength, AstShadow,
};

/// Convert an AST length to iced. `Auto` means "no opinion" — the caller
/// picks its own per-widget default (rows want `Fill`, padding wants
/// `Shrink`, etc.).
pub(super) fn length_or(l: AstLength, default: Length) -> Length {
    match l {
        AstLength::Auto => default,
        AstLength::Fill => Length::Fill,
        AstLength::Shrink => Length::Shrink,
        AstLength::Fixed(n) => Length::Fixed(n),
    }
}

/// AST-driven version that returns `None` when the AST said "auto" so
/// the caller can choose to skip setting the dimension at all.
pub(super) fn length_explicit(l: AstLength) -> Option<Length> {
    match l {
        AstLength::Auto => None,
        AstLength::Fill => Some(Length::Fill),
        AstLength::Shrink => Some(Length::Shrink),
        AstLength::Fixed(n) => Some(Length::Fixed(n)),
    }
}

pub(super) fn color_to_iced(color: &AstColor) -> Option<Color> {
    if let Some(token) = &color.token {
        return match token.as_str() {
            "text.primary" => Some(crate::theme::TEXT_PRIMARY),
            "text.secondary" => Some(crate::theme::TEXT_SECONDARY),
            "text.dim" => Some(crate::theme::TEXT_DIM),
            "accent.blue" => Some(crate::theme::ACCENT_BLUE),
            "accent.green" => Some(crate::theme::ACCENT_GREEN),
            "accent.orange" => Some(crate::theme::ACCENT_ORANGE),
            "bg.panel" => Some(crate::theme::BG_PANEL),
            "bg.sidebar" => Some(crate::theme::BG_SIDEBAR),
            _ => None,
        };
    }
    parse_hex(&color.raw)
}

pub(super) fn opt_color_to_iced(color: &Option<AstColor>) -> Option<Color> {
    color.as_ref().and_then(color_to_iced)
}

fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 && s.len() != 8 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
    let a = if s.len() == 8 {
        u8::from_str_radix(&s[6..8], 16).ok()? as f32 / 255.0
    } else {
        1.0
    };
    Some(Color { r, g, b, a })
}

pub(super) fn align_x_to_iced(a: AstAlignX) -> Alignment {
    match a {
        AstAlignX::Start => Alignment::Start,
        AstAlignX::Center => Alignment::Center,
        AstAlignX::End => Alignment::End,
    }
}

pub(super) fn align_y_to_iced(a: AstAlignY) -> Alignment {
    match a {
        AstAlignY::Start => Alignment::Start,
        AstAlignY::Center => Alignment::Center,
        AstAlignY::End => Alignment::End,
    }
}

/// Translate an `AstBorder` into an iced `Border`. Missing color → fully
/// transparent (matches `Border::default()`).
pub(super) fn border_to_iced(b: &AstBorder) -> Border {
    let color = opt_color_to_iced(&b.color).unwrap_or(Color::TRANSPARENT);
    Border {
        color,
        width: b.width,
        radius: b.radius.into(),
    }
}

pub(super) fn shadow_to_iced(s: &AstShadow) -> Shadow {
    Shadow {
        color: opt_color_to_iced(&s.color).unwrap_or(Color::TRANSPARENT),
        offset: Vector::new(s.offset_x, s.offset_y),
        blur_radius: s.blur_radius,
    }
}

/// Reject any plugin-supplied path that could escape the sandbox root via
/// `..`, `.`, absolute prefixes, drive letters, null bytes, or overlong
/// strings. Shared by every widget that resolves a plugin asset.
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

/// Render a structured decode error / limit hit as a visible widget.
/// Plugin authors see "this slot is broken" instead of empty space; the
/// host emits the matching `widget.*` diagnostic in parallel so devtools
/// can drill in.
pub fn build_error_widget(code: &str, message: &str) -> Element<'static, Message> {
    let truncated = if message.len() > 256 {
        let mut t = message[..256].to_string();
        t.push_str("...");
        t
    } else {
        message.to_string()
    };
    let body: Element<'static, Message> = iced::widget::column![
        text(format!("[{code}]"))
            .size(11.0)
            .style(|_: &Theme| iced::widget::text::Style {
                color: Some(Color {
                    r: 1.0,
                    g: 0.5,
                    b: 0.5,
                    a: 1.0
                }),
            }),
        text(truncated)
            .size(12.0)
            .style(|_: &Theme| iced::widget::text::Style {
                color: Some(Color {
                    r: 1.0,
                    g: 0.85,
                    b: 0.85,
                    a: 1.0,
                }),
            }),
    ]
    .spacing(2.0)
    .into();
    iced::widget::container(body)
        .padding(iced::Padding {
            top: 4.0,
            right: 6.0,
            bottom: 4.0,
            left: 6.0,
        })
        .style(|_: &Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(Color {
                r: 0.18,
                g: 0.05,
                b: 0.05,
                a: 1.0,
            })),
            border: Border {
                color: Color {
                    r: 1.0,
                    g: 0.3,
                    b: 0.3,
                    a: 1.0,
                },
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_color_hex() {
        let c = color_to_iced(&AstColor {
            raw: "#ff8800".into(),
            token: None,
        })
        .unwrap();
        assert!((c.r - 1.0).abs() < 0.01);
        assert!((c.g - 0.533).abs() < 0.01);
        assert!((c.b - 0.0).abs() < 0.01);
        assert!((c.a - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_color_hex_with_alpha() {
        let c = color_to_iced(&AstColor {
            raw: "#00000080".into(),
            token: None,
        })
        .unwrap();
        assert!((c.r - 0.0).abs() < 0.01);
        assert!((c.g - 0.0).abs() < 0.01);
        assert!((c.b - 0.0).abs() < 0.01);
        assert!((c.a - 0.502).abs() < 0.01);
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
    }

    #[test]
    fn length_helpers_propagate_explicit_and_default() {
        assert!(matches!(
            length_or(AstLength::Fill, Length::Shrink),
            Length::Fill
        ));
        assert!(matches!(
            length_or(AstLength::Auto, Length::Shrink),
            Length::Shrink
        ));
        assert!(length_explicit(AstLength::Auto).is_none());
        assert!(matches!(
            length_explicit(AstLength::Fixed(7.5)),
            Some(Length::Fixed(_))
        ));
    }

    #[test]
    fn align_helpers_translate_x_and_y() {
        assert!(matches!(
            align_x_to_iced(AstAlignX::Center),
            Alignment::Center
        ));
        assert!(matches!(align_y_to_iced(AstAlignY::End), Alignment::End));
    }

    #[test]
    fn border_translates_with_default_color_when_missing() {
        let b = border_to_iced(&AstBorder {
            width: 2.0,
            radius: 4.0,
            color: None,
        });
        assert!((b.width - 2.0).abs() < 0.001);
        assert_eq!(b.color, Color::TRANSPARENT);
    }

    #[test]
    fn error_widget_builds() {
        let _ = build_error_widget("widget.unknown_kind", "kind 'oof' is not known");
    }

    #[test]
    fn error_widget_truncates_long_messages() {
        let long: String = "x".repeat(1024);
        // Just confirm it returns without panicking; the truncation
        // logic is a string slice + suffix which we exercise with this
        // input.
        let _ = build_error_widget("widget.string_too_long", &long);
    }
}

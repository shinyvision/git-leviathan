use iced::Color;

use crate::theme;

pub const LANE_COUNT: usize = theme::LANE_COLORS.len();

/// Semantic roles for palette lookups. Replaces ad-hoc `LANE_COLORS[idx]`
/// indexing and scattered conflict/tag color literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteRole {
    Lane(usize),
    ConflictOurs,
    ConflictTheirs,
    Tag,
}

pub fn palette_color(role: PaletteRole) -> Color {
    match role {
        PaletteRole::Lane(idx) => theme::LANE_COLORS[idx % theme::LANE_COLORS.len()],
        PaletteRole::ConflictOurs => Color {
            r: 0.024,
            g: 0.760,
            b: 0.910,
            a: 1.0,
        },
        PaletteRole::ConflictTheirs => Color {
            r: 0.945,
            g: 0.765,
            b: 0.180,
            a: 1.0,
        },
        PaletteRole::Tag => Color {
            r: 0.95,
            g: 0.90,
            b: 0.73,
            a: 1.0,
        },
    }
}

pub fn lane_color(idx: usize) -> Color {
    palette_color(PaletteRole::Lane(idx))
}

/// Linear blend of `base` toward `color` by `weight` (0..=1). Alpha forced to 1.
pub fn mix(base: Color, color: Color, weight: f32) -> Color {
    let base_weight = 1.0 - weight;
    Color {
        r: base.r * base_weight + color.r * weight,
        g: base.g * base_weight + color.g * weight,
        b: base.b * base_weight + color.b * weight,
        a: 1.0,
    }
}

/// Scale RGB by `factor`, keep alpha.
pub fn scale(c: Color, factor: f32) -> Color {
    Color {
        r: c.r * factor,
        g: c.g * factor,
        b: c.b * factor,
        a: c.a,
    }
}

/// `c * factor + lift` per channel, clamped to 1.0. Used to produce a lighter
/// tinted variant for text on dark backgrounds.
pub fn tint(c: Color, factor: f32, lift: f32) -> Color {
    Color {
        r: (c.r * factor + lift).min(1.0),
        g: (c.g * factor + lift).min(1.0),
        b: (c.b * factor + lift).min(1.0),
        a: 1.0,
    }
}

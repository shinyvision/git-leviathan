//! Phase 17 decoration ASTs.
//!
//! Plugins describe graph and diff decorations as small typed values
//! the host renders verbatim. The trees are pure data so devtools and
//! tests can inspect / round-trip them without touching any
//! renderer.

use serde::{Deserialize, Serialize};

/// One decoration attached to a commit row in the graph view.
///
/// Variants are intentionally narrow so the renderer can stay
/// declarative — plugins choose between four shapes and the host
/// owns layout / theming.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GraphDecoration {
    /// Short pill of text rendered next to the commit summary.
    Badge {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fg: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bg: Option<String>,
    },
    /// Named glyph drawn in the row's icon column.
    Icon {
        glyph: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
    },
    /// Small marker shape (dot / square / triangle) on the lane.
    Marker { shape: MarkerShape, color: String },
    /// Highlight a graph lane in a custom color.
    Lane { index: u32, color: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerShape {
    Dot,
    Square,
    Triangle,
}

impl GraphDecoration {
    pub fn kind(&self) -> &'static str {
        match self {
            GraphDecoration::Badge { .. } => "badge",
            GraphDecoration::Icon { .. } => "icon",
            GraphDecoration::Marker { .. } => "marker",
            GraphDecoration::Lane { .. } => "lane",
        }
    }
}

/// One decoration attached to a diff line or hunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffDecoration {
    /// Inline hint rendered next to a line (`info`/`warn`/`error`).
    LineHint {
        severity: HintSeverity,
        text: String,
        file: String,
        line: u32,
    },
    /// Badge rendered against a hunk header.
    HunkBadge {
        hunk_id: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
    },
    /// Glyph rendered in the gutter beside a specific line.
    LineGutter {
        file: String,
        line: u32,
        glyph: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HintSeverity {
    Info,
    Warn,
    Error,
}

impl DiffDecoration {
    pub fn kind(&self) -> &'static str {
        match self {
            DiffDecoration::LineHint { .. } => "line_hint",
            DiffDecoration::HunkBadge { .. } => "hunk_badge",
            DiffDecoration::LineGutter { .. } => "line_gutter",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_decoration_round_trip_badge() {
        let v = GraphDecoration::Badge {
            text: "WIP".into(),
            fg: Some("#fff".into()),
            bg: None,
        };
        let s = serde_json::to_string(&v).unwrap();
        let back: GraphDecoration = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);
        assert_eq!(v.kind(), "badge");
    }

    #[test]
    fn diff_decoration_kinds() {
        let lh = DiffDecoration::LineHint {
            severity: HintSeverity::Warn,
            text: "trailing whitespace".into(),
            file: "src/x.rs".into(),
            line: 10,
        };
        assert_eq!(lh.kind(), "line_hint");
        let hb = DiffDecoration::HunkBadge {
            hunk_id: "h1".into(),
            label: "+5/-1".into(),
            color: Some("#00ff00".into()),
        };
        assert_eq!(hb.kind(), "hunk_badge");
        let lg = DiffDecoration::LineGutter {
            file: "src/x.rs".into(),
            line: 12,
            glyph: ">".into(),
            color: None,
        };
        assert_eq!(lg.kind(), "line_gutter");
    }
}

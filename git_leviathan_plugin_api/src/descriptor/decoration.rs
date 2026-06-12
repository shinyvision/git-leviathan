//! extension points decoration ASTs.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecorationFieldDescriptor {
    pub name: &'static str,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecorationDescriptor {
    pub family: &'static str,
    pub kind: &'static str,
    pub fields: &'static [DecorationFieldDescriptor],
}

const fn req(name: &'static str) -> DecorationFieldDescriptor {
    DecorationFieldDescriptor {
        name,
        required: true,
    }
}

const fn opt(name: &'static str) -> DecorationFieldDescriptor {
    DecorationFieldDescriptor {
        name,
        required: false,
    }
}

/// Authoritative field schema for every graph/diff decoration kind,
/// mirroring [`GraphDecoration`] / [`DiffDecoration`]. Devtools (the
/// plugin linter) consume this instead of hand-maintaining the same
/// field lists. A `#[cfg(test)]` assertion below keeps it in sync with
/// the enum variants.
pub const DECORATIONS: &[DecorationDescriptor] = &[
    DecorationDescriptor {
        family: "graph",
        kind: "badge",
        fields: &[req("text"), opt("fg"), opt("bg")],
    },
    DecorationDescriptor {
        family: "graph",
        kind: "icon",
        fields: &[req("glyph"), opt("color")],
    },
    DecorationDescriptor {
        family: "graph",
        kind: "marker",
        fields: &[req("shape"), req("color")],
    },
    DecorationDescriptor {
        family: "graph",
        kind: "lane",
        fields: &[req("index"), req("color")],
    },
    DecorationDescriptor {
        family: "diff",
        kind: "line_hint",
        fields: &[req("severity"), req("text"), req("file"), req("line")],
    },
    DecorationDescriptor {
        family: "diff",
        kind: "hunk_badge",
        fields: &[req("hunk_id"), req("label"), opt("color")],
    },
    DecorationDescriptor {
        family: "diff",
        kind: "line_gutter",
        fields: &[req("file"), req("line"), req("glyph"), opt("color")],
    },
];

pub fn decoration_descriptor(family: &str, kind: &str) -> Option<&'static DecorationDescriptor> {
    DECORATIONS
        .iter()
        .find(|d| d.family == family && d.kind == kind)
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

    fn json_field_names(value: &serde_json::Value) -> std::collections::BTreeSet<String> {
        value
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| *k != "kind")
            .cloned()
            .collect()
    }

    fn descriptor_field_names(family: &str, kind: &str) -> std::collections::BTreeSet<String> {
        decoration_descriptor(family, kind)
            .unwrap()
            .fields
            .iter()
            .map(|f| f.name.to_string())
            .collect()
    }

    #[test]
    fn decoration_descriptors_match_graph_variants() {
        let all = [
            GraphDecoration::Badge {
                text: "t".into(),
                fg: Some("#fff".into()),
                bg: Some("#000".into()),
            },
            GraphDecoration::Icon {
                glyph: "g".into(),
                color: Some("#fff".into()),
            },
            GraphDecoration::Marker {
                shape: MarkerShape::Dot,
                color: "#fff".into(),
            },
            GraphDecoration::Lane {
                index: 0,
                color: "#fff".into(),
            },
        ];
        for value in &all {
            let json = serde_json::to_value(value).unwrap();
            assert_eq!(
                json_field_names(&json),
                descriptor_field_names("graph", value.kind()),
                "graph decoration `{}` fields drifted from descriptor",
                value.kind()
            );
        }
        let required: std::collections::BTreeSet<&str> = DECORATIONS
            .iter()
            .filter(|d| d.family == "graph" && d.kind == "badge")
            .flat_map(|d| d.fields.iter())
            .filter(|f| f.required)
            .map(|f| f.name)
            .collect();
        let minimal = serde_json::to_value(GraphDecoration::Badge {
            text: "t".into(),
            fg: None,
            bg: None,
        })
        .unwrap();
        assert_eq!(
            json_field_names(&minimal),
            required.iter().map(|s| s.to_string()).collect()
        );
    }

    #[test]
    fn decoration_descriptors_match_diff_variants() {
        let all = [
            DiffDecoration::LineHint {
                severity: HintSeverity::Warn,
                text: "t".into(),
                file: "f".into(),
                line: 1,
            },
            DiffDecoration::HunkBadge {
                hunk_id: "h".into(),
                label: "l".into(),
                color: Some("#fff".into()),
            },
            DiffDecoration::LineGutter {
                file: "f".into(),
                line: 1,
                glyph: "g".into(),
                color: Some("#fff".into()),
            },
        ];
        for value in &all {
            let json = serde_json::to_value(value).unwrap();
            assert_eq!(
                json_field_names(&json),
                descriptor_field_names("diff", value.kind()),
                "diff decoration `{}` fields drifted from descriptor",
                value.kind()
            );
        }
        let minimal = serde_json::to_value(DiffDecoration::HunkBadge {
            hunk_id: "h".into(),
            label: "l".into(),
            color: None,
        })
        .unwrap();
        let required: std::collections::BTreeSet<String> =
            decoration_descriptor("diff", "hunk_badge")
                .unwrap()
                .fields
                .iter()
                .filter(|f| f.required)
                .map(|f| f.name.to_string())
                .collect();
        assert_eq!(json_field_names(&minimal), required);
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

//! Widget kind schemas. Mirrors the contract enforced by the runtime
//! widget builders in `src/plugin/bridge/widget_tree/*.rs`. Structs are
//! used for boundary validation and LSP stub generation; the runtime
//! still reads JSON values directly for now.
//!
//! Each widget struct accepts the field set its bridge counterpart reads,
//! with `#[serde(default)]` on every field the bridge treats as optional
//! (i.e. anything fetched via `node.get(...).and_then(...).unwrap_or(...)`).
//! Inner structs do **not** carry `deny_unknown_fields` — the bridge
//! tolerates extra fields silently and we want zero behavior change at the
//! validation boundary. The outer enum uses `#[serde(tag = "kind")]` to
//! discriminate; an unknown kind is rejected by serde's `untagged`/tagged
//! enum mechanics, which is the typo-catching behavior we want.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WidgetKind {
    Text(TextWidget),
    Button(ButtonWidget),
    Row(RowWidget),
    Column(ColumnWidget),
    Container(ContainerWidget),
    Padding(PaddingWidget),
    Space(SpaceWidget),
    Icon(IconWidget),
    Image(ImageWidget),
    Scrollable(ScrollableWidget),
    MouseArea(MouseAreaWidget),
    Tablist(TablistWidget),
    ResizableSplit(ResizableSplitWidget),
}

/// Length values: numbers are pixel sizes, strings are `"fill"` / `"shrink"`.
/// Mirrors `widget_tree::common::parse_length`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Length {
    Fixed(f32),
    Named(String),
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Border {
    #[serde(default)]
    pub width: Option<f32>,
    #[serde(default)]
    pub radius: Option<f32>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ButtonStyle {
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub background_hover: Option<String>,
    #[serde(default)]
    pub text_color: Option<String>,
    #[serde(default)]
    pub border: Option<Border>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TextWidget {
    // Bridge defaults missing `value` to "" so we keep it optional for parity.
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub size: Option<f32>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ButtonWidget {
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub on_click: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
    #[serde(default)]
    pub style: Option<ButtonStyle>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RowWidget {
    #[serde(default)]
    pub children: Vec<WidgetKind>,
    #[serde(default)]
    pub spacing: Option<f32>,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
    #[serde(default)]
    pub align_y: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ColumnWidget {
    #[serde(default)]
    pub children: Vec<WidgetKind>,
    #[serde(default)]
    pub spacing: Option<f32>,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
    #[serde(default)]
    pub align_x: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ContainerWidget {
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
    #[serde(default)]
    pub bg: Option<String>,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
    #[serde(default)]
    pub max_width: Option<f32>,
    #[serde(default)]
    pub max_height: Option<f32>,
    #[serde(default)]
    pub min_width: Option<f32>,
    #[serde(default)]
    pub min_height: Option<f32>,
    #[serde(default)]
    pub center_x: Option<bool>,
    #[serde(default)]
    pub center_y: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PaddingWidget {
    #[serde(default)]
    pub top: Option<f32>,
    #[serde(default)]
    pub right: Option<f32>,
    #[serde(default)]
    pub bottom: Option<f32>,
    #[serde(default)]
    pub left: Option<f32>,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SpaceWidget {
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct IconWidget {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub size: Option<f32>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImageWidget {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub size: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ScrollableWidget {
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MouseAreaWidget {
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
    #[serde(default)]
    pub on_click: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TabSpec {
    #[serde(default)]
    pub id: serde_json::Value,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TablistWidget {
    #[serde(default)]
    pub tabs: Vec<TabSpec>,
    #[serde(default)]
    pub active: Option<serde_json::Value>,
    #[serde(default)]
    pub orderable: Option<bool>,
    #[serde(default)]
    pub on_select: Option<String>,
    #[serde(default)]
    pub on_close: Option<String>,
    #[serde(default)]
    pub on_reorder: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ResizableSplitWidget {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub children: Vec<WidgetKind>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_for_text_widget() {
        let s = schemars::schema_for!(TextWidget);
        let v = serde_json::to_value(s).unwrap();
        let props = &v["properties"];
        assert!(props.get("value").is_some());
        assert!(props.get("size").is_some());
        assert!(props.get("color").is_some());
    }

    #[test]
    fn widget_kind_discriminates_by_kind_field() {
        let json = serde_json::json!({ "kind": "text", "value": "hello" });
        let w: WidgetKind = serde_json::from_value(json).unwrap();
        assert!(matches!(w, WidgetKind::Text(_)));
    }

    #[test]
    fn unknown_widget_kind_rejected() {
        let json = serde_json::json!({ "kind": "rwo", "children": [] });
        let err = serde_json::from_value::<WidgetKind>(json)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("rwo") || err.contains("unknown variant"),
            "got: {err}"
        );
    }

    #[test]
    fn nested_children_validate() {
        let json = serde_json::json!({
            "kind": "row",
            "children": [
                { "kind": "text", "value": "a" },
                { "kind": "text", "value": "b" },
            ],
        });
        let w: WidgetKind = serde_json::from_value(json).unwrap();
        if let WidgetKind::Row(r) = w {
            assert_eq!(r.children.len(), 2);
        } else {
            panic!("expected row");
        }
    }

    #[test]
    fn padding_with_child_validates() {
        let json = serde_json::json!({
            "kind": "padding",
            "top": 6, "right": 8, "bottom": 6, "left": 8,
            "child": { "kind": "text", "value": "hi" },
        });
        let _: WidgetKind = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn length_accepts_number_or_named() {
        let json = serde_json::json!({ "kind": "space", "width": "fill", "height": 10 });
        let _: WidgetKind = serde_json::from_value(json).unwrap();
    }

    #[test]
    fn dancing_banana_image_path_validates() {
        let json = serde_json::json!({
            "kind": "padding",
            "top": 0, "right": 0, "bottom": 0, "left": 0,
            "child": { "kind": "image", "path": "assets/dancing_banana.gif", "size": 25 },
        });
        let _: WidgetKind = serde_json::from_value(json).unwrap();
    }
}

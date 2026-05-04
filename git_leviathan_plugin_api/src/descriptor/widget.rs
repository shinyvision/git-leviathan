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

#[derive(Debug, Clone, Copy, Serialize)]
pub struct WidgetFieldDescriptor {
    pub name: &'static str,
    pub lua_type: &'static str,
    pub required: bool,
    pub doc: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct WidgetDescriptor {
    pub kind: &'static str,
    pub since: &'static str,
    pub doc: &'static str,
    pub fields: &'static [WidgetFieldDescriptor],
}

pub struct WidgetDescriptorTable(&'static [WidgetDescriptor]);

impl WidgetDescriptorTable {
    pub fn iter(&self) -> impl Iterator<Item = &WidgetDescriptor> {
        self.0.iter()
    }

    pub fn get(&self, kind: &str) -> Option<&WidgetDescriptor> {
        self.0.iter().find(|d| d.kind == kind)
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(|d| d.kind).collect()
    }
}

const SEMANTIC_WIDGET_FIELDS: &[WidgetFieldDescriptor] = &[
    WidgetFieldDescriptor {
        name: "label",
        lua_type: "string",
        required: false,
        doc: "Accessible label or visible fallback.",
    },
    WidgetFieldDescriptor {
        name: "title",
        lua_type: "string",
        required: false,
        doc: "Section or item title.",
    },
    WidgetFieldDescriptor {
        name: "text",
        lua_type: "string",
        required: false,
        doc: "Body text.",
    },
    WidgetFieldDescriptor {
        name: "command",
        lua_type: "string",
        required: false,
        doc: "Command id to invoke.",
    },
    WidgetFieldDescriptor {
        name: "on_click",
        lua_type: "string",
        required: false,
        doc: "Plugin event emitted on click.",
    },
    WidgetFieldDescriptor {
        name: "on_change",
        lua_type: "string",
        required: false,
        doc: "Plugin event emitted when value changes.",
    },
    WidgetFieldDescriptor {
        name: "disabled_reason",
        lua_type: "string",
        required: false,
        doc: "Reason shown when disabled.",
    },
    WidgetFieldDescriptor {
        name: "shortcut",
        lua_type: "string",
        required: false,
        doc: "Keyboard shortcut hint.",
    },
    WidgetFieldDescriptor {
        name: "child",
        lua_type: "LeviathanWidget",
        required: false,
        doc: "Primary nested widget.",
    },
    WidgetFieldDescriptor {
        name: "children",
        lua_type: "LeviathanWidget[]",
        required: false,
        doc: "Nested widgets.",
    },
    WidgetFieldDescriptor {
        name: "items",
        lua_type: "table[]",
        required: false,
        doc: "List, tree, or menu items.",
    },
    WidgetFieldDescriptor {
        name: "options",
        lua_type: "table[]",
        required: false,
        doc: "Select or radio options.",
    },
    WidgetFieldDescriptor {
        name: "color",
        lua_type: "string|{token:string}",
        required: false,
        doc: "Theme token or raw color where allowed.",
    },
];

const LAYOUT_WIDGET_FIELDS: &[WidgetFieldDescriptor] = &[
    WidgetFieldDescriptor {
        name: "children",
        lua_type: "LeviathanWidget[]",
        required: false,
        doc: "Child widgets.",
    },
    WidgetFieldDescriptor {
        name: "tabs",
        lua_type: "table[]",
        required: false,
        doc: "Tab descriptors with child widgets.",
    },
    WidgetFieldDescriptor {
        name: "direction",
        lua_type: "string",
        required: false,
        doc: "horizontal or vertical.",
    },
    WidgetFieldDescriptor {
        name: "columns",
        lua_type: "number",
        required: false,
        doc: "Grid column count.",
    },
    WidgetFieldDescriptor {
        name: "spacing",
        lua_type: "number|{token:string}",
        required: false,
        doc: "Theme spacing token or pixels.",
    },
    WidgetFieldDescriptor {
        name: "focus_order",
        lua_type: "number",
        required: false,
        doc: "Keyboard focus ordering hint.",
    },
];

const fn semantic_descriptor(kind: &'static str, doc: &'static str) -> WidgetDescriptor {
    WidgetDescriptor {
        kind,
        since: "1.0",
        doc,
        fields: SEMANTIC_WIDGET_FIELDS,
    }
}

const fn layout_descriptor(kind: &'static str, doc: &'static str) -> WidgetDescriptor {
    WidgetDescriptor {
        kind,
        since: "1.0",
        doc,
        fields: LAYOUT_WIDGET_FIELDS,
    }
}

pub static WIDGETS: WidgetDescriptorTable = WidgetDescriptorTable(&[
    WidgetDescriptor {
        kind: "text",
        since: "1.0",
        doc: "Static text label.",
        fields: &[
            WidgetFieldDescriptor {
                name: "value",
                lua_type: "string",
                required: false,
                doc: "Text content.",
            },
            WidgetFieldDescriptor {
                name: "size",
                lua_type: "number",
                required: false,
                doc: "Font size in pixels.",
            },
            WidgetFieldDescriptor {
                name: "color",
                lua_type: "string",
                required: false,
                doc: "CSS-style color string.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "button",
        since: "1.0",
        doc: "Clickable button that emits a widget event.",
        fields: &[
            WidgetFieldDescriptor {
                name: "child",
                lua_type: "LeviathanWidget",
                required: false,
                doc: "Nested widget content.",
            },
            WidgetFieldDescriptor {
                name: "text",
                lua_type: "string",
                required: false,
                doc: "Fallback text content.",
            },
            WidgetFieldDescriptor {
                name: "on_click",
                lua_type: "string",
                required: false,
                doc: "Event name emitted on click.",
            },
            WidgetFieldDescriptor {
                name: "value",
                lua_type: "LeviathanJson",
                required: false,
                doc: "Event payload.",
            },
            WidgetFieldDescriptor {
                name: "width",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "height",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "style",
                lua_type: "table",
                required: false,
                doc: "Button style overrides.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "text_input",
        since: "1.0",
        doc: "Single-line text input that emits plugin events.",
        fields: &[
            WidgetFieldDescriptor {
                name: "id",
                lua_type: "string",
                required: false,
                doc: "Stable node id used to preserve input focus across re-renders.",
            },
            WidgetFieldDescriptor {
                name: "placeholder",
                lua_type: "string",
                required: true,
                doc: "Placeholder text shown when the value is empty.",
            },
            WidgetFieldDescriptor {
                name: "value",
                lua_type: "string",
                required: true,
                doc: "Current input value.",
            },
            WidgetFieldDescriptor {
                name: "on_input",
                lua_type: "string",
                required: true,
                doc: "Event emitted with the new string value after edits.",
            },
            WidgetFieldDescriptor {
                name: "on_submit",
                lua_type: "string",
                required: false,
                doc: "Event emitted with null when Enter is pressed.",
            },
            WidgetFieldDescriptor {
                name: "width",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "height",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "autofocus",
                lua_type: "boolean",
                required: false,
                doc: "Focus this input when its overlay is opened.",
            },
            WidgetFieldDescriptor {
                name: "style",
                lua_type: "table",
                required: false,
                doc: "Text input style overrides.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "row",
        since: "1.0",
        doc: "Horizontal widget layout.",
        fields: &[
            WidgetFieldDescriptor {
                name: "children",
                lua_type: "LeviathanWidget[]",
                required: false,
                doc: "Child widgets.",
            },
            WidgetFieldDescriptor {
                name: "spacing",
                lua_type: "number",
                required: false,
                doc: "Gap between children.",
            },
            WidgetFieldDescriptor {
                name: "width",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "height",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "align_y",
                lua_type: "string",
                required: false,
                doc: "Vertical alignment.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "column",
        since: "1.0",
        doc: "Vertical widget layout.",
        fields: &[
            WidgetFieldDescriptor {
                name: "children",
                lua_type: "LeviathanWidget[]",
                required: false,
                doc: "Child widgets.",
            },
            WidgetFieldDescriptor {
                name: "spacing",
                lua_type: "number",
                required: false,
                doc: "Gap between children.",
            },
            WidgetFieldDescriptor {
                name: "width",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "height",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "align_x",
                lua_type: "string",
                required: false,
                doc: "Horizontal alignment.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "container",
        since: "1.0",
        doc: "Single-child layout and background wrapper.",
        fields: &[
            WidgetFieldDescriptor {
                name: "child",
                lua_type: "LeviathanWidget",
                required: false,
                doc: "Nested widget content.",
            },
            WidgetFieldDescriptor {
                name: "bg",
                lua_type: "string",
                required: false,
                doc: "Background color.",
            },
            WidgetFieldDescriptor {
                name: "width",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "height",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "max_width",
                lua_type: "number",
                required: false,
                doc: "Maximum width.",
            },
            WidgetFieldDescriptor {
                name: "max_height",
                lua_type: "number",
                required: false,
                doc: "Maximum height.",
            },
            WidgetFieldDescriptor {
                name: "min_width",
                lua_type: "number",
                required: false,
                doc: "Minimum width.",
            },
            WidgetFieldDescriptor {
                name: "min_height",
                lua_type: "number",
                required: false,
                doc: "Minimum height.",
            },
            WidgetFieldDescriptor {
                name: "center_x",
                lua_type: "boolean",
                required: false,
                doc: "Center child horizontally.",
            },
            WidgetFieldDescriptor {
                name: "center_y",
                lua_type: "boolean",
                required: false,
                doc: "Center child vertically.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "padding",
        since: "1.0",
        doc: "Single-child padding wrapper.",
        fields: &[
            WidgetFieldDescriptor {
                name: "top",
                lua_type: "number",
                required: false,
                doc: "Top inset.",
            },
            WidgetFieldDescriptor {
                name: "right",
                lua_type: "number",
                required: false,
                doc: "Right inset.",
            },
            WidgetFieldDescriptor {
                name: "bottom",
                lua_type: "number",
                required: false,
                doc: "Bottom inset.",
            },
            WidgetFieldDescriptor {
                name: "left",
                lua_type: "number",
                required: false,
                doc: "Left inset.",
            },
            WidgetFieldDescriptor {
                name: "width",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "height",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "child",
                lua_type: "LeviathanWidget",
                required: false,
                doc: "Nested widget content.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "space",
        since: "1.0",
        doc: "Empty spacer.",
        fields: &[
            WidgetFieldDescriptor {
                name: "width",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "height",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "icon",
        since: "1.0",
        doc: "SVG icon loaded from plugin assets.",
        fields: &[
            WidgetFieldDescriptor {
                name: "path",
                lua_type: "string",
                required: false,
                doc: "Asset path.",
            },
            WidgetFieldDescriptor {
                name: "size",
                lua_type: "number",
                required: false,
                doc: "Icon size.",
            },
            WidgetFieldDescriptor {
                name: "color",
                lua_type: "string",
                required: false,
                doc: "Tint color.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "image",
        since: "1.0",
        doc: "Raster image loaded from plugin assets.",
        fields: &[
            WidgetFieldDescriptor {
                name: "path",
                lua_type: "string",
                required: false,
                doc: "Asset path.",
            },
            WidgetFieldDescriptor {
                name: "size",
                lua_type: "number",
                required: false,
                doc: "Rendered square size.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "scrollable",
        since: "1.0",
        doc: "Scrollable single-child container.",
        fields: &[
            WidgetFieldDescriptor {
                name: "child",
                lua_type: "LeviathanWidget",
                required: false,
                doc: "Nested widget content.",
            },
            WidgetFieldDescriptor {
                name: "width",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
            WidgetFieldDescriptor {
                name: "height",
                lua_type: "number|string",
                required: false,
                doc: "Fixed pixels, fill, or shrink.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "mouse_area",
        since: "1.0",
        doc: "Clickable wrapper around a child widget.",
        fields: &[
            WidgetFieldDescriptor {
                name: "child",
                lua_type: "LeviathanWidget",
                required: false,
                doc: "Nested widget content.",
            },
            WidgetFieldDescriptor {
                name: "on_click",
                lua_type: "string",
                required: false,
                doc: "Event name emitted on click.",
            },
            WidgetFieldDescriptor {
                name: "value",
                lua_type: "LeviathanJson",
                required: false,
                doc: "Event payload.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "tablist",
        since: "1.0",
        doc: "Tab strip widget backed by plugin-supplied tabs.",
        fields: &[
            WidgetFieldDescriptor {
                name: "tabs",
                lua_type: "table[]",
                required: false,
                doc: "Tab id/name list.",
            },
            WidgetFieldDescriptor {
                name: "active",
                lua_type: "LeviathanJson",
                required: false,
                doc: "Active tab id.",
            },
            WidgetFieldDescriptor {
                name: "orderable",
                lua_type: "boolean",
                required: false,
                doc: "Whether drag reorder is enabled.",
            },
            WidgetFieldDescriptor {
                name: "on_select",
                lua_type: "string",
                required: false,
                doc: "Selection event name.",
            },
            WidgetFieldDescriptor {
                name: "on_close",
                lua_type: "string",
                required: false,
                doc: "Close event name.",
            },
            WidgetFieldDescriptor {
                name: "on_reorder",
                lua_type: "string",
                required: false,
                doc: "Reorder event name.",
            },
        ],
    },
    WidgetDescriptor {
        kind: "resizable_split",
        since: "1.0",
        doc: "Resizable split layout.",
        fields: &[
            WidgetFieldDescriptor {
                name: "id",
                lua_type: "string",
                required: false,
                doc: "Stable split id.",
            },
            WidgetFieldDescriptor {
                name: "direction",
                lua_type: "string",
                required: false,
                doc: "Split direction.",
            },
            WidgetFieldDescriptor {
                name: "children",
                lua_type: "LeviathanWidget[]",
                required: false,
                doc: "Child panels.",
            },
        ],
    },
    semantic_descriptor("command_button", "Button that invokes a command id."),
    semantic_descriptor("toolbar_button", "Compact toolbar action button."),
    semantic_descriptor("status_item", "Status-line item."),
    semantic_descriptor("badge", "Small status badge."),
    semantic_descriptor("tag", "Small tag pill."),
    semantic_descriptor("list", "Semantic list."),
    semantic_descriptor("tree", "Semantic tree."),
    semantic_descriptor("table", "Semantic table."),
    semantic_descriptor("section", "Titled content section."),
    semantic_descriptor("form", "Form field group."),
    semantic_descriptor("checkbox", "Checkbox control."),
    semantic_descriptor("toggle", "Toggle control."),
    semantic_descriptor("select", "Select control."),
    semantic_descriptor("radio_group", "Radio option group."),
    semantic_descriptor("divider", "Visual divider."),
    semantic_descriptor("tooltip", "Tooltip wrapper."),
    semantic_descriptor("popover", "Popover wrapper."),
    semantic_descriptor("menu", "Menu list."),
    semantic_descriptor("empty_state", "Empty-state presentation."),
    semantic_descriptor("code", "Code text block."),
    semantic_descriptor("diff", "Diff text block."),
    semantic_descriptor("commit_ref", "Commit reference chip."),
    semantic_descriptor("branch_ref", "Branch reference chip."),
    semantic_descriptor("remote_ref", "Remote reference chip."),
    semantic_descriptor("progress", "Progress indicator."),
    layout_descriptor("stack", "Stack children vertically."),
    layout_descriptor("grid", "Grid layout."),
    layout_descriptor("dock", "Dock-style layout primitive."),
    layout_descriptor("split", "Semantic split layout primitive."),
    layout_descriptor("tabs", "Tabbed layout primitive."),
    layout_descriptor("virtual_list", "Large-list layout primitive."),
]);

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WidgetKind {
    Text(TextWidget),
    Button(ButtonWidget),
    TextInput(TextInputWidget),
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
    CommandButton(SemanticWidget),
    ToolbarButton(SemanticWidget),
    StatusItem(SemanticWidget),
    Badge(SemanticWidget),
    Tag(SemanticWidget),
    List(SemanticWidget),
    Tree(SemanticWidget),
    Table(SemanticWidget),
    Section(SemanticWidget),
    Form(SemanticWidget),
    Checkbox(SemanticWidget),
    Toggle(SemanticWidget),
    Select(SemanticWidget),
    RadioGroup(SemanticWidget),
    Divider(SemanticWidget),
    Tooltip(SemanticWidget),
    Popover(SemanticWidget),
    Menu(SemanticWidget),
    EmptyState(SemanticWidget),
    Code(SemanticWidget),
    Diff(SemanticWidget),
    CommitRef(SemanticWidget),
    BranchRef(SemanticWidget),
    RemoteRef(SemanticWidget),
    Progress(SemanticWidget),
    Stack(LayoutWidget),
    Grid(LayoutWidget),
    Dock(LayoutWidget),
    Split(LayoutWidget),
    Tabs(LayoutWidget),
    VirtualList(LayoutWidget),
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
#[serde(untagged)]
pub enum TokenValue {
    Raw(String),
    Token { token: String },
}

pub type ColorValue = TokenValue;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SpacingValue {
    Fixed(f32),
    Token { token: String },
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct WidgetMeta {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub focus_order: Option<i32>,
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AssetHandle {
    pub path: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub handle: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Border {
    #[serde(default)]
    pub width: Option<f32>,
    #[serde(default)]
    pub radius: Option<f32>,
    #[serde(default)]
    pub color: Option<ColorValue>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ButtonStyle {
    #[serde(default)]
    pub background: Option<ColorValue>,
    #[serde(default)]
    pub background_hover: Option<ColorValue>,
    #[serde(default)]
    pub text_color: Option<ColorValue>,
    #[serde(default)]
    pub border: Option<Border>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TextInputStyle {
    #[serde(default)]
    pub background: Option<ColorValue>,
    #[serde(default)]
    pub text_color: Option<ColorValue>,
    #[serde(default)]
    pub placeholder_color: Option<ColorValue>,
    #[serde(default)]
    pub border: Option<Border>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TextWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    // Bridge defaults missing `value` to "" so we keep it optional for parity.
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub size: Option<f32>,
    #[serde(default)]
    pub color: Option<ColorValue>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ButtonWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
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
pub struct TextInputWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub id: Option<String>,
    pub placeholder: String,
    pub value: String,
    pub on_input: String,
    #[serde(default)]
    pub on_submit: Option<String>,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
    #[serde(default)]
    pub autofocus: Option<bool>,
    #[serde(default)]
    pub style: Option<TextInputStyle>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RowWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub children: Vec<WidgetKind>,
    #[serde(default)]
    pub spacing: Option<SpacingValue>,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
    #[serde(default)]
    pub align_y: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ColumnWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub children: Vec<WidgetKind>,
    #[serde(default)]
    pub spacing: Option<SpacingValue>,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
    #[serde(default)]
    pub align_x: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ContainerWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
    #[serde(default)]
    pub bg: Option<ColorValue>,
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
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub top: Option<SpacingValue>,
    #[serde(default)]
    pub right: Option<SpacingValue>,
    #[serde(default)]
    pub bottom: Option<SpacingValue>,
    #[serde(default)]
    pub left: Option<SpacingValue>,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SpaceWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct IconWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub asset: Option<AssetHandle>,
    #[serde(default)]
    pub size: Option<f32>,
    #[serde(default)]
    pub color: Option<ColorValue>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImageWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub asset: Option<AssetHandle>,
    #[serde(default)]
    pub size: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ScrollableWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
    #[serde(default)]
    pub width: Option<Length>,
    #[serde(default)]
    pub height: Option<Length>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MouseAreaWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
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
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
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
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub children: Vec<WidgetKind>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SemanticItem {
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
    #[serde(default)]
    pub children: Vec<SemanticItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SemanticColumn {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub width: Option<Length>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SemanticRow {
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    #[serde(default)]
    pub cells: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SemanticOption {
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SemanticWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub on_click: Option<String>,
    #[serde(default)]
    pub on_change: Option<String>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub checked: Option<bool>,
    #[serde(default)]
    pub selected: Option<serde_json::Value>,
    #[serde(default)]
    pub progress: Option<f32>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub color: Option<ColorValue>,
    #[serde(default)]
    pub spacing: Option<SpacingValue>,
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
    #[serde(default)]
    pub children: Vec<WidgetKind>,
    #[serde(default)]
    pub items: Vec<SemanticItem>,
    #[serde(default)]
    pub columns: Vec<SemanticColumn>,
    #[serde(default)]
    pub rows: Vec<SemanticRow>,
    #[serde(default)]
    pub options: Vec<SemanticOption>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct LayoutTabWidget {
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct LayoutWidget {
    #[serde(default, flatten)]
    pub meta: WidgetMeta,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub columns: Option<usize>,
    #[serde(default)]
    pub spacing: Option<SpacingValue>,
    #[serde(default)]
    pub active: Option<serde_json::Value>,
    #[serde(default)]
    pub child: Option<Box<WidgetKind>>,
    #[serde(default)]
    pub children: Vec<WidgetKind>,
    #[serde(default)]
    pub tabs: Vec<LayoutTabWidget>,
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
    fn text_input_widget_validates_required_fields() {
        let json = serde_json::json!({
            "kind": "text_input",
            "id": "palette-query",
            "placeholder": "Run command",
            "value": "checkout",
            "on_input": "palette.input",
            "on_submit": "palette.submit",
            "width": "fill",
            "height": 32,
            "autofocus": true,
            "style": {
                "background": "#101119",
                "text_color": "#e1e5f4",
                "placeholder_color": "#585d6e",
                "border": { "width": 1, "radius": 4, "color": "#242535" }
            }
        });
        let w: WidgetKind = serde_json::from_value(json).unwrap();
        if let WidgetKind::TextInput(input) = w {
            assert_eq!(input.id.as_deref(), Some("palette-query"));
            assert_eq!(input.placeholder, "Run command");
            assert_eq!(input.value, "checkout");
            assert_eq!(input.on_input, "palette.input");
            assert_eq!(input.on_submit.as_deref(), Some("palette.submit"));
            assert_eq!(input.autofocus, Some(true));
        } else {
            panic!("expected text_input");
        }
    }

    #[test]
    fn text_input_widget_requires_on_input() {
        let json = serde_json::json!({
            "kind": "text_input",
            "placeholder": "Run command",
            "value": ""
        });
        let err = serde_json::from_value::<WidgetKind>(json)
            .unwrap_err()
            .to_string();
        assert!(err.contains("on_input"), "got: {err}");
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
    fn widget_descriptors_match_schema_kinds() {
        let schema = schemars::schema_for!(WidgetKind);
        let value = serde_json::to_value(schema).unwrap();
        let mut schema_kinds = value["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|variant| variant["properties"]["kind"]["enum"][0].as_str())
            .collect::<Vec<_>>();
        schema_kinds.sort_unstable();

        let mut descriptor_kinds = WIDGETS.names();
        descriptor_kinds.sort_unstable();

        assert_eq!(descriptor_kinds, schema_kinds);
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

    #[test]
    fn semantic_widgets_validate() {
        let json = serde_json::json!({
            "kind": "command_button",
            "label": "Fetch",
            "command": "repository.fetch",
            "shortcut": "Ctrl+R",
            "disabled_reason": "No repository"
        });
        let widget: WidgetKind = serde_json::from_value(json).unwrap();
        assert!(matches!(widget, WidgetKind::CommandButton(_)));
    }

    #[test]
    fn theme_tokens_and_asset_handles_validate() {
        let json = serde_json::json!({
            "kind": "icon",
            "asset": { "path": "icons/foo.svg", "kind": "svg", "handle": "asset:svg:icons/foo.svg" },
            "color": { "token": "text.primary" }
        });
        let widget: WidgetKind = serde_json::from_value(json).unwrap();
        assert!(matches!(widget, WidgetKind::Icon(_)));
    }

    #[test]
    fn layout_widgets_validate() {
        let json = serde_json::json!({
            "kind": "tabs",
            "active": "a",
            "tabs": [{ "id": "a", "title": "A", "child": { "kind": "text", "value": "A" } }]
        });
        let widget: WidgetKind = serde_json::from_value(json).unwrap();
        assert!(matches!(widget, WidgetKind::Tabs(_)));
    }
}

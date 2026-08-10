//! Typed widget AST for plugin UI.

use std::fmt;

use serde_json::Value;

use crate::plugin::ui::widget_ast_semantic::{
    self, AstAssetRef, LayoutNode, LayoutTab, SemanticColumn, SemanticItem, SemanticNode,
    SemanticOption, SemanticRow, WidgetMeta,
};

/// Hard ceilings on decoded widget trees.
#[derive(Debug, Clone, Copy)]
pub struct WidgetLimits {
    pub max_tree_depth: usize,
    pub max_node_count: usize,
    pub max_string_length: usize,
    pub max_asset_path_length: usize,
    pub max_image_size_bytes: u64,
}

impl WidgetLimits {
    pub const DEFAULT: Self = Self {
        max_tree_depth: 64,
        max_node_count: 4096,
        max_string_length: 64 * 1024,
        max_asset_path_length: 256,
        max_image_size_bytes: 8 * 1024 * 1024,
    };
}

impl Default for WidgetLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Stable error codes emitted by the AST decoder. Mirrors the typed diagnostics
/// diagnostic-code convention (`widget.<short>`). Surfaces of these
/// strings:
/// - `PluginDiagnostic::code`
/// - the in-place error widget
/// - assertions in tests
pub mod codes {
    pub const UNKNOWN_KIND: &str = "widget.unknown_kind";
    pub const FIELD_MISSING: &str = "widget.field_missing";
    pub const FIELD_TYPE_MISMATCH: &str = "widget.field_type_mismatch";
    pub const DEPTH_EXCEEDED: &str = "widget.depth_exceeded";
    pub const NODE_COUNT_EXCEEDED: &str = "widget.node_count_exceeded";
    pub const STRING_TOO_LONG: &str = "widget.string_too_long";
    pub const IMAGE_TOO_LARGE: &str = "widget.image_too_large";
    pub const ASSET_LIMIT_EXCEEDED: &str = "widget.asset_limit_exceeded";
    pub const NOT_A_TABLE: &str = "widget.not_a_table";
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// One field-level decode error. Renders to a `PluginDiagnostic` via
/// `into_diagnostic` (host wires this up so tests don't depend on the
/// diagnostic store directly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetDecodeError {
    /// One of the `widget.*` codes from [`codes`].
    pub code: &'static str,
    /// Path inside the widget tree, e.g. `root.children[2].child.label`.
    pub path: String,
    /// Human-readable message. Plugin authors read this; keep it
    /// specific.
    pub message: String,
}

impl WidgetDecodeError {
    pub fn new(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for WidgetDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}: {}", self.code, self.path, self.message)
    }
}

impl std::error::Error for WidgetDecodeError {}

// ---------------------------------------------------------------------------
// AST core types
// ---------------------------------------------------------------------------

/// `width` / `height` lengths. Matches the runtime
/// `bridge::widget_tree::common::parse_length` vocabulary. `Auto` means
/// "let the widget pick its own default" (each widget kind has different
/// preferred defaults, so we leave that decision to the renderer rather
/// than baking it in here).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AstLength {
    Auto,
    Fill,
    Shrink,
    Fixed(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AstAlignX {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AstAlignY {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstColor {
    pub raw: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstBorder {
    pub width: f32,
    pub radius: f32,
    pub color: Option<AstColor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstFont {
    Default,
    Mono,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstShadow {
    pub color: Option<AstColor>,
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AstButtonStyle {
    pub background: Option<AstColor>,
    pub background_hover: Option<AstColor>,
    pub text_color: Option<AstColor>,
    pub border: Option<AstBorder>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AstTextInputStyle {
    pub background: Option<AstColor>,
    pub text_color: Option<AstColor>,
    pub placeholder_color: Option<AstColor>,
    pub border: Option<AstBorder>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AstSplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AstTabSpec {
    pub id: Value,
    pub name: String,
}

/// Stable identifier for one node in the tree. If the plugin set
/// `id = "..."` explicitly, that id is used (it's also recorded in
/// `explicit` so devtools can tell the difference). Otherwise the id is
/// the path to the node (`root.children[2].child`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeId {
    pub value: String,
    pub explicit: bool,
}

impl NodeId {
    pub fn from_path(path: &str) -> Self {
        Self {
            value: path.to_string(),
            explicit: false,
        }
    }
    pub fn explicit(value: String) -> Self {
        Self {
            value,
            explicit: true,
        }
    }
}

/// One AST node. Every variant carries a `node_id` for stable
/// identification and a `node` payload with the kind-specific fields.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetAst {
    pub node_id: NodeId,
    pub meta: WidgetMeta,
    pub node: WidgetNode,
}

/// All currently supported widget kinds. Mirrors the descriptor list in
/// `git_leviathan_plugin_api::descriptor::widget::WIDGETS`. Keep these
/// in lock-step: a new descriptor without a matching variant (or vice
/// versa) is a contract bug.
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetNode {
    Text(TextNode),
    Button(ButtonNode),
    TextInput(TextInputNode),
    Terminal(TerminalNode),
    Row(RowNode),
    Column(ColumnNode),
    Container(ContainerNode),
    Padding(PaddingNode),
    Space(SpaceNode),
    Icon(IconNode),
    Image(ImageNode),
    Scrollable(ScrollableNode),
    MouseArea(MouseAreaNode),
    Tablist(TablistNode),
    ResizableSplit(ResizableSplitNode),
    Semantic(SemanticNode),
    Layout(LayoutNode),
}

impl WidgetNode {
    /// Stable kind discriminator — mirrors the `kind` field plugins
    /// write in Lua. Useful for telemetry, snapshot tests, and the
    /// `widget.decoded` diagnostic context.
    pub fn kind(&self) -> &str {
        match self {
            Self::Text(_) => "text",
            Self::Button(_) => "button",
            Self::TextInput(_) => "text_input",
            Self::Terminal(_) => "terminal",
            Self::Row(_) => "row",
            Self::Column(_) => "column",
            Self::Container(_) => "container",
            Self::Padding(_) => "padding",
            Self::Space(_) => "space",
            Self::Icon(_) => "icon",
            Self::Image(_) => "image",
            Self::Scrollable(_) => "scrollable",
            Self::MouseArea(_) => "mouse_area",
            Self::Tablist(_) => "tablist",
            Self::ResizableSplit(_) => "resizable_split",
            Self::Semantic(n) => n.kind.as_str(),
            Self::Layout(n) => n.kind.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextNode {
    pub value: String,
    pub size: f32,
    pub color: Option<AstColor>,
    pub font: AstFont,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonNode {
    pub child: Option<Box<WidgetAst>>,
    pub text: Option<String>,
    pub on_click: Option<String>,
    pub value: Value,
    pub width: AstLength,
    pub height: AstLength,
    pub style: AstButtonStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextInputNode {
    pub placeholder: String,
    pub value: String,
    pub on_input: String,
    pub on_submit: Option<String>,
    pub width: AstLength,
    pub height: AstLength,
    pub autofocus: bool,
    pub style: AstTextInputStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TerminalNode {
    pub session: u64,
    pub width: AstLength,
    pub height: AstLength,
    pub font_size: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RowNode {
    pub children: Vec<WidgetAst>,
    pub spacing: f32,
    pub width: AstLength,
    pub height: AstLength,
    pub align_y: Option<AstAlignY>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnNode {
    pub children: Vec<WidgetAst>,
    pub spacing: f32,
    pub width: AstLength,
    pub height: AstLength,
    pub align_x: Option<AstAlignX>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContainerNode {
    pub child: Option<Box<WidgetAst>>,
    pub bg: Option<AstColor>,
    pub border: Option<AstBorder>,
    pub shadow: Option<AstShadow>,
    pub width: AstLength,
    pub height: AstLength,
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub center_x: bool,
    pub center_y: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaddingNode {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
    pub width: AstLength,
    pub height: AstLength,
    pub child: Option<Box<WidgetAst>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpaceNode {
    pub width: AstLength,
    pub height: AstLength,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IconNode {
    pub path: String,
    pub asset: Option<AstAssetRef>,
    pub size: f32,
    pub color: Option<AstColor>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageNode {
    pub path: String,
    pub asset: Option<AstAssetRef>,
    pub size: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScrollableNode {
    pub child: Option<Box<WidgetAst>>,
    pub width: AstLength,
    pub height: AstLength,
    pub id: Option<String>,
    pub scroll_y: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MouseAreaNode {
    pub child: Option<Box<WidgetAst>>,
    pub on_click: Option<String>,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TablistNode {
    pub tabs: Vec<AstTabSpec>,
    pub active: Value,
    pub orderable: bool,
    pub on_select: Option<String>,
    pub on_close: Option<String>,
    pub on_reorder: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResizableSplitNode {
    pub split_id: Option<String>,
    pub direction: AstSplitDirection,
    pub children: Vec<WidgetAst>,
}

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

struct DecodeCtx {
    limits: WidgetLimits,
    node_count: usize,
}

impl DecodeCtx {
    fn new(limits: WidgetLimits) -> Self {
        Self {
            limits,
            node_count: 0,
        }
    }
    fn count_node(&mut self, path: &str) -> Result<(), WidgetDecodeError> {
        self.node_count += 1;
        if self.node_count > self.limits.max_node_count {
            return Err(WidgetDecodeError::new(
                codes::NODE_COUNT_EXCEEDED,
                path,
                format!(
                    "tree exceeds max node count ({})",
                    self.limits.max_node_count
                ),
            ));
        }
        Ok(())
    }
}

/// Decode `value` into a `WidgetAst` using the default limits. Errors
/// pin-point the bad field via `path`. The path notation is dot-separated
/// for object keys with `[i]` for array indices, rooted at `root`.
pub fn decode(value: &Value) -> Result<WidgetAst, WidgetDecodeError> {
    decode_with_limits(value, WidgetLimits::DEFAULT)
}

pub fn decode_with_limits(
    value: &Value,
    limits: WidgetLimits,
) -> Result<WidgetAst, WidgetDecodeError> {
    let mut ctx = DecodeCtx::new(limits);
    decode_node(value, "root", 0, &mut ctx)
}

fn decode_node(
    value: &Value,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<WidgetAst, WidgetDecodeError> {
    if depth > ctx.limits.max_tree_depth {
        return Err(WidgetDecodeError::new(
            codes::DEPTH_EXCEEDED,
            path,
            format!("tree depth exceeds max ({})", ctx.limits.max_tree_depth),
        ));
    }
    ctx.count_node(path)?;
    let obj = value.as_object().ok_or_else(|| {
        WidgetDecodeError::new(
            codes::NOT_A_TABLE,
            path,
            format!("expected widget table, got {}", json_kind(value)),
        )
    })?;

    let kind = obj
        .get("kind")
        .ok_or_else(|| WidgetDecodeError::new(codes::FIELD_MISSING, path, "missing field 'kind'"))?
        .as_str()
        .ok_or_else(|| {
            WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{path}.kind"),
                "field 'kind' must be a string",
            )
        })?;

    // Stable node id: explicit `id = "..."` wins, else derive from path.
    // Tablist treats `id` as something else (it's per-tab) so we only
    // pull a top-level id for non-tablist nodes that opt in. We accept
    // an `id` field on every node uniformly — the field itself is
    // optional everywhere.
    let node_id = match obj.get("id") {
        Some(Value::String(s)) => NodeId::explicit(s.clone()),
        Some(other) if !matches!(other, Value::Null) && kind != "resizable_split" => {
            // `resizable_split` historically stores its split id as a
            // string only, so a non-string id there is *not* an error
            // here — handle it inside the variant decoder.
            return Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{path}.id"),
                format!("field 'id' must be a string, got {}", json_kind(other)),
            ));
        }
        _ => NodeId::from_path(path),
    };

    let meta = decode_meta(obj, path, ctx)?;
    let node = match kind {
        "text" => WidgetNode::Text(decode_text(obj, path, ctx)?),
        "button" => WidgetNode::Button(decode_button(obj, path, depth, ctx)?),
        "text_input" => WidgetNode::TextInput(decode_text_input(obj, path, ctx)?),
        "terminal" => WidgetNode::Terminal(decode_terminal(obj, path)?),
        "row" => WidgetNode::Row(decode_row(obj, path, depth, ctx)?),
        "column" => WidgetNode::Column(decode_column(obj, path, depth, ctx)?),
        "container" => WidgetNode::Container(decode_container(obj, path, depth, ctx)?),
        "padding" => WidgetNode::Padding(decode_padding(obj, path, depth, ctx)?),
        "space" => WidgetNode::Space(decode_space(obj, path)?),
        "icon" => WidgetNode::Icon(decode_icon(obj, path, ctx)?),
        "image" => WidgetNode::Image(decode_image(obj, path, ctx)?),
        "scrollable" => WidgetNode::Scrollable(decode_scrollable(obj, path, depth, ctx)?),
        "mouse_area" => WidgetNode::MouseArea(decode_mouse_area(obj, path, depth, ctx)?),
        "tablist" => WidgetNode::Tablist(decode_tablist(obj, path, ctx)?),
        "resizable_split" => {
            WidgetNode::ResizableSplit(decode_resizable_split(obj, path, depth, ctx)?)
        }
        other if widget_ast_semantic::is_semantic_kind(other) => {
            WidgetNode::Semantic(decode_semantic(other, obj, path, depth, ctx)?)
        }
        other if widget_ast_semantic::is_layout_kind(other) => {
            WidgetNode::Layout(decode_layout(other, obj, path, depth, ctx)?)
        }
        other => {
            return Err(WidgetDecodeError::new(
                codes::UNKNOWN_KIND,
                format!("{path}.kind"),
                format!("unknown widget kind '{other}'"),
            ));
        }
    };
    Ok(WidgetAst {
        node_id,
        meta,
        node,
    })
}

// ---- per-kind decoders --------------------------------------------------

type Obj = serde_json::Map<String, Value>;

fn decode_text(obj: &Obj, path: &str, ctx: &mut DecodeCtx) -> Result<TextNode, WidgetDecodeError> {
    let value = opt_string(obj, "value", path, ctx)?.unwrap_or_default();
    let size = opt_f32(obj, "size", path)?.unwrap_or(14.0);
    let color = opt_color(obj, "color", path, ctx)?;
    let font = opt_font(obj, "font", path)?.unwrap_or(AstFont::Default);
    Ok(TextNode {
        value,
        size,
        color,
        font,
    })
}

fn decode_button(
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<ButtonNode, WidgetDecodeError> {
    let child = opt_child(obj, "child", path, depth, ctx)?;
    let text = opt_string(obj, "text", path, ctx)?;
    let on_click = opt_string(obj, "on_click", path, ctx)?;
    let value = obj.get("value").cloned().unwrap_or(Value::Null);
    let width = opt_length(obj, "width", path)?;
    let height = opt_length(obj, "height", path)?;
    let style = decode_button_style(obj.get("style"), path, ctx)?;
    Ok(ButtonNode {
        child,
        text,
        on_click,
        value,
        width,
        height,
        style,
    })
}

fn decode_button_style(
    value: Option<&Value>,
    path: &str,
    ctx: &mut DecodeCtx,
) -> Result<AstButtonStyle, WidgetDecodeError> {
    let Some(v) = value else {
        return Ok(AstButtonStyle::default());
    };
    if matches!(v, Value::Null) {
        return Ok(AstButtonStyle::default());
    }
    let obj = v.as_object().ok_or_else(|| {
        WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{path}.style"),
            format!("field 'style' must be a table, got {}", json_kind(v)),
        )
    })?;
    let style_path = format!("{path}.style");
    Ok(AstButtonStyle {
        background: opt_color(obj, "background", &style_path, ctx)?,
        background_hover: opt_color(obj, "background_hover", &style_path, ctx)?,
        text_color: opt_color(obj, "text_color", &style_path, ctx)?,
        border: opt_border(obj, "border", &style_path, ctx)?,
    })
}

fn decode_text_input(
    obj: &Obj,
    path: &str,
    ctx: &mut DecodeCtx,
) -> Result<TextInputNode, WidgetDecodeError> {
    Ok(TextInputNode {
        placeholder: req_string(obj, "placeholder", path, ctx)?,
        value: req_string(obj, "value", path, ctx)?,
        on_input: req_string(obj, "on_input", path, ctx)?,
        on_submit: opt_string(obj, "on_submit", path, ctx)?,
        width: opt_length(obj, "width", path)?,
        height: opt_length(obj, "height", path)?,
        autofocus: opt_bool(obj, "autofocus", path)?.unwrap_or(false),
        style: decode_text_input_style(obj.get("style"), path, ctx)?,
    })
}

fn decode_terminal(obj: &Obj, path: &str) -> Result<TerminalNode, WidgetDecodeError> {
    let session = opt_usize(obj, "session", path)?.ok_or_else(|| {
        WidgetDecodeError::new(codes::FIELD_MISSING, path, "missing field 'session'")
    })? as u64;
    if session == 0 {
        return Err(WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{path}.session"),
            "field 'session' must be a positive integer",
        ));
    }
    Ok(TerminalNode {
        session,
        width: opt_length(obj, "width", path)?,
        height: opt_length(obj, "height", path)?,
        font_size: opt_f32(obj, "font_size", path)?.unwrap_or(13.0),
    })
}

fn decode_text_input_style(
    value: Option<&Value>,
    path: &str,
    ctx: &mut DecodeCtx,
) -> Result<AstTextInputStyle, WidgetDecodeError> {
    let Some(v) = value else {
        return Ok(AstTextInputStyle::default());
    };
    if matches!(v, Value::Null) {
        return Ok(AstTextInputStyle::default());
    }
    let obj = v.as_object().ok_or_else(|| {
        WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{path}.style"),
            format!("field 'style' must be a table, got {}", json_kind(v)),
        )
    })?;
    let style_path = format!("{path}.style");
    Ok(AstTextInputStyle {
        background: opt_color(obj, "background", &style_path, ctx)?,
        text_color: opt_color(obj, "text_color", &style_path, ctx)?,
        placeholder_color: opt_color(obj, "placeholder_color", &style_path, ctx)?,
        border: opt_border(obj, "border", &style_path, ctx)?,
    })
}

fn decode_row(
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<RowNode, WidgetDecodeError> {
    let children = decode_children(obj, path, depth, ctx)?;
    let spacing = opt_spacing_value(obj, "spacing", path, ctx)?.unwrap_or(0.0);
    let width = opt_length(obj, "width", path)?;
    let height = opt_length(obj, "height", path)?;
    let align_y = opt_align_y(obj, "align_y", path)?;
    Ok(RowNode {
        children,
        spacing,
        width,
        height,
        align_y,
    })
}

fn decode_column(
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<ColumnNode, WidgetDecodeError> {
    let children = decode_children(obj, path, depth, ctx)?;
    let spacing = opt_spacing_value(obj, "spacing", path, ctx)?.unwrap_or(0.0);
    let width = opt_length(obj, "width", path)?;
    let height = opt_length(obj, "height", path)?;
    let align_x = opt_align_x(obj, "align_x", path)?;
    Ok(ColumnNode {
        children,
        spacing,
        width,
        height,
        align_x,
    })
}

fn decode_container(
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<ContainerNode, WidgetDecodeError> {
    let child = opt_child(obj, "child", path, depth, ctx)?;
    Ok(ContainerNode {
        child,
        bg: opt_color(obj, "bg", path, ctx)?,
        border: opt_border(obj, "border", path, ctx)?,
        shadow: opt_shadow(obj, "shadow", path, ctx)?,
        width: opt_length(obj, "width", path)?,
        height: opt_length(obj, "height", path)?,
        max_width: opt_f32(obj, "max_width", path)?,
        max_height: opt_f32(obj, "max_height", path)?,
        min_width: opt_f32(obj, "min_width", path)?,
        min_height: opt_f32(obj, "min_height", path)?,
        center_x: opt_bool(obj, "center_x", path)?.unwrap_or(false),
        center_y: opt_bool(obj, "center_y", path)?.unwrap_or(false),
    })
}

fn decode_padding(
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<PaddingNode, WidgetDecodeError> {
    Ok(PaddingNode {
        top: opt_spacing_value(obj, "top", path, ctx)?.unwrap_or(0.0),
        right: opt_spacing_value(obj, "right", path, ctx)?.unwrap_or(0.0),
        bottom: opt_spacing_value(obj, "bottom", path, ctx)?.unwrap_or(0.0),
        left: opt_spacing_value(obj, "left", path, ctx)?.unwrap_or(0.0),
        width: opt_length(obj, "width", path)?,
        height: opt_length(obj, "height", path)?,
        child: opt_child(obj, "child", path, depth, ctx)?,
    })
}

fn decode_space(obj: &Obj, path: &str) -> Result<SpaceNode, WidgetDecodeError> {
    Ok(SpaceNode {
        width: opt_length(obj, "width", path)?,
        height: opt_length(obj, "height", path)?,
    })
}

fn decode_icon(obj: &Obj, path: &str, ctx: &mut DecodeCtx) -> Result<IconNode, WidgetDecodeError> {
    let asset = opt_asset(obj, "asset", path, ctx)?;
    let raw_path = match &asset {
        Some(asset) => asset.path.clone(),
        None => opt_string(obj, "path", path, ctx)?.unwrap_or_default(),
    };
    check_asset_path_len(&raw_path, &format!("{path}.path"), ctx)?;
    Ok(IconNode {
        path: raw_path,
        asset,
        size: opt_f32(obj, "size", path)?.unwrap_or(16.0),
        color: opt_color(obj, "color", path, ctx)?,
    })
}

fn decode_image(
    obj: &Obj,
    path: &str,
    ctx: &mut DecodeCtx,
) -> Result<ImageNode, WidgetDecodeError> {
    let asset = opt_asset(obj, "asset", path, ctx)?;
    let raw_path = match &asset {
        Some(asset) => asset.path.clone(),
        None => opt_string(obj, "path", path, ctx)?.unwrap_or_default(),
    };
    check_asset_path_len(&raw_path, &format!("{path}.path"), ctx)?;
    Ok(ImageNode {
        path: raw_path,
        asset,
        size: opt_f32(obj, "size", path)?.unwrap_or(16.0),
    })
}

fn decode_scrollable(
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<ScrollableNode, WidgetDecodeError> {
    Ok(ScrollableNode {
        child: opt_child(obj, "child", path, depth, ctx)?,
        width: opt_length(obj, "width", path)?,
        height: opt_length(obj, "height", path)?,
        id: opt_string(obj, "id", path, ctx)?,
        scroll_y: opt_f32(obj, "scroll_y", path)?.map(|value| value.max(0.0)),
    })
}

fn decode_mouse_area(
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<MouseAreaNode, WidgetDecodeError> {
    Ok(MouseAreaNode {
        child: opt_child(obj, "child", path, depth, ctx)?,
        on_click: opt_string(obj, "on_click", path, ctx)?,
        value: obj.get("value").cloned().unwrap_or(Value::Null),
    })
}

fn decode_tablist(
    obj: &Obj,
    path: &str,
    ctx: &mut DecodeCtx,
) -> Result<TablistNode, WidgetDecodeError> {
    let tabs_value = obj.get("tabs");
    let tabs = match tabs_value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Object(map)) if map.is_empty() => Vec::new(),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for (i, t) in arr.iter().enumerate() {
                let tpath = format!("{path}.tabs[{i}]");
                let tbl = t.as_object().ok_or_else(|| {
                    WidgetDecodeError::new(
                        codes::FIELD_TYPE_MISMATCH,
                        &tpath,
                        format!("expected tab table, got {}", json_kind(t)),
                    )
                })?;
                let id = tbl.get("id").cloned().unwrap_or(Value::Null);
                let name = opt_string(tbl, "name", &tpath, ctx)?.unwrap_or_default();
                out.push(AstTabSpec { id, name });
            }
            out
        }
        Some(other) => {
            return Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{path}.tabs"),
                format!("field 'tabs' must be an array, got {}", json_kind(other)),
            ));
        }
    };
    let active = obj.get("active").cloned().unwrap_or(Value::Null);
    let orderable = opt_bool(obj, "orderable", path)?.unwrap_or(false);
    let on_select = opt_string(obj, "on_select", path, ctx)?;
    let on_close = opt_string(obj, "on_close", path, ctx)?;
    let on_reorder = opt_string(obj, "on_reorder", path, ctx)?;
    Ok(TablistNode {
        tabs,
        active,
        orderable,
        on_select,
        on_close,
        on_reorder,
    })
}

fn decode_resizable_split(
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<ResizableSplitNode, WidgetDecodeError> {
    let split_id = match obj.get("id") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            check_string_len(s, &format!("{path}.id"), ctx)?;
            Some(s.clone())
        }
        Some(other) => {
            return Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{path}.id"),
                format!("field 'id' must be a string, got {}", json_kind(other)),
            ));
        }
    };
    let direction = match obj.get("direction") {
        None | Some(Value::Null) => AstSplitDirection::Horizontal,
        Some(Value::String(s)) => match s.as_str() {
            "vertical" => AstSplitDirection::Vertical,
            "horizontal" => AstSplitDirection::Horizontal,
            other => {
                return Err(WidgetDecodeError::new(
                    codes::FIELD_TYPE_MISMATCH,
                    format!("{path}.direction"),
                    format!("field 'direction' must be 'horizontal' or 'vertical', got '{other}'"),
                ));
            }
        },
        Some(other) => {
            return Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{path}.direction"),
                format!(
                    "field 'direction' must be a string, got {}",
                    json_kind(other)
                ),
            ));
        }
    };
    let children = decode_children(obj, path, depth, ctx)?;
    Ok(ResizableSplitNode {
        split_id,
        direction,
        children,
    })
}

fn decode_semantic(
    kind: &str,
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<SemanticNode, WidgetDecodeError> {
    Ok(SemanticNode {
        kind: kind.to_string(),
        title: opt_string(obj, "title", path, ctx)?,
        text: opt_string(obj, "text", path, ctx)?.or(opt_string(obj, "value_text", path, ctx)?),
        label: opt_string(obj, "label", path, ctx)?,
        command: opt_string(obj, "command", path, ctx)?,
        on_click: opt_string(obj, "on_click", path, ctx)?,
        on_change: opt_string(obj, "on_change", path, ctx)?,
        value: obj.get("value").cloned().unwrap_or(Value::Null),
        disabled: opt_bool(obj, "disabled", path)?.unwrap_or(false),
        checked: opt_bool(obj, "checked", path)?.unwrap_or(false),
        selected: obj.get("selected").cloned().unwrap_or(Value::Null),
        progress: opt_f32(obj, "progress", path)?.or(opt_f32(obj, "value_number", path)?),
        language: opt_string(obj, "language", path, ctx)?,
        color: opt_color(obj, "color", path, ctx)?,
        spacing: opt_spacing_value(obj, "spacing", path, ctx)?,
        child: opt_child(obj, "child", path, depth, ctx)?,
        children: decode_children(obj, path, depth, ctx)?,
        items: decode_semantic_items(obj, "items", path, depth, ctx)?,
        columns: decode_semantic_columns(obj, path)?,
        rows: decode_semantic_rows(obj, path)?,
        options: decode_semantic_options(obj, path, ctx)?,
    })
}

fn decode_layout(
    kind: &str,
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<LayoutNode, WidgetDecodeError> {
    Ok(LayoutNode {
        kind: kind.to_string(),
        direction: opt_string(obj, "direction", path, ctx)?,
        columns: opt_usize(obj, "columns", path)?.unwrap_or(2).max(1),
        spacing: opt_spacing_value(obj, "spacing", path, ctx)?,
        active: obj.get("active").cloned().unwrap_or(Value::Null),
        child: opt_child(obj, "child", path, depth, ctx)?,
        children: decode_children(obj, path, depth, ctx)?,
        tabs: decode_layout_tabs(obj, path, depth, ctx)?,
    })
}

fn decode_meta(
    obj: &Obj,
    path: &str,
    ctx: &mut DecodeCtx,
) -> Result<WidgetMeta, WidgetDecodeError> {
    Ok(WidgetMeta {
        label: opt_string(obj, "label", path, ctx)?,
        role: opt_string(obj, "role", path, ctx)?,
        shortcut: opt_string(obj, "shortcut", path, ctx)?,
        disabled_reason: opt_string(obj, "disabled_reason", path, ctx)?,
        focus_order: opt_i32(obj, "focus_order", path)?,
    })
}

fn decode_semantic_items(
    obj: &Obj,
    key: &str,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<Vec<SemanticItem>, WidgetDecodeError> {
    // Nested `children` recurse through here; without a depth guard and node
    // count a deeply nested `items` chain overflows the stack (the enclosing
    // decode_node limits don't apply to this side channel).
    if depth > ctx.limits.max_tree_depth {
        return Err(WidgetDecodeError::new(
            codes::DEPTH_EXCEEDED,
            format!("{path}.{key}"),
            format!("tree depth exceeds max ({})", ctx.limits.max_tree_depth),
        ));
    }
    let arr = match obj.get(key) {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Object(map)) if map.is_empty() => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(other) => {
            return Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{path}.{key}"),
                format!("field '{key}' must be an array, got {}", json_kind(other)),
            ));
        }
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let item_path = format!("{path}.{key}[{i}]");
        ctx.count_node(&item_path)?;
        let tbl = item.as_object().ok_or_else(|| {
            WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                &item_path,
                format!("expected item table, got {}", json_kind(item)),
            )
        })?;
        out.push(SemanticItem {
            id: tbl.get("id").cloned().unwrap_or(Value::Null),
            label: opt_string(tbl, "label", &item_path, ctx)?
                .or(opt_string(tbl, "title", &item_path, ctx)?)
                .unwrap_or_default(),
            text: opt_string(tbl, "text", &item_path, ctx)?,
            value: tbl.get("value").cloned().unwrap_or(Value::Null),
            child: opt_child(tbl, "child", &item_path, depth, ctx)?,
            children: decode_semantic_items(tbl, "children", &item_path, depth + 1, ctx)?,
        });
    }
    Ok(out)
}

fn decode_semantic_columns(
    obj: &Obj,
    path: &str,
) -> Result<Vec<SemanticColumn>, WidgetDecodeError> {
    let arr = match obj.get("columns") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Object(map)) if map.is_empty() => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(other) => {
            return Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{path}.columns"),
                format!("field 'columns' must be an array, got {}", json_kind(other)),
            ));
        }
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, column) in arr.iter().enumerate() {
        let cpath = format!("{path}.columns[{i}]");
        let tbl = column.as_object().ok_or_else(|| {
            WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                &cpath,
                format!("expected column table, got {}", json_kind(column)),
            )
        })?;
        out.push(SemanticColumn {
            id: tbl
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            title: tbl
                .get("title")
                .and_then(Value::as_str)
                .or_else(|| tbl.get("label").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string(),
            width: opt_length(tbl, "width", &cpath)?,
        });
    }
    Ok(out)
}

fn decode_semantic_rows(obj: &Obj, path: &str) -> Result<Vec<SemanticRow>, WidgetDecodeError> {
    let arr = match obj.get("rows") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Object(map)) if map.is_empty() => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(other) => {
            return Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{path}.rows"),
                format!("field 'rows' must be an array, got {}", json_kind(other)),
            ));
        }
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, row) in arr.iter().enumerate() {
        let rpath = format!("{path}.rows[{i}]");
        let tbl = row.as_object().ok_or_else(|| {
            WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                &rpath,
                format!("expected row table, got {}", json_kind(row)),
            )
        })?;
        let cells = match tbl.get("cells") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(cells)) => cells.clone(),
            Some(other) => {
                return Err(WidgetDecodeError::new(
                    codes::FIELD_TYPE_MISMATCH,
                    format!("{rpath}.cells"),
                    format!("field 'cells' must be an array, got {}", json_kind(other)),
                ));
            }
        };
        out.push(SemanticRow {
            id: tbl.get("id").cloned().unwrap_or(Value::Null),
            cells,
        });
    }
    Ok(out)
}

fn decode_semantic_options(
    obj: &Obj,
    path: &str,
    ctx: &mut DecodeCtx,
) -> Result<Vec<SemanticOption>, WidgetDecodeError> {
    let arr = match obj.get("options") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Object(map)) if map.is_empty() => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(other) => {
            return Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{path}.options"),
                format!("field 'options' must be an array, got {}", json_kind(other)),
            ));
        }
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, option) in arr.iter().enumerate() {
        let opath = format!("{path}.options[{i}]");
        let tbl = option.as_object().ok_or_else(|| {
            WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                &opath,
                format!("expected option table, got {}", json_kind(option)),
            )
        })?;
        out.push(SemanticOption {
            value: tbl.get("value").cloned().unwrap_or(Value::Null),
            label: opt_string(tbl, "label", &opath, ctx)?.unwrap_or_default(),
        });
    }
    Ok(out)
}

fn decode_layout_tabs(
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<Vec<LayoutTab>, WidgetDecodeError> {
    let arr = match obj.get("tabs") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Object(map)) if map.is_empty() => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(other) => {
            return Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{path}.tabs"),
                format!("field 'tabs' must be an array, got {}", json_kind(other)),
            ));
        }
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, tab) in arr.iter().enumerate() {
        let tpath = format!("{path}.tabs[{i}]");
        let tbl = tab.as_object().ok_or_else(|| {
            WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                &tpath,
                format!("expected tab table, got {}", json_kind(tab)),
            )
        })?;
        out.push(LayoutTab {
            id: tbl.get("id").cloned().unwrap_or(Value::Null),
            title: opt_string(tbl, "title", &tpath, ctx)?
                .or(opt_string(tbl, "label", &tpath, ctx)?)
                .unwrap_or_default(),
            child: opt_child(tbl, "child", &tpath, depth, ctx)?,
        });
    }
    Ok(out)
}

fn decode_children(
    obj: &Obj,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<Vec<WidgetAst>, WidgetDecodeError> {
    let arr = match obj.get("children") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Object(map)) if map.is_empty() => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(other) => {
            return Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{path}.children"),
                format!(
                    "field 'children' must be an array, got {}",
                    json_kind(other)
                ),
            ));
        }
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, child) in arr.iter().enumerate() {
        let child_path = format!("{path}.children[{i}]");
        out.push(decode_node(child, &child_path, depth + 1, ctx)?);
    }
    Ok(out)
}

fn opt_child(
    obj: &Obj,
    key: &str,
    path: &str,
    depth: usize,
    ctx: &mut DecodeCtx,
) -> Result<Option<Box<WidgetAst>>, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let child_path = format!("{path}.{key}");
            let ast = decode_node(v, &child_path, depth + 1, ctx)?;
            Ok(Some(Box::new(ast)))
        }
    }
}

// ---- field helpers ------------------------------------------------------

fn json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn check_string_len(s: &str, path: &str, ctx: &mut DecodeCtx) -> Result<(), WidgetDecodeError> {
    if s.len() > ctx.limits.max_string_length {
        return Err(WidgetDecodeError::new(
            codes::STRING_TOO_LONG,
            path,
            format!(
                "string of length {} exceeds max ({})",
                s.len(),
                ctx.limits.max_string_length
            ),
        ));
    }
    Ok(())
}

fn opt_string(
    obj: &Obj,
    key: &str,
    parent: &str,
    ctx: &mut DecodeCtx,
) -> Result<Option<String>, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            check_string_len(s, &format!("{parent}.{key}"), ctx)?;
            Ok(Some(s.clone()))
        }
        Some(other) => Err(WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{parent}.{key}"),
            format!("field '{key}' must be a string, got {}", json_kind(other)),
        )),
    }
}

fn req_string(
    obj: &Obj,
    key: &str,
    parent: &str,
    ctx: &mut DecodeCtx,
) -> Result<String, WidgetDecodeError> {
    opt_string(obj, key, parent, ctx)?.ok_or_else(|| {
        WidgetDecodeError::new(
            codes::FIELD_MISSING,
            parent,
            format!("missing field '{key}'"),
        )
    })
}

fn opt_f32(obj: &Obj, key: &str, parent: &str) -> Result<Option<f32>, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64() {
            Some(n) => Ok(Some(n as f32)),
            None => Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{parent}.{key}"),
                format!("field '{key}' must be a number, got {}", json_kind(v)),
            )),
        },
    }
}

fn opt_i32(obj: &Obj, key: &str, parent: &str) -> Result<Option<i32>, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_i64() {
            Some(n) => Ok(Some(n as i32)),
            None => Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{parent}.{key}"),
                format!("field '{key}' must be an integer, got {}", json_kind(v)),
            )),
        },
    }
}

fn opt_usize(obj: &Obj, key: &str, parent: &str) -> Result<Option<usize>, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_u64() {
            Some(n) => Ok(Some(n as usize)),
            None => Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{parent}.{key}"),
                format!(
                    "field '{key}' must be a positive integer, got {}",
                    json_kind(v)
                ),
            )),
        },
    }
}

fn opt_bool(obj: &Obj, key: &str, parent: &str) -> Result<Option<bool>, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{parent}.{key}"),
            format!("field '{key}' must be a boolean, got {}", json_kind(other)),
        )),
    }
}

fn opt_spacing_value(
    obj: &Obj,
    key: &str,
    parent: &str,
    ctx: &mut DecodeCtx,
) -> Result<Option<f32>, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            if let Some(n) = v.as_f64() {
                return Ok(Some(n as f32));
            }
            if let Some(token) = token_from_value(v, &format!("{parent}.{key}"), ctx)? {
                return Ok(Some(resolve_spacing_token(&token)));
            }
            Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{parent}.{key}"),
                format!(
                    "field '{key}' must be a number or {{ token = string }}, got {}",
                    json_kind(v)
                ),
            ))
        }
    }
}

fn opt_length(obj: &Obj, key: &str, parent: &str) -> Result<AstLength, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(AstLength::Auto),
        Some(v) => {
            if let Some(n) = v.as_f64() {
                return Ok(AstLength::Fixed(n as f32));
            }
            if let Some(s) = v.as_str() {
                return match s {
                    "fill" => Ok(AstLength::Fill),
                    "shrink" => Ok(AstLength::Shrink),
                    other => Err(WidgetDecodeError::new(
                        codes::FIELD_TYPE_MISMATCH,
                        format!("{parent}.{key}"),
                        format!("field '{key}' must be a number or 'fill'/'shrink', got '{other}'"),
                    )),
                };
            }
            Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{parent}.{key}"),
                format!(
                    "field '{key}' must be a number or string, got {}",
                    json_kind(v)
                ),
            ))
        }
    }
}

fn opt_color(
    obj: &Obj,
    key: &str,
    parent: &str,
    ctx: &mut DecodeCtx,
) -> Result<Option<AstColor>, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            check_string_len(s, &format!("{parent}.{key}"), ctx)?;
            Ok(Some(AstColor {
                raw: s.clone(),
                token: None,
            }))
        }
        Some(v) => {
            if let Some(token) = token_from_value(v, &format!("{parent}.{key}"), ctx)? {
                return Ok(Some(AstColor {
                    raw: String::new(),
                    token: Some(token),
                }));
            }
            Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{parent}.{key}"),
                format!(
                    "field '{key}' must be a string or {{ token = string }}, got {}",
                    json_kind(v)
                ),
            ))
        }
    }
}

fn token_from_value(
    value: &Value,
    path: &str,
    ctx: &mut DecodeCtx,
) -> Result<Option<String>, WidgetDecodeError> {
    let Some(obj) = value.as_object() else {
        return Ok(None);
    };
    match obj.get("token") {
        Some(Value::String(s)) => {
            check_string_len(s, &format!("{path}.token"), ctx)?;
            Ok(Some(s.clone()))
        }
        Some(other) => Err(WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{path}.token"),
            format!("field 'token' must be a string, got {}", json_kind(other)),
        )),
        None => Ok(None),
    }
}

fn resolve_spacing_token(token: &str) -> f32 {
    match token {
        "space.0" => 0.0,
        "space.1" => 4.0,
        "space.2" => 8.0,
        "space.3" => 12.0,
        "space.4" => 16.0,
        "space.5" => 24.0,
        "space.6" => 32.0,
        _ => 0.0,
    }
}

fn opt_align_x(obj: &Obj, key: &str, parent: &str) -> Result<Option<AstAlignX>, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => match s.as_str() {
            "start" | "left" => Ok(Some(AstAlignX::Start)),
            "center" | "centre" => Ok(Some(AstAlignX::Center)),
            "end" | "right" => Ok(Some(AstAlignX::End)),
            other => Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{parent}.{key}"),
                format!("unknown horizontal alignment '{other}'"),
            )),
        },
        Some(other) => Err(WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{parent}.{key}"),
            format!("field '{key}' must be a string, got {}", json_kind(other)),
        )),
    }
}

fn opt_align_y(obj: &Obj, key: &str, parent: &str) -> Result<Option<AstAlignY>, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => match s.as_str() {
            "start" | "top" => Ok(Some(AstAlignY::Start)),
            "center" | "centre" => Ok(Some(AstAlignY::Center)),
            "end" | "bottom" => Ok(Some(AstAlignY::End)),
            other => Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{parent}.{key}"),
                format!("unknown vertical alignment '{other}'"),
            )),
        },
        Some(other) => Err(WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{parent}.{key}"),
            format!("field '{key}' must be a string, got {}", json_kind(other)),
        )),
    }
}

fn opt_font(obj: &Obj, key: &str, parent: &str) -> Result<Option<AstFont>, WidgetDecodeError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => match s.as_str() {
            "default" | "system" => Ok(Some(AstFont::Default)),
            "mono" | "monospace" => Ok(Some(AstFont::Mono)),
            other => Err(WidgetDecodeError::new(
                codes::FIELD_TYPE_MISMATCH,
                format!("{parent}.{key}"),
                format!("unknown font '{other}'"),
            )),
        },
        Some(other) => Err(WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{parent}.{key}"),
            format!("field '{key}' must be a string, got {}", json_kind(other)),
        )),
    }
}

fn opt_border(
    obj: &Obj,
    key: &str,
    parent: &str,
    ctx: &mut DecodeCtx,
) -> Result<Option<AstBorder>, WidgetDecodeError> {
    let Some(v) = obj.get(key) else {
        return Ok(None);
    };
    if matches!(v, Value::Null) {
        return Ok(None);
    }
    let inner = v.as_object().ok_or_else(|| {
        WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{parent}.{key}"),
            format!("field '{key}' must be a table, got {}", json_kind(v)),
        )
    })?;
    let bpath = format!("{parent}.{key}");
    Ok(Some(AstBorder {
        width: opt_f32(inner, "width", &bpath)?.unwrap_or(0.0),
        radius: opt_f32(inner, "radius", &bpath)?.unwrap_or(0.0),
        color: opt_color(inner, "color", &bpath, ctx)?,
    }))
}

fn opt_shadow(
    obj: &Obj,
    key: &str,
    parent: &str,
    ctx: &mut DecodeCtx,
) -> Result<Option<AstShadow>, WidgetDecodeError> {
    let Some(v) = obj.get(key) else {
        return Ok(None);
    };
    if matches!(v, Value::Null) {
        return Ok(None);
    }
    let inner = v.as_object().ok_or_else(|| {
        WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{parent}.{key}"),
            format!("field '{key}' must be a table, got {}", json_kind(v)),
        )
    })?;
    let spath = format!("{parent}.{key}");
    Ok(Some(AstShadow {
        color: opt_color(inner, "color", &spath, ctx)?,
        offset_x: opt_f32(inner, "offset_x", &spath)?.unwrap_or(0.0),
        offset_y: opt_f32(inner, "offset_y", &spath)?.unwrap_or(0.0),
        blur_radius: opt_f32(inner, "blur_radius", &spath)?.unwrap_or(0.0),
    }))
}

fn opt_asset(
    obj: &Obj,
    key: &str,
    parent: &str,
    ctx: &mut DecodeCtx,
) -> Result<Option<AstAssetRef>, WidgetDecodeError> {
    let Some(v) = obj.get(key) else {
        return Ok(None);
    };
    if matches!(v, Value::Null) {
        return Ok(None);
    }
    let asset = v.as_object().ok_or_else(|| {
        WidgetDecodeError::new(
            codes::FIELD_TYPE_MISMATCH,
            format!("{parent}.{key}"),
            format!(
                "field '{key}' must be an asset handle table, got {}",
                json_kind(v)
            ),
        )
    })?;
    let apath = format!("{parent}.{key}");
    let path = opt_string(asset, "path", &apath, ctx)?.ok_or_else(|| {
        WidgetDecodeError::new(
            codes::FIELD_MISSING,
            &apath,
            "asset handle missing field 'path'",
        )
    })?;
    check_asset_path_len(&path, &format!("{apath}.path"), ctx)?;
    Ok(Some(AstAssetRef {
        path,
        kind: opt_string(asset, "kind", &apath, ctx)?,
        handle: opt_string(asset, "handle", &apath, ctx)?.or(opt_string(
            asset,
            "__leviathan_asset_handle",
            &apath,
            ctx,
        )?),
    }))
}

fn check_asset_path_len(s: &str, path: &str, ctx: &DecodeCtx) -> Result<(), WidgetDecodeError> {
    if s.len() > ctx.limits.max_asset_path_length {
        return Err(WidgetDecodeError::new(
            codes::ASSET_LIMIT_EXCEEDED,
            path,
            format!(
                "asset path length {} exceeds max ({})",
                s.len(),
                ctx.limits.max_asset_path_length
            ),
        ));
    }
    Ok(())
}

pub fn raw_color_paths(ast: &WidgetAst) -> Vec<String> {
    let mut out = Vec::new();
    collect_raw_color_paths(ast, "root", &mut out);
    out
}

fn collect_raw_color_paths(ast: &WidgetAst, path: &str, out: &mut Vec<String>) {
    match &ast.node {
        WidgetNode::Text(node) => push_raw_color(out, path, "color", &node.color),
        WidgetNode::Button(node) => {
            push_raw_color(out, path, "style.background", &node.style.background);
            push_raw_color(
                out,
                path,
                "style.background_hover",
                &node.style.background_hover,
            );
            push_raw_color(out, path, "style.text_color", &node.style.text_color);
            if let Some(border) = &node.style.border {
                push_raw_color(out, path, "style.border.color", &border.color);
            }
            if let Some(child) = &node.child {
                collect_raw_color_paths(child, &format!("{path}.child"), out);
            }
        }
        WidgetNode::TextInput(node) => {
            push_raw_color(out, path, "style.background", &node.style.background);
            push_raw_color(out, path, "style.text_color", &node.style.text_color);
            push_raw_color(
                out,
                path,
                "style.placeholder_color",
                &node.style.placeholder_color,
            );
            if let Some(border) = &node.style.border {
                push_raw_color(out, path, "style.border.color", &border.color);
            }
        }
        WidgetNode::Terminal(_) => {}
        WidgetNode::Container(node) => {
            push_raw_color(out, path, "bg", &node.bg);
            if let Some(border) = &node.border {
                push_raw_color(out, path, "border.color", &border.color);
            }
            if let Some(shadow) = &node.shadow {
                push_raw_color(out, path, "shadow.color", &shadow.color);
            }
            if let Some(child) = &node.child {
                collect_raw_color_paths(child, &format!("{path}.child"), out);
            }
        }
        WidgetNode::Icon(node) => push_raw_color(out, path, "color", &node.color),
        WidgetNode::Row(node) => {
            for (idx, child) in node.children.iter().enumerate() {
                collect_raw_color_paths(child, &format!("{path}.children[{idx}]"), out);
            }
        }
        WidgetNode::Column(node) => {
            for (idx, child) in node.children.iter().enumerate() {
                collect_raw_color_paths(child, &format!("{path}.children[{idx}]"), out);
            }
        }
        WidgetNode::ResizableSplit(node) => {
            for (idx, child) in node.children.iter().enumerate() {
                collect_raw_color_paths(child, &format!("{path}.children[{idx}]"), out);
            }
        }
        WidgetNode::Padding(node) => {
            if let Some(child) = &node.child {
                collect_raw_color_paths(child, &format!("{path}.child"), out);
            }
        }
        WidgetNode::Scrollable(node) => {
            if let Some(child) = &node.child {
                collect_raw_color_paths(child, &format!("{path}.child"), out);
            }
        }
        WidgetNode::MouseArea(node) => {
            if let Some(child) = &node.child {
                collect_raw_color_paths(child, &format!("{path}.child"), out);
            }
        }
        WidgetNode::Semantic(node) => {
            if let Some(child) = &node.child {
                collect_raw_color_paths(child, &format!("{path}.child"), out);
            }
            for (idx, child) in node.children.iter().enumerate() {
                collect_raw_color_paths(child, &format!("{path}.children[{idx}]"), out);
            }
            for (idx, item) in node.items.iter().enumerate() {
                if let Some(child) = &item.child {
                    collect_raw_color_paths(child, &format!("{path}.items[{idx}].child"), out);
                }
            }
        }
        WidgetNode::Layout(node) => {
            if let Some(child) = &node.child {
                collect_raw_color_paths(child, &format!("{path}.child"), out);
            }
            for (idx, child) in node.children.iter().enumerate() {
                collect_raw_color_paths(child, &format!("{path}.children[{idx}]"), out);
            }
            for (idx, tab) in node.tabs.iter().enumerate() {
                if let Some(child) = &tab.child {
                    collect_raw_color_paths(child, &format!("{path}.tabs[{idx}].child"), out);
                }
            }
        }
        _ => {}
    }
}

fn push_raw_color(out: &mut Vec<String>, path: &str, field: &str, color: &Option<AstColor>) {
    if let Some(color) = color {
        if color.token.is_none() && !color.raw.is_empty() {
            out.push(format!("{path}.{field}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Snapshot (stable text dump)
// ---------------------------------------------------------------------------

/// Dump the AST to a stable, line-oriented text form. Used by the
/// snapshot tests + the fuzz harness so a regression in the decoder
/// shows up as a textual diff rather than a `Debug` blob full of struct
/// names + line breaks that vary across rustc versions.
#[cfg(test)]
pub fn snapshot(ast: &WidgetAst) -> String {
    let mut out = String::new();
    snapshot_node(ast, 0, &mut out);
    out
}

#[cfg(test)]
fn snapshot_node(ast: &WidgetAst, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    let id_marker = if ast.node_id.explicit {
        format!(" id=*{}", ast.node_id.value)
    } else {
        format!(" id={}", ast.node_id.value)
    };
    out.push_str(&pad);
    out.push_str(ast.node.kind());
    out.push_str(&id_marker);
    match &ast.node {
        WidgetNode::Text(t) => {
            out.push_str(&format!(
                " value={:?} size={} color={}\n",
                t.value,
                t.size,
                fmt_color(&t.color)
            ));
        }
        WidgetNode::Button(b) => {
            out.push_str(&format!(
                " text={:?} on_click={:?} width={} height={}\n",
                b.text,
                b.on_click,
                fmt_length(b.width),
                fmt_length(b.height)
            ));
            if let Some(c) = &b.child {
                snapshot_node(c, indent + 1, out);
            }
        }
        WidgetNode::TextInput(t) => {
            out.push_str(&format!(
                " placeholder={:?} value={:?} on_input={:?} on_submit={:?} width={} height={} autofocus={}\n",
                t.placeholder,
                t.value,
                t.on_input,
                t.on_submit,
                fmt_length(t.width),
                fmt_length(t.height),
                t.autofocus
            ));
        }
        WidgetNode::Terminal(t) => {
            out.push_str(&format!(
                " session={} width={} height={} font_size={}\n",
                t.session,
                fmt_length(t.width),
                fmt_length(t.height),
                t.font_size
            ));
        }
        WidgetNode::Row(r) => {
            out.push_str(&format!(
                " spacing={} width={} height={} align_y={:?}\n",
                r.spacing,
                fmt_length(r.width),
                fmt_length(r.height),
                r.align_y
            ));
            for c in &r.children {
                snapshot_node(c, indent + 1, out);
            }
        }
        WidgetNode::Column(c) => {
            out.push_str(&format!(
                " spacing={} width={} height={} align_x={:?}\n",
                c.spacing,
                fmt_length(c.width),
                fmt_length(c.height),
                c.align_x
            ));
            for ch in &c.children {
                snapshot_node(ch, indent + 1, out);
            }
        }
        WidgetNode::Container(c) => {
            out.push_str(&format!(
                " bg={} width={} height={} center=({},{})\n",
                fmt_color(&c.bg),
                fmt_length(c.width),
                fmt_length(c.height),
                c.center_x,
                c.center_y
            ));
            if let Some(child) = &c.child {
                snapshot_node(child, indent + 1, out);
            }
        }
        WidgetNode::Padding(p) => {
            out.push_str(&format!(
                " top={} right={} bottom={} left={} width={} height={}\n",
                p.top,
                p.right,
                p.bottom,
                p.left,
                fmt_length(p.width),
                fmt_length(p.height)
            ));
            if let Some(c) = &p.child {
                snapshot_node(c, indent + 1, out);
            }
        }
        WidgetNode::Space(s) => {
            out.push_str(&format!(
                " width={} height={}\n",
                fmt_length(s.width),
                fmt_length(s.height)
            ));
        }
        WidgetNode::Icon(i) => {
            out.push_str(&format!(
                " path={:?} size={} color={}\n",
                i.path,
                i.size,
                fmt_color(&i.color)
            ));
        }
        WidgetNode::Image(i) => {
            out.push_str(&format!(" path={:?} size={}\n", i.path, i.size));
        }
        WidgetNode::Scrollable(s) => {
            out.push_str(&format!(
                " width={} height={}",
                fmt_length(s.width),
                fmt_length(s.height)
            ));
            if let Some(id) = &s.id {
                out.push_str(&format!(" scroll_id={id:?}"));
            }
            if let Some(scroll_y) = s.scroll_y {
                out.push_str(&format!(" scroll_y={scroll_y}"));
            }
            out.push('\n');
            if let Some(c) = &s.child {
                snapshot_node(c, indent + 1, out);
            }
        }
        WidgetNode::MouseArea(m) => {
            out.push_str(&format!(" on_click={:?}\n", m.on_click));
            if let Some(c) = &m.child {
                snapshot_node(c, indent + 1, out);
            }
        }
        WidgetNode::Tablist(t) => {
            out.push_str(&format!(
                " active={} orderable={} on_select={:?} on_close={:?} on_reorder={:?}\n",
                t.active, t.orderable, t.on_select, t.on_close, t.on_reorder
            ));
            for (i, tab) in t.tabs.iter().enumerate() {
                let pad2 = "  ".repeat(indent + 1);
                out.push_str(&pad2);
                out.push_str(&format!("tab[{i}] id={} name={:?}\n", tab.id, tab.name));
            }
        }
        WidgetNode::ResizableSplit(s) => {
            out.push_str(&format!(
                " split_id={:?} direction={:?}\n",
                s.split_id, s.direction
            ));
            for c in &s.children {
                snapshot_node(c, indent + 1, out);
            }
        }
        WidgetNode::Semantic(s) => {
            out.push_str(&format!(
                " title={:?} label={:?} text={:?} command={:?} disabled={}\n",
                s.title, s.label, s.text, s.command, s.disabled
            ));
            if let Some(c) = &s.child {
                snapshot_node(c, indent + 1, out);
            }
            for c in &s.children {
                snapshot_node(c, indent + 1, out);
            }
            for item in &s.items {
                let pad2 = "  ".repeat(indent + 1);
                out.push_str(&pad2);
                out.push_str(&format!(
                    "item label={:?} text={:?}\n",
                    item.label, item.text
                ));
            }
        }
        WidgetNode::Layout(l) => {
            out.push_str(&format!(
                " direction={:?} columns={} active={}\n",
                l.direction, l.columns, l.active
            ));
            if let Some(c) = &l.child {
                snapshot_node(c, indent + 1, out);
            }
            for c in &l.children {
                snapshot_node(c, indent + 1, out);
            }
            for tab in &l.tabs {
                let pad2 = "  ".repeat(indent + 1);
                out.push_str(&pad2);
                out.push_str(&format!("tab id={} title={:?}\n", tab.id, tab.title));
                if let Some(c) = &tab.child {
                    snapshot_node(c, indent + 2, out);
                }
            }
        }
    }
}

#[cfg(test)]
fn fmt_length(l: AstLength) -> String {
    match l {
        AstLength::Auto => "auto".into(),
        AstLength::Fill => "fill".into(),
        AstLength::Shrink => "shrink".into(),
        AstLength::Fixed(n) => format!("fixed({n})"),
    }
}

#[cfg(test)]
fn fmt_color(c: &Option<AstColor>) -> String {
    match c {
        Some(c) if c.token.is_some() => format!("token({:?})", c.token.as_deref().unwrap()),
        Some(c) => format!("{:?}", c.raw),
        None => "none".into(),
    }
}

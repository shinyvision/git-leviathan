use serde_json::Value;

use crate::plugin::ui::widget_ast::{AstColor, AstLength, WidgetAst};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WidgetMeta {
    pub label: Option<String>,
    pub role: Option<String>,
    pub shortcut: Option<String>,
    pub disabled_reason: Option<String>,
    pub focus_order: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstAssetRef {
    pub path: String,
    pub kind: Option<String>,
    pub handle: Option<String>,
}

impl AstAssetRef {
    pub fn from_path(path: String) -> Self {
        Self {
            path,
            kind: None,
            handle: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticItem {
    pub id: Value,
    pub label: String,
    pub text: Option<String>,
    pub value: Value,
    pub child: Option<Box<WidgetAst>>,
    pub children: Vec<SemanticItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticColumn {
    pub id: String,
    pub title: String,
    pub width: AstLength,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticRow {
    pub id: Value,
    pub cells: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticOption {
    pub value: Value,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticNode {
    pub kind: String,
    pub title: Option<String>,
    pub text: Option<String>,
    pub label: Option<String>,
    pub command: Option<String>,
    pub on_click: Option<String>,
    pub on_change: Option<String>,
    pub value: Value,
    pub disabled: bool,
    pub checked: bool,
    pub selected: Value,
    pub progress: Option<f32>,
    pub language: Option<String>,
    pub color: Option<AstColor>,
    pub spacing: Option<f32>,
    pub child: Option<Box<WidgetAst>>,
    pub children: Vec<WidgetAst>,
    pub items: Vec<SemanticItem>,
    pub columns: Vec<SemanticColumn>,
    pub rows: Vec<SemanticRow>,
    pub options: Vec<SemanticOption>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutTab {
    pub id: Value,
    pub title: String,
    pub child: Option<Box<WidgetAst>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutNode {
    pub kind: String,
    pub direction: Option<String>,
    pub columns: usize,
    pub spacing: Option<f32>,
    pub active: Value,
    pub child: Option<Box<WidgetAst>>,
    pub children: Vec<WidgetAst>,
    pub tabs: Vec<LayoutTab>,
}

pub const SEMANTIC_KINDS: &[&str] = &[
    "command_button",
    "toolbar_button",
    "status_item",
    "badge",
    "tag",
    "list",
    "tree",
    "table",
    "section",
    "form",
    "checkbox",
    "toggle",
    "select",
    "radio_group",
    "divider",
    "tooltip",
    "popover",
    "menu",
    "empty_state",
    "code",
    "diff",
    "commit_ref",
    "branch_ref",
    "remote_ref",
    "progress",
];

pub const LAYOUT_KINDS: &[&str] = &["stack", "grid", "dock", "split", "tabs", "virtual_list"];

pub fn is_semantic_kind(kind: &str) -> bool {
    SEMANTIC_KINDS.contains(&kind)
}

pub fn is_layout_kind(kind: &str) -> bool {
    LAYOUT_KINDS.contains(&kind)
}

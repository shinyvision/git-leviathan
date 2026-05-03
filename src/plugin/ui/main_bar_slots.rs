//! Region-agnostic prepared slot.
//!
//! Plugin slot ops are resolved during plugin load into `PreparedSlot`
//! values keyed by `(region, container, id)`. Per-region appliers in
//! `crate::plugin::host` consume the ops and produce concrete slots
//! (`MainBarSlot`, `TabBarSlot`, ...) using the region-specific
//! `into_*` shims on `PreparedSlot`.
//!
//! widget AST: every cached widget is a typed [`WidgetAst`]. The dynamic
//! variant carries an `Option<WidgetAst>` so the host can store `None`
//! before the first refresh, then a real AST after re-decoding the
//! plugin's Lua return.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use iced::Element;

use crate::message::Message;
use crate::plugin::bridge::widget_tree::{self, build_error_widget, BuildCtx, DispatchScope};
use crate::plugin::slots::Container;
use crate::plugin::ui::widget_ast::WidgetAst;
use crate::widgets::chrome::main_bar::{MainBarSlot, SlotCtx as MainBarSlotCtx};
use crate::widgets::chrome::repo_region::{RepoPaneCtx, RepoPaneSlot};
use crate::widgets::chrome::tab_bar_slots::{TabBarCtx, TabBarSlot};

/// Cache cell shared between the host (writer, on dynamic refresh) and
/// the slot's render closure (reader). `None` means "not yet refreshed
/// successfully" — the renderer falls back to the error widget so the
/// user sees something explicit.
pub type DynamicAstCache = Rc<RefCell<Option<WidgetAst>>>;

#[derive(Clone)]
pub enum SlotWidget {
    Static(WidgetAst),
    Dynamic(DynamicAstCache),
}

#[derive(Clone)]
pub struct PreparedSlot {
    pub plugin_id: String,
    pub id: String,
    pub region: String,
    pub container: Container,
    pub priority: i32,
    pub widget: SlotWidget,
    pub plugin_root: PathBuf,
}

pub enum PreparedSlotOp {
    Add(PreparedSlot),
    Remove {
        region: String,
        container: Container,
        id: String,
    },
    Replace {
        region: String,
        container: Container,
        id: String,
        spec: PreparedSlot,
    },
}

impl PreparedSlot {
    fn render(&self) -> Element<'static, Message> {
        let empty_splits: HashMap<String, Vec<f32>> = HashMap::new();
        let container_str = self.container.key();
        let bc = BuildCtx {
            plugin_id: &self.plugin_id,
            scope: DispatchScope::Slot {
                region: &self.region,
                container: &container_str,
                slot_id: &self.id,
            },
            plugin_root: self.plugin_root.as_path(),
            split_states: &empty_splits,
            active_drag: None,
        };
        match &self.widget {
            SlotWidget::Static(ast) => widget_tree::build(ast, &bc),
            SlotWidget::Dynamic(cache) => {
                let guard = cache.borrow();
                match guard.as_ref() {
                    Some(ast) => widget_tree::build(ast, &bc),
                    None => build_error_widget(
                        "widget.invalid_tree",
                        "dynamic widget has no valid AST yet (check diagnostics)",
                    ),
                }
            }
        }
    }

    pub fn into_main_bar(self) -> MainBarSlot {
        let container = self.container.clone();
        let priority = self.priority;
        let id = self.id.clone();
        let prepared = self;
        MainBarSlot {
            id,
            container,
            priority,
            builder: Box::new(move |_ctx: &MainBarSlotCtx<'_>| prepared.render()),
        }
    }

    pub fn into_tab_bar(self) -> TabBarSlot {
        let container = self.container.clone();
        let priority = self.priority;
        let id = self.id.clone();
        let prepared = self;
        TabBarSlot {
            id,
            container,
            priority,
            builder: Box::new(move |_ctx: &TabBarCtx<'_>| prepared.render()),
        }
    }

    pub fn into_repo_pane(self) -> RepoPaneSlot {
        let container = self.container.clone();
        let priority = self.priority;
        let id = self.id.clone();
        let prepared = self;
        RepoPaneSlot {
            id,
            container,
            priority,
            builder: Box::new(move |_ctx: &RepoPaneCtx<'_>| prepared.render()),
        }
    }
}

pub fn parse_container(raw: &str) -> Container {
    if let Some((pane, section)) = raw.split_once('.') {
        Container::Pane {
            pane: pane.to_string(),
            section: section.to_string(),
        }
    } else {
        Container::Section(raw.to_string())
    }
}

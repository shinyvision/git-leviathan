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
use std::sync::OnceLock;

use iced::Element;
use mlua::RegistryKey;

use crate::message::Message;
use crate::plugin::api::DynamicWidgetCall;
use crate::plugin::bridge::widget_tree::{self, build_error_widget, BuildCtx, DispatchScope};
use crate::plugin::slots::{Container, SlotAddress};
use crate::plugin::ui::invalidation::{DynamicWidgetTelemetry, UiDependency};
use crate::plugin::ui::widget_ast::WidgetAst;
use crate::widgets::chrome::main_bar::{MainBarSlot, SlotCtx as MainBarSlotCtx};
use crate::widgets::chrome::repo_region::{RepoPaneCtx, RepoPaneSlot};
use crate::widgets::chrome::tab_bar_slots::{TabBarCtx, TabBarSlot};

/// Cache cell shared between the host (writer, on dynamic refresh) and
/// the slot's render closure (reader). `None` means "not yet refreshed
/// successfully" — the renderer falls back to the error widget so the
/// user sees something explicit.
pub type DynamicAstCache = Rc<RefCell<Option<WidgetAst>>>;

pub struct DynamicWidgetRegistration {
    pub key: RegistryKey,
    pub cache: DynamicAstCache,
    pub call: DynamicWidgetCall,
    pub dependencies: Vec<UiDependency>,
    pub telemetry: Rc<RefCell<DynamicWidgetTelemetry>>,
}

#[derive(Clone)]
pub enum SlotWidget {
    Static(Box<WidgetAst>),
    Dynamic(DynamicAstCache),
}

#[derive(Clone)]
pub struct PreparedSlot {
    pub plugin_id: String,
    pub id: String,
    pub address: SlotAddress,
    pub mount_address: SlotAddress,
    pub region: String,
    pub container: Container,
    pub priority: i32,
    pub widget: SlotWidget,
    pub plugin_root: PathBuf,
}

#[derive(Clone)]
pub enum PreparedSlotOp {
    Add(PreparedSlot),
    Remove {
        requester_plugin_id: String,
        target: SlotAddress,
    },
    Replace {
        requester_plugin_id: String,
        target: SlotAddress,
        spec: PreparedSlot,
    },
}

impl PreparedSlot {
    fn render(&self) -> Element<'static, Message> {
        static EMPTY_SPLITS: OnceLock<HashMap<String, Vec<f32>>> = OnceLock::new();
        let empty_splits = EMPTY_SPLITS.get_or_init(HashMap::new);
        let container_str = self.container.key();
        let bc = BuildCtx {
            plugin_id: &self.plugin_id,
            scope: DispatchScope::Slot {
                region: &self.region,
                container: &container_str,
                slot_id: &self.id,
            },
            plugin_root: self.plugin_root.as_path(),
            split_states: empty_splits,
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
        let address = self.mount_address.clone();
        let priority = self.priority;
        let id = self.id.clone();
        let prepared = self;
        MainBarSlot {
            id,
            address,
            container,
            priority,
            builder: Box::new(move |_ctx: &MainBarSlotCtx<'_>| prepared.render()),
        }
    }

    pub fn into_tab_bar(self) -> TabBarSlot {
        let container = self.container.clone();
        let address = self.mount_address.clone();
        let priority = self.priority;
        let id = self.id.clone();
        let prepared = self;
        TabBarSlot {
            id,
            address,
            container,
            priority,
            builder: Box::new(move |_ctx: &TabBarCtx<'_>| prepared.render()),
        }
    }

    pub fn into_repo_pane(self) -> RepoPaneSlot {
        let container = self.container.clone();
        let address = self.mount_address.clone();
        let priority = self.priority;
        let id = self.id.clone();
        let prepared = self;
        RepoPaneSlot {
            id,
            address,
            container,
            priority,
            builder: Box::new(move |_ctx: &RepoPaneCtx<'_>| prepared.render()),
        }
    }

    pub fn mounted_at(mut self, target: SlotAddress) -> Self {
        self.mount_address = target;
        self
    }
}

pub fn parse_container(raw: &str) -> Container {
    Container::parse(raw)
}

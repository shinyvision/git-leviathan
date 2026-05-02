//! Region-agnostic prepared slot.
//!
//! Plugin slot ops are resolved during plugin load into `PreparedSlot`
//! values keyed by `(region, container, id)`. Per-region appliers in
//! `crate::plugin::host` consume the ops and produce concrete slots
//! (`MainBarSlot`, `TabBarSlot`, ...) using the region-specific
//! `into_*` shims on `PreparedSlot`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use iced::Element;
use serde_json::Value;

use crate::message::Message;
use crate::plugin::bridge::widget_tree::{self, BuildCtx, DispatchScope};
use crate::plugin::slots::Container;
use crate::widgets::chrome::main_bar::{MainBarSlot, SlotCtx as MainBarSlotCtx};
use crate::widgets::chrome::repo_region::{RepoPaneCtx, RepoPaneSlot};
use crate::widgets::chrome::tab_bar_slots::{TabBarCtx, TabBarSlot};

#[derive(Clone)]
pub enum SlotWidget {
    Static(Value),
    Dynamic(Rc<RefCell<Value>>),
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

#[allow(dead_code)]
pub enum PreparedSlotOp {
    Add(PreparedSlot),
    Remove { region: String, container: Container, id: String },
    /// Compat removal where the user did not specify a container (legacy
    /// `main_bar.remove("id")`). The applier scans every container in the
    /// region.
    RemoveAnyContainer { region: String, id: String },
    Replace { region: String, container: Container, id: String, spec: PreparedSlot },
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
            SlotWidget::Static(tree) => widget_tree::build(tree, &bc),
            SlotWidget::Dynamic(cache) => {
                let guard = cache.borrow();
                widget_tree::build(&guard, &bc)
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

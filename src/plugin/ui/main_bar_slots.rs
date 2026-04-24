//! Plugin-contributed main-bar slots — widget-tree-based, widget-type
//! agnostic.
//!
//! The plugin hands us either an arbitrary widget tree or a function that
//! returns one (same DSL used by plugin screens). The host hands the tree
//! to [`widget_tree::build`] on every render, scoped as a main-bar slot so
//! clickable widgets inside route to [`PluginMessage::SlotClicked`] and
//! ultimately to the slot's `on_click` Lua fn.
//!
//! Dynamic (function-backed) slots share a `Rc<RefCell<Value>>` cache
//! with the [`PluginHost`](crate::plugin::PluginHost). The host writes
//! into the cache after every autocmd fire (and on initial load); the
//! slot builder reads from it at render time. This keeps the builder
//! closure `Fn + 'static` without needing Lua access inside, and keeps
//! the view a pure function of whatever the last refresh produced.
//!
//! The host contributes zero opinions about the slot's look: shape,
//! padding, border, colours, icon vs. text vs. icon-over-text — all
//! declared in Lua.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use serde_json::Value;

use crate::plugin::bridge::widget_tree::{self, BuildCtx, DispatchScope};
use crate::widgets::chrome::main_bar::{MainBarSlot, Section};

/// The source a prepared slot renders from.
///
/// `Dynamic` carries a shared cell the host mutates when plugin state
/// might have changed; the slot builder just reads it. Starting value is
/// whatever the host put there via the post-load refresh (usually the
/// first evaluation of the plugin's widget fn).
#[derive(Clone)]
pub enum SlotWidget {
    Static(Value),
    Dynamic(Rc<RefCell<Value>>),
}

/// Slot spec the host keeps post-resolution. `plugin_root` is needed at
/// render time so icon paths inside the tree resolve against the plugin's
/// sandbox.
#[derive(Clone)]
pub struct PreparedMainBarSlot {
    pub plugin_id: String,
    pub id: String,
    pub section: Section,
    pub priority: i32,
    pub widget: SlotWidget,
    pub plugin_root: PathBuf,
}

/// One ordered hook op after resolution.
pub enum PreparedSlotOp {
    Add(PreparedMainBarSlot),
    Remove(String),
    Replace(String, PreparedMainBarSlot),
}

/// Parse a section string ("left"/"center"/"right", case-insensitive).
pub fn parse_section(raw: &str) -> Result<Section, String> {
    match raw.to_ascii_lowercase().as_str() {
        "left" => Ok(Section::Left),
        "center" | "centre" => Ok(Section::Center),
        "right" => Ok(Section::Right),
        other => Err(format!("unknown section: {other:?} (want left/center/right)")),
    }
}

impl PreparedMainBarSlot {
    pub fn into_main_bar_slot(self) -> MainBarSlot {
        let PreparedMainBarSlot {
            plugin_id,
            id,
            section,
            priority,
            widget,
            plugin_root,
        } = self;

        MainBarSlot::new(id.clone(), section, priority, move |_slot_ctx| {
            // Slots don't use resizable_split, so split_states is empty.
            // The HashMap is cheap — a `new()` allocates nothing until an
            // item is inserted.
            let empty_splits: HashMap<String, Vec<f32>> = HashMap::new();
            let bc = BuildCtx {
                plugin_id: &plugin_id,
                scope: DispatchScope::MainBarSlot { slot_id: &id },
                plugin_root: plugin_root.as_path(),
                split_states: &empty_splits,
                active_drag: None,
            };
            match &widget {
                SlotWidget::Static(tree) => widget_tree::build(tree, &bc),
                SlotWidget::Dynamic(cache) => {
                    let guard = cache.borrow();
                    widget_tree::build(&guard, &bc)
                }
            }
        })
    }
}


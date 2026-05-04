//! Tab-bar slot registry.
//!
//! Mirrors the main-bar surface: typed `Section`, `TabBarCtx` carrying
//! read-only per-render state, `TabBarSlot` with its own builder closure
//! shape, registry alias, section-typed iterator.

use iced::Element;

use crate::message::Message;
use crate::plugin::slots::{Container, IsSlot, SlotAddress, SlotRegistry};
use crate::plugin::tab_snapshot::TabsSnapshot;

/// Where on the tab bar a slot sits. `Left`/`Right` are the chrome edges
/// (plus button, version label by default); `Center` is the tab list
/// itself (`builtin.tab_list` by default — replaced via
/// `leviathan.ui.slot.replace`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Section {
    Left,
    Center,
    Right,
}

impl Section {
    pub fn as_str(self) -> &'static str {
        match self {
            Section::Left => "left",
            Section::Center => "center",
            Section::Right => "right",
        }
    }

    pub fn container(self) -> Container {
        Container::Section(self.as_str().to_string())
    }
}

/// Per-render context. Carries the live tab snapshot so the
/// `builtin.tab_list` slot — and any plugin slot that wants to mirror
/// native tab state — can render without reaching back into `App`.
pub struct TabBarCtx<'a> {
    pub tabs: Option<&'a TabsSnapshot>,
}

impl<'a> TabBarCtx<'a> {
    pub fn with_tabs(tabs: &'a TabsSnapshot) -> Self {
        Self { tabs: Some(tabs) }
    }
}

pub type TabBarBuilder =
    Box<dyn for<'ctx, 'data> Fn(&'ctx TabBarCtx<'data>) -> Element<'data, Message> + 'static>;

pub struct TabBarSlot {
    pub id: String,
    pub address: SlotAddress,
    pub container: Container,
    pub priority: i32,
    pub builder: TabBarBuilder,
}

impl TabBarSlot {
    pub fn new<F>(id: impl Into<String>, section: Section, priority: i32, builder: F) -> Self
    where
        F: for<'ctx, 'data> Fn(&'ctx TabBarCtx<'data>) -> Element<'data, Message> + 'static,
    {
        let id = id.into();
        let container = section.container();
        Self {
            address: SlotAddress::builtin("tab_bar", container.clone(), id.clone()),
            id,
            container,
            priority,
            builder: Box::new(builder),
        }
    }
}

impl IsSlot for TabBarSlot {
    fn address(&self) -> &SlotAddress {
        &self.address
    }
    fn display_id(&self) -> &str {
        &self.id
    }
    fn priority(&self) -> i32 {
        self.priority
    }
    fn set_priority(&mut self, priority: i32) {
        self.priority = priority;
    }
}

pub type TabBarRegistry = SlotRegistry<TabBarSlot>;

pub fn iter_section<'a>(
    registry: &'a TabBarRegistry,
    section: Section,
) -> impl Iterator<Item = &'a TabBarSlot> + 'a {
    registry.iter_container(section.container())
}

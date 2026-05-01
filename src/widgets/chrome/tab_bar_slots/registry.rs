//! Tab-bar slot registry.
//!
//! Mirrors the main-bar surface: typed `Section`, `TabBarCtx` carrying
//! read-only per-render state, `TabBarSlot` with its own builder closure
//! shape, registry alias, section-typed iterator.

use iced::Element;

use crate::message::Message;
use crate::plugin::slots::{Container, IsSlot, SlotRegistry};

/// Where on the tab bar the slot sits. Middle is the tabs themselves, so
/// plugins only target leading or trailing edges.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Section {
    Left,
    Right,
}

#[allow(dead_code)]
impl Section {
    pub fn as_str(self) -> &'static str {
        match self {
            Section::Left => "left",
            Section::Right => "right",
        }
    }

    pub fn container(self) -> Container {
        Container::Section(self.as_str().to_string())
    }
}

/// Per-render context. Empty for now — kept as a distinct type so future
/// plumbing (active tab id, total count, etc.) doesn't change the shape.
pub struct TabBarCtx<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> TabBarCtx<'a> {
    pub fn new() -> Self {
        Self { _marker: std::marker::PhantomData }
    }
}

impl<'a> Default for TabBarCtx<'a> {
    fn default() -> Self { Self::new() }
}

pub type TabBarBuilder = Box<
    dyn for<'ctx, 'data> Fn(&'ctx TabBarCtx<'data>) -> Element<'data, Message> + 'static,
>;

#[allow(dead_code)]
pub struct TabBarSlot {
    pub id: String,
    pub container: Container,
    pub priority: i32,
    pub builder: TabBarBuilder,
}

#[allow(dead_code)]
impl TabBarSlot {
    pub fn new<F>(
        id: impl Into<String>,
        section: Section,
        priority: i32,
        builder: F,
    ) -> Self
    where
        F: for<'ctx, 'data> Fn(&'ctx TabBarCtx<'data>) -> Element<'data, Message> + 'static,
    {
        Self {
            id: id.into(),
            container: section.container(),
            priority,
            builder: Box::new(builder),
        }
    }
}

#[allow(dead_code)]
impl IsSlot for TabBarSlot {
    fn id(&self) -> &str { &self.id }
    fn container(&self) -> &Container { &self.container }
    fn priority(&self) -> i32 { self.priority }
}

#[allow(dead_code)]
pub type TabBarRegistry = SlotRegistry<TabBarSlot>;

#[allow(dead_code)]
pub fn iter_section<'a>(
    registry: &'a TabBarRegistry,
    section: Section,
) -> impl Iterator<Item = &'a TabBarSlot> + 'a {
    registry.iter_container(section.container())
}

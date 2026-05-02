//! Repository region — plugin-extensible content region with three
//! built-in panes (sidebar, graph, details), each exposing top/bottom
//! extension sections.

use iced::Element;

use crate::message::Message;
use crate::plugin::slots::{Container, IsSlot, SlotRegistry};

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Pane {
    Sidebar,
    Graph,
    Details,
}

#[allow(dead_code)]
impl Pane {
    pub fn as_str(self) -> &'static str {
        match self {
            Pane::Sidebar => "sidebar",
            Pane::Graph => "graph",
            Pane::Details => "details",
        }
    }
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Section {
    Top,
    Bottom,
}

#[allow(dead_code)]
impl Section {
    pub fn as_str(self) -> &'static str {
        match self {
            Section::Top => "top",
            Section::Bottom => "bottom",
        }
    }
}

#[allow(dead_code)]
pub fn container(pane: Pane, section: Section) -> Container {
    Container::Pane {
        pane: pane.as_str().to_string(),
        section: section.as_str().to_string(),
    }
}

/// Per-render context for repo-pane slots. Empty for now — kept as a
/// distinct type so future ctx fields (current branch, selection,
/// commit count, etc.) can be added without changing the registry shape.
pub struct RepoPaneCtx<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}

#[allow(dead_code)]
impl<'a> RepoPaneCtx<'a> {
    pub fn new() -> Self {
        Self { _marker: std::marker::PhantomData }
    }
}

impl<'a> Default for RepoPaneCtx<'a> {
    fn default() -> Self { Self::new() }
}

pub type RepoPaneBuilder = Box<
    dyn for<'ctx, 'data> Fn(&'ctx RepoPaneCtx<'data>) -> Element<'data, Message> + 'static,
>;

#[allow(dead_code)]
pub struct RepoPaneSlot {
    pub id: String,
    pub container: Container,
    pub priority: i32,
    pub builder: RepoPaneBuilder,
}

#[allow(dead_code)]
impl RepoPaneSlot {
    pub fn new<F>(
        id: impl Into<String>,
        pane: Pane,
        section: Section,
        priority: i32,
        builder: F,
    ) -> Self
    where
        F: for<'ctx, 'data> Fn(&'ctx RepoPaneCtx<'data>) -> Element<'data, Message> + 'static,
    {
        Self {
            id: id.into(),
            container: container(pane, section),
            priority,
            builder: Box::new(builder),
        }
    }
}

#[allow(dead_code)]
impl IsSlot for RepoPaneSlot {
    fn id(&self) -> &str { &self.id }
    fn container(&self) -> &Container { &self.container }
    fn priority(&self) -> i32 { self.priority }
}

#[allow(dead_code)]
pub type RepoRegionRegistry = SlotRegistry<RepoPaneSlot>;

#[allow(dead_code)]
pub fn iter<'a>(
    registry: &'a RepoRegionRegistry,
    pane: Pane,
    section: Section,
) -> impl Iterator<Item = &'a RepoPaneSlot> + 'a {
    registry.iter_container(container(pane, section))
}

/// Returns true when no slots are registered for the pane+section. Used
/// by view code to skip wrapping the built-in pane in an extra column
/// when no extensions are present.
#[allow(dead_code)]
pub fn is_empty(registry: &RepoRegionRegistry, pane: Pane, section: Section) -> bool {
    iter(registry, pane, section).next().is_none()
}

/// Render the top-section slots for a pane as a stacked column. Returns
/// `None` if the pane has no top slots; callers should render their body
/// alone in that case.
pub fn render_top<'a>(
    registry: &'a RepoRegionRegistry,
    pane: Pane,
) -> Option<Element<'a, Message>> {
    if is_empty(registry, pane, Section::Top) {
        return None;
    }
    let pane_ctx = RepoPaneCtx::new();
    let items: Vec<Element<'a, Message>> = iter(registry, pane, Section::Top)
        .map(|s| (s.builder)(&pane_ctx))
        .collect();
    Some(iced::widget::column(items).spacing(0).into())
}

/// Render the bottom-section slots for a pane as a stacked column.
/// Returns `None` if no bottom slots are registered.
pub fn render_bottom<'a>(
    registry: &'a RepoRegionRegistry,
    pane: Pane,
) -> Option<Element<'a, Message>> {
    if is_empty(registry, pane, Section::Bottom) {
        return None;
    }
    let pane_ctx = RepoPaneCtx::new();
    let items: Vec<Element<'a, Message>> = iter(registry, pane, Section::Bottom)
        .map(|s| (s.builder)(&pane_ctx))
        .collect();
    Some(iced::widget::column(items).spacing(0).into())
}

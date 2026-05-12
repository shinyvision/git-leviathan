use std::collections::HashMap;

use iced::Element;

use crate::message::Message;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ChromePane {
    Sidebar,
    Graph,
    Details,
    Diff,
}

impl ChromePane {
    pub fn as_str(self) -> &'static str {
        match self {
            ChromePane::Sidebar => "sidebar",
            ChromePane::Graph => "graph",
            ChromePane::Details => "details",
            ChromePane::Diff => "diff",
        }
    }

    pub fn point_id(self) -> &'static str {
        match self {
            ChromePane::Sidebar => "repository.sidebar.chrome",
            ChromePane::Graph => "repository.graph.chrome",
            ChromePane::Details => "repository.details.chrome",
            ChromePane::Diff => "repository.diff.chrome",
        }
    }

    pub fn from_point_id(point_id: &str) -> Option<Self> {
        match point_id {
            "repository.sidebar.chrome" => Some(ChromePane::Sidebar),
            "repository.graph.chrome" => Some(ChromePane::Graph),
            "repository.details.chrome" => Some(ChromePane::Details),
            "repository.diff.chrome" => Some(ChromePane::Diff),
            _ => None,
        }
    }
}

pub struct RepoPaneChromeCtx<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> RepoPaneChromeCtx<'a> {
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a> Default for RepoPaneChromeCtx<'a> {
    fn default() -> Self {
        Self::new()
    }
}

pub type ChromeBuilder = Box<
    dyn for<'ctx, 'data> Fn(&'ctx RepoPaneChromeCtx<'data>) -> Option<Element<'data, Message>>
        + 'static,
>;

pub struct RepoChromeSlot {
    pub plugin_id: String,
    pub id: String,
    pub priority: i32,
    pub builder: ChromeBuilder,
}

#[derive(Default)]
pub struct RepoChromeRegistry {
    panes: HashMap<ChromePane, Vec<RepoChromeSlot>>,
}

impl RepoChromeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, pane: ChromePane, slot: RepoChromeSlot) {
        let entry = self.panes.entry(pane).or_default();
        entry.push(slot);
        entry.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.plugin_id.cmp(&b.plugin_id))
                .then_with(|| a.id.cmp(&b.id))
        });
    }

    pub fn iter(&self, pane: ChromePane) -> impl Iterator<Item = &RepoChromeSlot> + '_ {
        self.panes.get(&pane).into_iter().flat_map(|v| v.iter())
    }

    pub fn is_empty(&self, pane: ChromePane) -> bool {
        self.panes.get(&pane).is_none_or(|v| v.is_empty())
    }

    pub fn render_overlay<'a>(&'a self, pane: ChromePane) -> Option<Vec<Element<'a, Message>>> {
        if self.is_empty(pane) {
            return None;
        }
        let ctx = RepoPaneChromeCtx::new();
        let layers: Vec<Element<'a, Message>> = self
            .iter(pane)
            .filter_map(|slot| (slot.builder)(&ctx))
            .collect();
        if layers.is_empty() {
            None
        } else {
            Some(layers)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text;

    fn make_slot(plugin: &str, id: &str, priority: i32) -> RepoChromeSlot {
        RepoChromeSlot {
            plugin_id: plugin.to_string(),
            id: id.to_string(),
            priority,
            builder: Box::new(|_ctx| Some(text("x").into())),
        }
    }

    #[test]
    fn empty_registry_yields_no_overlay() {
        let reg = RepoChromeRegistry::new();
        assert!(reg.is_empty(ChromePane::Graph));
        assert!(reg.render_overlay(ChromePane::Graph).is_none());
    }

    #[test]
    fn insert_sorts_by_priority_then_plugin_then_id() {
        let mut reg = RepoChromeRegistry::new();
        reg.insert(ChromePane::Graph, make_slot("zeta", "y", 10));
        reg.insert(ChromePane::Graph, make_slot("alpha", "z", 10));
        reg.insert(ChromePane::Graph, make_slot("alpha", "a", 10));
        reg.insert(ChromePane::Graph, make_slot("alpha", "a", 5));
        let order: Vec<(String, String, i32)> = reg
            .iter(ChromePane::Graph)
            .map(|s| (s.plugin_id.clone(), s.id.clone(), s.priority))
            .collect();
        assert_eq!(
            order,
            vec![
                ("alpha".into(), "a".into(), 5),
                ("alpha".into(), "a".into(), 10),
                ("alpha".into(), "z".into(), 10),
                ("zeta".into(), "y".into(), 10),
            ]
        );
    }

    #[test]
    fn from_point_id_round_trip() {
        for pane in [
            ChromePane::Sidebar,
            ChromePane::Graph,
            ChromePane::Details,
            ChromePane::Diff,
        ] {
            assert_eq!(ChromePane::from_point_id(pane.point_id()), Some(pane));
        }
        assert!(ChromePane::from_point_id("repository.bogus.chrome").is_none());
    }
}

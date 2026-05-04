use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockArea {
    LeftSidebar,
    RightInspector,
    BottomPanel,
    GraphAdjacent,
    DiffAdjacent,
    TabContent,
    FloatingPopout,
}

impl DockArea {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "left" | "left_sidebar" => Ok(Self::LeftSidebar),
            "right" | "right_inspector" => Ok(Self::RightInspector),
            "bottom" | "bottom_panel" => Ok(Self::BottomPanel),
            "graph" | "graph_adjacent" => Ok(Self::GraphAdjacent),
            "diff" | "diff_adjacent" => Ok(Self::DiffAdjacent),
            "tab" | "tab_content" => Ok(Self::TabContent),
            "floating" | "floating_popout" | "popout" => Ok(Self::FloatingPopout),
            other => Err(format!("unknown dock area `{other}`")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LeftSidebar => "left",
            Self::RightInspector => "right",
            Self::BottomPanel => "bottom",
            Self::GraphAdjacent => "graph",
            Self::DiffAdjacent => "diff",
            Self::TabContent => "tab",
            Self::FloatingPopout => "floating",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::LeftSidebar => "left sidebar",
            Self::RightInspector => "right inspector",
            Self::BottomPanel => "bottom panel",
            Self::GraphAdjacent => "graph-adjacent panel",
            Self::DiffAdjacent => "diff-adjacent panel",
            Self::TabContent => "tab content",
            Self::FloatingPopout => "floating popout",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DockPanelRegistration {
    pub plugin_id: String,
    pub generation_id: u64,
    pub id: String,
    pub title: String,
    pub area: DockArea,
    pub default_open: bool,
    pub source_location: Option<String>,
}

impl DockPanelRegistration {
    pub fn key(&self) -> String {
        panel_key(&self.plugin_id, &self.id)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DockPanelSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub id: String,
    pub key: String,
    pub title: String,
    pub area: String,
    pub area_label: String,
    pub open: bool,
    pub order: usize,
    pub split_size: Option<f32>,
    pub selected: bool,
    pub state: Value,
    pub source_location: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct DockLayoutFile {
    version: u32,
    layout: DockLayout,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct DockLayout {
    areas: BTreeMap<String, DockAreaLayout>,
    panel_states: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct DockAreaLayout {
    order: Vec<String>,
    open: BTreeMap<String, bool>,
    split_size: Option<f32>,
    selected: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DockManager {
    inner: Rc<RefCell<DockManagerInner>>,
}

#[derive(Debug, Default)]
struct DockManagerInner {
    registrations: BTreeMap<String, DockPanelRegistration>,
    layout: DockLayout,
    persist_path: Option<PathBuf>,
}

impl Default for DockManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DockManager {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(DockManagerInner::default())),
        }
    }

    pub fn set_persist_path(&self, path: PathBuf) {
        let registrations = {
            let mut inner = self.inner.borrow_mut();
            inner.persist_path = Some(path.clone());
            inner.layout = load_layout(&path).unwrap_or_default();
            inner.registrations.values().cloned().collect::<Vec<_>>()
        };
        for registration in registrations {
            self.register(registration);
        }
    }

    pub fn register(&self, registration: DockPanelRegistration) {
        let key = registration.key();
        {
            let mut inner = self.inner.borrow_mut();
            remove_from_other_areas(&mut inner.layout, registration.area.as_str(), &key);
            inner
                .registrations
                .insert(key.clone(), registration.clone());
            let area = inner
                .layout
                .areas
                .entry(registration.area.as_str().to_string())
                .or_default();
            if !area.order.iter().any(|existing| existing == &key) {
                area.order.push(key.clone());
            }
            area.open
                .entry(key.clone())
                .or_insert(registration.default_open);
            if area.selected.is_none() && *area.open.get(&key).unwrap_or(&false) {
                area.selected = Some(key);
            }
            save_inner(&inner);
        }
    }

    pub fn drop_registration_key(&self, key: &str) {
        let mut inner = self.inner.borrow_mut();
        inner.registrations.remove(key);
        save_inner(&inner);
    }

    pub fn drop_registrations_for_plugin(&self, plugin_id: &str) {
        let mut inner = self.inner.borrow_mut();
        inner
            .registrations
            .retain(|key, _| !key.starts_with(&format!("{plugin_id}:")));
        save_inner(&inner);
    }

    pub fn remove_plugin(&self, plugin_id: &str) {
        let mut inner = self.inner.borrow_mut();
        let keys = inner
            .registrations
            .keys()
            .filter(|key| key.starts_with(&format!("{plugin_id}:")))
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            inner.registrations.remove(&key);
            inner.layout.panel_states.remove(&key);
            for area in inner.layout.areas.values_mut() {
                area.order.retain(|candidate| candidate != &key);
                area.open.remove(&key);
                if area.selected.as_deref() == Some(key.as_str()) {
                    area.selected = first_open_key(area);
                }
            }
        }
        save_inner(&inner);
    }

    pub fn open_panel(&self, plugin_id: &str, id: &str) -> Result<(), String> {
        self.set_open(&panel_key(plugin_id, id), true)
    }

    pub fn close_panel(&self, plugin_id: &str, id: &str) -> Result<(), String> {
        self.set_open(&panel_key(plugin_id, id), false)
    }

    pub fn move_panel(
        &self,
        plugin_id: &str,
        id: &str,
        area: DockArea,
        order: Option<usize>,
    ) -> Result<(), String> {
        let key = panel_key(plugin_id, id);
        let mut inner = self.inner.borrow_mut();
        let registration = inner
            .registrations
            .get_mut(&key)
            .ok_or_else(|| format!("dock panel `{plugin_id}.{id}` is not registered"))?;
        registration.area = area;
        remove_from_all_areas(&mut inner.layout, &key);
        let area_layout = inner
            .layout
            .areas
            .entry(area.as_str().to_string())
            .or_default();
        let insert_at = order
            .unwrap_or(area_layout.order.len())
            .min(area_layout.order.len());
        area_layout.order.insert(insert_at, key.clone());
        area_layout.open.entry(key.clone()).or_insert(true);
        area_layout.selected = Some(key);
        save_inner(&inner);
        Ok(())
    }

    pub fn set_split_size(&self, area: DockArea, size: f32) {
        let mut inner = self.inner.borrow_mut();
        inner
            .layout
            .areas
            .entry(area.as_str().to_string())
            .or_default()
            .split_size = Some(size);
        save_inner(&inner);
    }

    pub fn reset_layout(&self) {
        let registrations = {
            let mut inner = self.inner.borrow_mut();
            inner.layout = DockLayout::default();
            inner.registrations.values().cloned().collect::<Vec<_>>()
        };
        for registration in registrations {
            self.register(registration);
        }
    }

    pub fn state(&self, plugin_id: &str, id: &str) -> Value {
        self.inner
            .borrow()
            .layout
            .panel_states
            .get(&panel_key(plugin_id, id))
            .cloned()
            .unwrap_or_else(|| json!({}))
    }

    pub fn set_state(&self, plugin_id: &str, id: &str, state: Value) {
        let mut inner = self.inner.borrow_mut();
        inner
            .layout
            .panel_states
            .insert(panel_key(plugin_id, id), state);
        save_inner(&inner);
    }

    pub fn summaries(&self) -> Vec<DockPanelSummary> {
        let inner = self.inner.borrow();
        let mut rows = inner
            .registrations
            .values()
            .map(|registration| summary_for(&inner.layout, registration))
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            a.area
                .cmp(&b.area)
                .then(a.order.cmp(&b.order))
                .then(a.plugin_id.cmp(&b.plugin_id))
                .then(a.id.cmp(&b.id))
        });
        rows
    }

    pub fn layout_json(&self) -> Value {
        let inner = self.inner.borrow();
        json!({
            "version": 1,
            "areas": inner.layout.areas,
            "panel_states": inner.layout.panel_states,
            "registered": inner.registrations.keys().cloned().collect::<Vec<_>>(),
        })
    }

    pub fn is_registered(&self, plugin_id: &str, id: &str) -> bool {
        self.inner
            .borrow()
            .registrations
            .contains_key(&panel_key(plugin_id, id))
    }

    fn set_open(&self, key: &str, open: bool) -> Result<(), String> {
        let mut inner = self.inner.borrow_mut();
        let area_name = inner
            .registrations
            .get(key)
            .map(|registration| registration.area.as_str().to_string())
            .ok_or_else(|| format!("dock panel `{key}` is not registered"))?;
        let area = inner.layout.areas.entry(area_name).or_default();
        area.open.insert(key.to_string(), open);
        if open {
            area.selected = Some(key.to_string());
        } else if area.selected.as_deref() == Some(key) {
            area.selected = first_open_key(area);
        }
        save_inner(&inner);
        Ok(())
    }
}

pub fn panel_key(plugin_id: &str, id: &str) -> String {
    format!("{plugin_id}:{id}")
}

fn summary_for(layout: &DockLayout, registration: &DockPanelRegistration) -> DockPanelSummary {
    let key = registration.key();
    let area_name = registration.area.as_str();
    let area = layout.areas.get(area_name);
    let order = area
        .and_then(|area| area.order.iter().position(|candidate| candidate == &key))
        .unwrap_or(usize::MAX);
    let open = area
        .and_then(|area| area.open.get(&key).copied())
        .unwrap_or(registration.default_open);
    DockPanelSummary {
        plugin_id: registration.plugin_id.clone(),
        generation_id: registration.generation_id,
        id: registration.id.clone(),
        key,
        title: registration.title.clone(),
        area: area_name.to_string(),
        area_label: registration.area.label().to_string(),
        open,
        order,
        split_size: area.and_then(|area| area.split_size),
        selected: area.and_then(|area| area.selected.as_deref())
            == Some(registration.key().as_str()),
        state: layout
            .panel_states
            .get(&registration.key())
            .cloned()
            .unwrap_or_else(|| json!({})),
        source_location: registration.source_location.clone(),
    }
}

fn remove_from_other_areas(layout: &mut DockLayout, keep_area: &str, key: &str) {
    for (area_name, area) in layout.areas.iter_mut() {
        if area_name == keep_area {
            continue;
        }
        remove_from_area(area, key);
    }
}

fn remove_from_all_areas(layout: &mut DockLayout, key: &str) {
    for area in layout.areas.values_mut() {
        remove_from_area(area, key);
    }
}

fn remove_from_area(area: &mut DockAreaLayout, key: &str) {
    area.order.retain(|candidate| candidate != key);
    area.open.remove(key);
    if area.selected.as_deref() == Some(key) {
        area.selected = first_open_key(area);
    }
}

fn first_open_key(area: &DockAreaLayout) -> Option<String> {
    let open = area
        .open
        .iter()
        .filter_map(|(key, open)| (*open).then_some(key.clone()))
        .collect::<BTreeSet<_>>();
    area.order
        .iter()
        .find(|key| open.contains(*key))
        .cloned()
        .or_else(|| open.into_iter().next())
}

fn load_layout(path: &std::path::Path) -> Option<DockLayout> {
    let raw = std::fs::read_to_string(path).ok()?;
    let file = serde_json::from_str::<DockLayoutFile>(&raw).ok()?;
    (file.version == 1).then_some(file.layout)
}

fn save_inner(inner: &DockManagerInner) {
    let Some(path) = &inner.persist_path else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = DockLayoutFile {
        version: 1,
        layout: inner.layout.clone(),
    };
    if let Ok(raw) = serde_json::to_string_pretty(&file) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, raw).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(id: &str, area: DockArea) -> DockPanelRegistration {
        DockPanelRegistration {
            plugin_id: "p".into(),
            generation_id: 1,
            id: id.into(),
            title: id.into(),
            area,
            default_open: true,
            source_location: None,
        }
    }

    #[test]
    fn reload_style_reregister_preserves_state_and_open() {
        let dock = DockManager::new();
        dock.register(reg("x", DockArea::RightInspector));
        dock.close_panel("p", "x").unwrap();
        dock.set_state("p", "x", json!({"n": 1}));
        dock.drop_registration_key("p:x");
        dock.register(DockPanelRegistration {
            generation_id: 2,
            ..reg("x", DockArea::RightInspector)
        });
        let row = dock.summaries().pop().unwrap();
        assert!(!row.open);
        assert_eq!(row.state, json!({"n": 1}));
        assert_eq!(row.generation_id, 2);
    }

    #[test]
    fn unload_removes_layout_entries() {
        let dock = DockManager::new();
        dock.register(reg("x", DockArea::RightInspector));
        dock.set_state("p", "x", json!({"n": 1}));
        dock.remove_plugin("p");
        assert!(dock.summaries().is_empty());
        assert_eq!(dock.state("p", "x"), json!({}));
    }
}

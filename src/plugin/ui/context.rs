use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{json, Value};

use crate::plugin::resources::GenerationId;
use crate::plugin::slots::{Container, SlotAddress};
use crate::plugin::tab_snapshot::TabsSnapshot;
use crate::theme;

#[derive(Clone)]
pub struct UiContextStore {
    current: Rc<RefCell<Value>>,
}

#[derive(Debug, Clone)]
pub struct RepositoryContextSnapshot {
    pub name: String,
    pub workdir_path: String,
    pub current_branch_name: String,
    pub head_hash: String,
    pub default_remote_name: String,
    pub has_remote: bool,
}

pub enum UiContextSurface {
    MainBar {
        section: String,
    },
    TabBar {
        section: String,
    },
    RepositorySidebar {
        section: String,
    },
    RepositoryGraph {
        section: String,
    },
    RepositoryDetails {
        section: String,
    },
    Overlay,
    Screen,
    StatusBar,
    RepositoryDiff,
    RepositoryGraphRow,
    RepositoryDiffLine,
    Settings,
    DockPanel {
        area: String,
        panel_id: String,
        title: String,
        open: bool,
        state: Value,
    },
}

impl UiContextStore {
    pub fn new(plugin_id: &str, generation_id: GenerationId) -> Self {
        Self {
            current: Rc::new(RefCell::new(context_for_surface(
                plugin_id,
                generation_id,
                UiContextSurface::Screen,
                None,
                &TabsSnapshot::default(),
            ))),
        }
    }

    pub fn get(&self) -> Value {
        self.current.borrow().clone()
    }

    pub fn set(&self, value: Value) {
        *self.current.borrow_mut() = value;
    }
}

pub fn context_for_slot(
    plugin_id: &str,
    generation_id: GenerationId,
    address: &SlotAddress,
    repository: Option<&RepositoryContextSnapshot>,
    tabs: &TabsSnapshot,
) -> Value {
    let surface = match (address.region().as_str(), address.container()) {
        ("main_bar", Container::Section(section)) => UiContextSurface::MainBar {
            section: section.clone(),
        },
        ("tab_bar", Container::Section(section)) => UiContextSurface::TabBar {
            section: section.clone(),
        },
        ("repository", Container::Pane { pane, section }) if pane == "sidebar" => {
            UiContextSurface::RepositorySidebar {
                section: section.clone(),
            }
        }
        ("repository", Container::Pane { pane, section }) if pane == "graph" => {
            UiContextSurface::RepositoryGraph {
                section: section.clone(),
            }
        }
        ("repository", Container::Pane { pane, section }) if pane == "details" => {
            UiContextSurface::RepositoryDetails {
                section: section.clone(),
            }
        }
        _ => UiContextSurface::Screen,
    };
    context_for_surface(plugin_id, generation_id, surface, repository, tabs)
}

pub fn context_for_surface(
    plugin_id: &str,
    generation_id: GenerationId,
    surface: UiContextSurface,
    repository: Option<&RepositoryContextSnapshot>,
    tabs: &TabsSnapshot,
) -> Value {
    let (context_type, surface_name, focus, payload) = surface_parts(surface);
    json!({
        "version": 1,
        "type": context_type,
        "plugin_id": plugin_id,
        "generation_id": generation_id.get(),
        "surface": surface_name,
        "features": features(),
        "theme": theme_tokens(),
        "repository": repository_summary(repository),
        "tab": tab_summary(tabs),
        "selection": selection_summary(),
        "focus": focus,
        "viewport": viewport_summary(),
        "payload": payload,
    })
}

pub fn context_for_dock_panel(
    plugin_id: &str,
    generation_id: GenerationId,
    panel: &crate::plugin::dock::DockPanelSummary,
    repository: Option<&RepositoryContextSnapshot>,
    tabs: &TabsSnapshot,
) -> Value {
    context_for_surface(
        plugin_id,
        generation_id,
        UiContextSurface::DockPanel {
            area: panel.area.clone(),
            panel_id: panel.id.clone(),
            title: panel.title.clone(),
            open: panel.open,
            state: panel.state.clone(),
        },
        repository,
        tabs,
    )
}

fn surface_parts(surface: UiContextSurface) -> (&'static str, &'static str, Value, Value) {
    match surface {
        UiContextSurface::MainBar { section } => (
            "MainBarContext",
            "main_bar",
            json!({ "surface": "main_bar", "region": "main_bar", "section": section }),
            json!({ "main_bar": { "section": section } }),
        ),
        UiContextSurface::TabBar { section } => (
            "TabBarContext",
            "tab_bar",
            json!({ "surface": "tab_bar", "region": "tab_bar", "section": section }),
            json!({ "tab_bar": { "section": section } }),
        ),
        UiContextSurface::RepositorySidebar { section } => repository_surface(
            "RepositorySidebarContext",
            "repository.sidebar",
            "sidebar",
            section,
        ),
        UiContextSurface::RepositoryGraph { section } => repository_surface(
            "RepositoryGraphContext",
            "repository.graph",
            "graph",
            section,
        ),
        UiContextSurface::RepositoryDetails { section } => repository_surface(
            "RepositoryDetailsContext",
            "repository.details",
            "details",
            section,
        ),
        UiContextSurface::Overlay => simple_surface("OverlayContext", "overlay"),
        UiContextSurface::Screen => simple_surface("ScreenContext", "screen"),
        UiContextSurface::StatusBar => simple_surface("StatusBarContext", "status_bar"),
        UiContextSurface::RepositoryDiff => {
            simple_surface("RepositoryDiffContext", "repository.diff")
        }
        UiContextSurface::RepositoryGraphRow => {
            simple_surface("RepositoryGraphRowContext", "repository.graph.row")
        }
        UiContextSurface::RepositoryDiffLine => {
            simple_surface("RepositoryDiffLineContext", "repository.diff.line")
        }
        UiContextSurface::Settings => simple_surface("SettingsContext", "settings"),
        UiContextSurface::DockPanel {
            area,
            panel_id,
            title,
            open,
            state,
        } => (
            "DockPanelContext",
            "dock.panel",
            json!({ "surface": "dock.panel", "area": area.clone(), "panel_id": panel_id.clone() }),
            json!({ "dock": { "area": area, "panel_id": panel_id, "title": title, "open": open, "state": state } }),
        ),
    }
}

fn repository_surface(
    context_type: &'static str,
    surface: &'static str,
    pane: &'static str,
    section: String,
) -> (&'static str, &'static str, Value, Value) {
    (
        context_type,
        surface,
        json!({ "surface": surface, "region": "repository", "pane": pane, "section": section }),
        json!({ "repository": { "pane": pane, "section": section } }),
    )
}

fn simple_surface(
    context_type: &'static str,
    surface: &'static str,
) -> (&'static str, &'static str, Value, Value) {
    (
        context_type,
        surface,
        json!({ "surface": surface }),
        json!({}),
    )
}

fn features() -> Value {
    json!({
        "ui.slot@1": true,
        "ui.context@1": true,
        "ui.context.typed@1": true,
        "ui.contribute@1": true,
        "ui.dock@1": true,
    })
}

fn repository_summary(repository: Option<&RepositoryContextSnapshot>) -> Value {
    match repository {
        Some(repo) => {
            let is_open = !repo.name.is_empty() || !repo.workdir_path.is_empty();
            json!({
                "is_open": is_open,
                "name": repo.name,
                "workdir_path": repo.workdir_path,
                "current_branch_name": repo.current_branch_name,
                "head_hash": repo.head_hash,
                "default_remote_name": repo.default_remote_name,
                "has_remote": repo.has_remote,
            })
        }
        None => json!({
            "is_open": false,
            "name": "",
            "workdir_path": "",
            "current_branch_name": "",
            "head_hash": "",
            "default_remote_name": "",
            "has_remote": false,
        }),
    }
}

fn tab_summary(tabs: &TabsSnapshot) -> Value {
    let active = tabs.active_id.and_then(|id| {
        tabs.tabs
            .iter()
            .position(|tab| tab.id == id)
            .map(|idx| (id, idx))
    });
    match active {
        Some((id, index)) => {
            let tab = &tabs.tabs[index];
            json!({
                "is_open": true,
                "id": id.raw(),
                "path": tab.path,
                "name": tab.name,
                "index": index,
                "count": tabs.tabs.len(),
            })
        }
        None => json!({
            "is_open": false,
            "id": null,
            "path": "",
            "name": "",
            "index": null,
            "count": tabs.tabs.len(),
        }),
    }
}

fn selection_summary() -> Value {
    json!({
        "available": false,
        "kind": "none",
        "selected_commit_id": null,
        "selected_file_path": null,
    })
}

fn viewport_summary() -> Value {
    json!({
        "known": false,
        "width": null,
        "height": null,
    })
}

fn theme_tokens() -> Value {
    json!({
        "name": "leviathan",
        "colors": {
            "background": color_hex(theme::BG_BASE),
            "panel": color_hex(theme::BG_PANEL),
            "toolbar": color_hex(theme::BG_TOOLBAR),
            "text_primary": color_hex(theme::TEXT_PRIMARY),
            "text_secondary": color_hex(theme::TEXT_SECONDARY),
            "accent_blue": color_hex(theme::ACCENT_BLUE),
            "accent_green": color_hex(theme::ACCENT_GREEN),
            "accent_orange": color_hex(theme::ACCENT_ORANGE),
            "border": color_hex(theme::BORDER),
        },
        "dimensions": {
            "sidebar_width": theme::SIDEBAR_WIDTH,
            "detail_panel_width": theme::DETAIL_PANEL_WIDTH,
            "tab_height": theme::TAB_HEIGHT,
            "toolbar_height": theme::TOOLBAR_HEIGHT,
            "row_height": theme::ROW_H,
            "lane_width": theme::LANE_WIDTH,
        },
        "fonts": {
            "xs": theme::FONT_XS,
            "sm": theme::FONT_SM,
            "md": theme::FONT_MD,
            "lg": theme::FONT_LG,
            "diff": theme::FONT_DIFF,
        },
    })
}

fn color_hex(color: iced::Color) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
}

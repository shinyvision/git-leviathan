use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{json, Value};

use crate::plugin::resources::GenerationId;
use crate::plugin::slots::{Container, SlotAddress};
use crate::plugin::tab_snapshot::TabsSnapshot;
use crate::plugin::ui::focus::{project_active_focus, FocusSnapshot};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionContextSnapshot {
    pub available: bool,
    pub kind: String,
    pub selected_commit_id: Option<String>,
    pub selected_file_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarDialogContextSnapshot {
    pub active: bool,
    pub id: String,
    pub owner: String,
    pub plugin_id: Option<String>,
    pub data: Vec<ToolbarDialogDataContextSnapshot>,
    pub controls: Vec<ToolbarDialogControlContextSnapshot>,
    pub buttons: Vec<ToolbarDialogButtonContextSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarDialogDataContextSnapshot {
    pub id: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarDialogControlContextSnapshot {
    pub id: String,
    pub kind: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarDialogButtonContextSnapshot {
    pub id: String,
    pub text: String,
    pub enabled: bool,
}

impl SelectionContextSnapshot {
    pub fn none() -> Self {
        Self {
            available: false,
            kind: "none".to_string(),
            selected_commit_id: None,
            selected_file_path: None,
        }
    }
}

impl ToolbarDialogContextSnapshot {
    pub fn none() -> Self {
        Self {
            active: false,
            id: String::new(),
            owner: "none".to_string(),
            plugin_id: None,
            data: Vec::new(),
            controls: Vec::new(),
            buttons: Vec::new(),
        }
    }
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
                None,
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
    focus: Option<&FocusSnapshot>,
) -> Value {
    context_for_slot_with_selection(
        plugin_id,
        generation_id,
        address,
        repository,
        None,
        tabs,
        focus,
    )
}

pub fn context_for_slot_with_selection(
    plugin_id: &str,
    generation_id: GenerationId,
    address: &SlotAddress,
    repository: Option<&RepositoryContextSnapshot>,
    selection: Option<&SelectionContextSnapshot>,
    tabs: &TabsSnapshot,
    focus: Option<&FocusSnapshot>,
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
    context_for_surface_with_selection(
        plugin_id,
        generation_id,
        surface,
        repository,
        selection,
        tabs,
        focus,
    )
}

pub fn context_for_surface(
    plugin_id: &str,
    generation_id: GenerationId,
    surface: UiContextSurface,
    repository: Option<&RepositoryContextSnapshot>,
    tabs: &TabsSnapshot,
    focus: Option<&FocusSnapshot>,
) -> Value {
    context_for_surface_with_selection(
        plugin_id,
        generation_id,
        surface,
        repository,
        None,
        tabs,
        focus,
    )
}

pub fn context_for_surface_with_selection(
    plugin_id: &str,
    generation_id: GenerationId,
    surface: UiContextSurface,
    repository: Option<&RepositoryContextSnapshot>,
    selection: Option<&SelectionContextSnapshot>,
    tabs: &TabsSnapshot,
    focus: Option<&FocusSnapshot>,
) -> Value {
    context_for_surface_with_selection_and_dialog(
        plugin_id,
        generation_id,
        surface,
        repository,
        selection,
        None,
        tabs,
        focus,
    )
}

pub fn context_for_surface_with_selection_and_dialog(
    plugin_id: &str,
    generation_id: GenerationId,
    surface: UiContextSurface,
    repository: Option<&RepositoryContextSnapshot>,
    selection: Option<&SelectionContextSnapshot>,
    dialog: Option<&ToolbarDialogContextSnapshot>,
    tabs: &TabsSnapshot,
    focus: Option<&FocusSnapshot>,
) -> Value {
    let parts = surface_parts(surface);
    let focus_value = build_focus_value(&parts, focus);
    json!({
        "version": 1,
        "type": parts.context_type,
        "plugin_id": plugin_id,
        "generation_id": generation_id.get(),
        "surface": parts.surface_name,
        "features": features(),
        "theme": theme_tokens(),
        "repository": repository_summary(repository),
        "tab": tab_summary(tabs),
        "selection": selection_summary(selection),
        "dialog": dialog_summary(dialog),
        "focus": focus_value,
        "viewport": viewport_summary(),
        "payload": parts.payload,
    })
}

pub fn context_for_dock_panel(
    plugin_id: &str,
    generation_id: GenerationId,
    panel: &crate::plugin::dock::DockPanelSummary,
    repository: Option<&RepositoryContextSnapshot>,
    tabs: &TabsSnapshot,
    focus: Option<&FocusSnapshot>,
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
        focus,
    )
}

struct SurfaceParts {
    context_type: &'static str,
    surface_name: &'static str,
    rendered_kind: &'static str,
    rendered_region: Option<&'static str>,
    rendered_pane: Option<&'static str>,
    rendered_section: Option<String>,
    payload: Value,
}

fn surface_parts(surface: UiContextSurface) -> SurfaceParts {
    match surface {
        UiContextSurface::MainBar { section } => SurfaceParts {
            context_type: "MainBarContext",
            surface_name: "main_bar",
            rendered_kind: "global",
            rendered_region: Some("main_bar"),
            rendered_pane: None,
            rendered_section: Some(section.clone()),
            payload: json!({ "main_bar": { "section": section } }),
        },
        UiContextSurface::TabBar { section } => SurfaceParts {
            context_type: "TabBarContext",
            surface_name: "tab_bar",
            rendered_kind: "tab_bar",
            rendered_region: Some("tab_bar"),
            rendered_pane: None,
            rendered_section: Some(section.clone()),
            payload: json!({ "tab_bar": { "section": section } }),
        },
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
        UiContextSurface::Overlay => simple_surface("OverlayContext", "overlay", "overlay"),
        UiContextSurface::Screen => simple_surface("ScreenContext", "screen", "global"),
        UiContextSurface::StatusBar => simple_surface("StatusBarContext", "status_bar", "global"),
        UiContextSurface::RepositoryDiff => SurfaceParts {
            context_type: "RepositoryDiffContext",
            surface_name: "repository.diff",
            rendered_kind: "repository",
            rendered_region: Some("repository"),
            rendered_pane: Some("diff"),
            rendered_section: None,
            payload: json!({}),
        },
        UiContextSurface::RepositoryGraphRow => SurfaceParts {
            context_type: "RepositoryGraphRowContext",
            surface_name: "repository.graph.row",
            rendered_kind: "repository",
            rendered_region: Some("repository"),
            rendered_pane: Some("graph"),
            rendered_section: None,
            payload: json!({}),
        },
        UiContextSurface::RepositoryDiffLine => SurfaceParts {
            context_type: "RepositoryDiffLineContext",
            surface_name: "repository.diff.line",
            rendered_kind: "repository",
            rendered_region: Some("repository"),
            rendered_pane: Some("diff"),
            rendered_section: None,
            payload: json!({}),
        },
        UiContextSurface::Settings => simple_surface("SettingsContext", "settings", "global"),
        UiContextSurface::DockPanel {
            area,
            panel_id,
            title,
            open,
            state,
        } => SurfaceParts {
            context_type: "DockPanelContext",
            surface_name: "dock.panel",
            rendered_kind: "global",
            rendered_region: None,
            rendered_pane: None,
            rendered_section: None,
            payload: json!({ "dock": { "area": area, "panel_id": panel_id, "title": title, "open": open, "state": state } }),
        },
    }
}

fn repository_surface(
    context_type: &'static str,
    surface: &'static str,
    pane: &'static str,
    section: String,
) -> SurfaceParts {
    SurfaceParts {
        context_type,
        surface_name: surface,
        rendered_kind: "repository",
        rendered_region: Some("repository"),
        rendered_pane: Some(pane),
        rendered_section: Some(section.clone()),
        payload: json!({ "repository": { "pane": pane, "section": section } }),
    }
}

fn simple_surface(
    context_type: &'static str,
    surface: &'static str,
    kind: &'static str,
) -> SurfaceParts {
    SurfaceParts {
        context_type,
        surface_name: surface,
        rendered_kind: kind,
        rendered_region: None,
        rendered_pane: None,
        rendered_section: None,
        payload: json!({}),
    }
}

fn build_focus_value(parts: &SurfaceParts, focus: Option<&FocusSnapshot>) -> Value {
    let rendered_region = parts.rendered_region;
    let rendered_pane = parts.rendered_pane;
    let rendered_section = parts.rendered_section.clone();
    let rendered_surface = parts.surface_name;

    let mut obj = serde_json::Map::new();
    match focus {
        Some(snapshot) => {
            let active = project_active_focus(&snapshot.surface);
            let matches_surface = active.surface == rendered_surface;
            let matches_region = active.region == rendered_region;
            let matches_pane = active.pane == rendered_pane;
            obj.insert("surface".into(), Value::String(active.surface));
            obj.insert("kind".into(), Value::String(active.kind.to_string()));
            if let Some(region) = active.region {
                obj.insert("region".into(), Value::String(region.to_string()));
            }
            if let Some(pane) = active.pane {
                obj.insert("pane".into(), Value::String(pane.to_string()));
            }
            if let Some(plugin_id) = active.plugin_id {
                obj.insert("plugin_id".into(), Value::String(plugin_id));
            }
            if let Some(screen_id) = active.screen_id {
                obj.insert("screen_id".into(), Value::String(screen_id));
            }
            if let Some(overlay_id) = active.overlay_id {
                obj.insert("overlay_id".into(), Value::String(overlay_id));
            }
            obj.insert(
                "reason".into(),
                Value::String(snapshot.reason.as_str().to_string()),
            );
            obj.insert("matches_surface".into(), Value::Bool(matches_surface));
            obj.insert("matches_region".into(), Value::Bool(matches_region));
            obj.insert("matches_pane".into(), Value::Bool(matches_pane));
        }
        None => {
            obj.insert(
                "surface".into(),
                Value::String(rendered_surface.to_string()),
            );
            obj.insert(
                "kind".into(),
                Value::String(parts.rendered_kind.to_string()),
            );
            if let Some(region) = rendered_region {
                obj.insert("region".into(), Value::String(region.to_string()));
            }
            if let Some(pane) = rendered_pane {
                obj.insert("pane".into(), Value::String(pane.to_string()));
            }
            if let Some(section) = rendered_section {
                obj.insert("section".into(), Value::String(section));
            }
            obj.insert("matches_surface".into(), Value::Bool(true));
            obj.insert("matches_region".into(), Value::Bool(true));
            obj.insert("matches_pane".into(), Value::Bool(true));
        }
    }
    Value::Object(obj)
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

fn selection_summary(selection: Option<&SelectionContextSnapshot>) -> Value {
    match selection {
        Some(selection) => json!({
            "available": selection.available,
            "kind": selection.kind,
            "selected_commit_id": selection.selected_commit_id,
            "selected_file_path": selection.selected_file_path,
        }),
        None => {
            let none = SelectionContextSnapshot::none();
            json!({
                "available": none.available,
                "kind": none.kind,
                "selected_commit_id": none.selected_commit_id,
                "selected_file_path": none.selected_file_path,
            })
        }
    }
}

fn dialog_summary(dialog: Option<&ToolbarDialogContextSnapshot>) -> Value {
    match dialog {
        Some(dialog) => json!({
            "active": dialog.active,
            "id": dialog.id,
            "owner": dialog.owner,
            "plugin_id": dialog.plugin_id,
            "data": dialog
                .data
                .iter()
                .map(|item| json!({ "id": item.id, "value": item.value }))
                .collect::<Vec<_>>(),
            "controls": dialog
                .controls
                .iter()
                .map(|control| json!({
                    "id": control.id,
                    "kind": control.kind,
                    "value": control.value,
                }))
                .collect::<Vec<_>>(),
            "buttons": dialog
                .buttons
                .iter()
                .map(|button| json!({
                    "id": button.id,
                    "text": button.text,
                    "enabled": button.enabled,
                }))
                .collect::<Vec<_>>(),
        }),
        None => {
            let none = ToolbarDialogContextSnapshot::none();
            json!({
                "active": none.active,
                "id": none.id,
                "owner": none.owner,
                "plugin_id": none.plugin_id,
                "data": [],
                "controls": [],
                "buttons": [],
            })
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::ui::focus::{FocusReason, FocusSurface, RepositoryFocusSurface};

    fn build_ctx(surface: UiContextSurface, focus: Option<&FocusSnapshot>) -> Value {
        context_for_surface(
            "p",
            GenerationId::new(0),
            surface,
            None,
            &TabsSnapshot::default(),
            focus,
        )
    }

    #[test]
    fn focus_none_compat_mirrors_rendered_surface() {
        let ctx = build_ctx(
            UiContextSurface::RepositoryGraph {
                section: "bottom".into(),
            },
            None,
        );
        let focus = &ctx["focus"];
        assert_eq!(focus["surface"], "repository.graph");
        assert_eq!(focus["region"], "repository");
        assert_eq!(focus["pane"], "graph");
        assert_eq!(focus["section"], "bottom");
        assert_eq!(focus["matches_surface"], true);
        assert_eq!(focus["matches_region"], true);
        assert_eq!(focus["matches_pane"], true);
        assert!(focus.get("reason").is_none());
    }

    #[test]
    fn focus_active_graph_matches_graph_surface() {
        let snap = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
            reason: FocusReason::Keymap,
        };
        let ctx = build_ctx(
            UiContextSurface::RepositoryGraph {
                section: "bottom".into(),
            },
            Some(&snap),
        );
        let focus = &ctx["focus"];
        assert_eq!(focus["surface"], "repository.graph");
        assert_eq!(focus["kind"], "repository");
        assert_eq!(focus["region"], "repository");
        assert_eq!(focus["pane"], "graph");
        assert_eq!(focus["reason"], "keymap");
        assert_eq!(focus["matches_surface"], true);
        assert_eq!(focus["matches_region"], true);
        assert_eq!(focus["matches_pane"], true);
    }

    #[test]
    fn focus_active_details_does_not_match_graph_render() {
        let snap = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Details),
            reason: FocusReason::Keymap,
        };
        let ctx = build_ctx(
            UiContextSurface::RepositoryGraph {
                section: "bottom".into(),
            },
            Some(&snap),
        );
        let focus = &ctx["focus"];
        assert_eq!(focus["surface"], "repository.details");
        assert_eq!(focus["region"], "repository");
        assert_eq!(focus["pane"], "details");
        assert_eq!(focus["matches_surface"], false);
        assert_eq!(focus["matches_region"], true);
        assert_eq!(focus["matches_pane"], false);
    }

    #[test]
    fn focus_graph_row_inherits_active_focus_without_exact_surface_match() {
        let snap = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
            reason: FocusReason::Mouse,
        };
        let ctx = build_ctx(UiContextSurface::RepositoryGraphRow, Some(&snap));
        let focus = &ctx["focus"];
        assert_eq!(focus["surface"], "repository.graph");
        assert_eq!(focus["matches_surface"], false);
        assert_eq!(focus["matches_region"], true);
        assert_eq!(focus["matches_pane"], true);
    }

    #[test]
    fn focus_diff_line_inherits_active_focus_without_exact_surface_match() {
        let snap = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Diff),
            reason: FocusReason::DiffOpened,
        };
        let ctx = build_ctx(UiContextSurface::RepositoryDiffLine, Some(&snap));
        let focus = &ctx["focus"];
        assert_eq!(focus["surface"], "repository.diff");
        assert_eq!(focus["reason"], "diff_opened");
        assert_eq!(focus["matches_surface"], false);
        assert_eq!(focus["matches_region"], true);
        assert_eq!(focus["matches_pane"], true);
    }

    #[test]
    fn focus_plugin_screen_active_matches_screen_render() {
        let snap = FocusSnapshot {
            surface: FocusSurface::PluginScreen {
                plugin_id: "demo".into(),
                screen_id: "main".into(),
            },
            reason: FocusReason::PluginScreenChanged,
        };
        let ctx = build_ctx(UiContextSurface::Screen, Some(&snap));
        let focus = &ctx["focus"];
        assert_eq!(focus["surface"], "plugin_screen");
        assert_eq!(focus["kind"], "plugin_screen");
        assert_eq!(focus["plugin_id"], "demo");
        assert_eq!(focus["screen_id"], "main");
        assert_eq!(focus["reason"], "plugin_screen_changed");
    }

    #[test]
    fn focus_none_active_clears_match_helpers_for_specific_surface() {
        let snap = FocusSnapshot::default();
        let ctx = build_ctx(
            UiContextSurface::RepositoryGraph {
                section: "bottom".into(),
            },
            Some(&snap),
        );
        let focus = &ctx["focus"];
        assert_eq!(focus["surface"], "none");
        assert_eq!(focus["kind"], "none");
        assert_eq!(focus["reason"], "startup");
        assert_eq!(focus["matches_surface"], false);
        assert_eq!(focus["matches_region"], false);
        assert!(focus.get("region").is_none());
    }
}

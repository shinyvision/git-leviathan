#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusSurface {
    None,
    Global,
    TabBar,
    Repository(RepositoryFocusSurface),
    PluginScreen {
        plugin_id: String,
        screen_id: String,
    },
    Overlay {
        plugin_id: Option<String>,
        overlay_id: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryFocusSurface {
    Sidebar,
    Graph,
    Details,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusReason {
    Startup,
    Mouse,
    Keymap,
    TabChanged,
    DiffOpened,
    DiffClosed,
    OverlayOpened,
    OverlayClosed,
    PluginScreenChanged,
    RepositoryState,
}

impl FocusReason {
    pub fn as_str(self) -> &'static str {
        match self {
            FocusReason::Startup => "startup",
            FocusReason::Mouse => "mouse",
            FocusReason::Keymap => "keymap",
            FocusReason::TabChanged => "tab_changed",
            FocusReason::DiffOpened => "diff_opened",
            FocusReason::DiffClosed => "diff_closed",
            FocusReason::OverlayOpened => "overlay_opened",
            FocusReason::OverlayClosed => "overlay_closed",
            FocusReason::PluginScreenChanged => "plugin_screen_changed",
            FocusReason::RepositoryState => "repository_state",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusSnapshot {
    pub surface: FocusSurface,
    pub reason: FocusReason,
}

impl Default for FocusSnapshot {
    fn default() -> Self {
        Self {
            surface: FocusSurface::None,
            reason: FocusReason::Startup,
        }
    }
}

pub(crate) struct ActiveFocusProjection {
    pub surface: String,
    pub kind: &'static str,
    pub region: Option<&'static str>,
    pub pane: Option<&'static str>,
    pub plugin_id: Option<String>,
    pub screen_id: Option<String>,
    pub overlay_id: Option<String>,
}

pub(crate) fn project_active_focus(focus: &FocusSurface) -> ActiveFocusProjection {
    match focus {
        FocusSurface::None => ActiveFocusProjection {
            surface: "none".to_string(),
            kind: "none",
            region: None,
            pane: None,
            plugin_id: None,
            screen_id: None,
            overlay_id: None,
        },
        FocusSurface::Global => ActiveFocusProjection {
            surface: "global".to_string(),
            kind: "global",
            region: None,
            pane: None,
            plugin_id: None,
            screen_id: None,
            overlay_id: None,
        },
        FocusSurface::TabBar => ActiveFocusProjection {
            surface: "tab_bar".to_string(),
            kind: "tab_bar",
            region: Some("tab_bar"),
            pane: None,
            plugin_id: None,
            screen_id: None,
            overlay_id: None,
        },
        FocusSurface::Repository(repo) => {
            let (surface, pane) = match repo {
                RepositoryFocusSurface::Sidebar => ("repository.sidebar", "sidebar"),
                RepositoryFocusSurface::Graph => ("repository.graph", "graph"),
                RepositoryFocusSurface::Details => ("repository.details", "details"),
                RepositoryFocusSurface::Diff => ("repository.diff", "diff"),
            };
            ActiveFocusProjection {
                surface: surface.to_string(),
                kind: "repository",
                region: Some("repository"),
                pane: Some(pane),
                plugin_id: None,
                screen_id: None,
                overlay_id: None,
            }
        }
        FocusSurface::PluginScreen {
            plugin_id,
            screen_id,
        } => ActiveFocusProjection {
            surface: "plugin_screen".to_string(),
            kind: "plugin_screen",
            region: None,
            pane: None,
            plugin_id: Some(plugin_id.clone()),
            screen_id: Some(screen_id.clone()),
            overlay_id: None,
        },
        FocusSurface::Overlay {
            plugin_id,
            overlay_id,
        } => ActiveFocusProjection {
            surface: "overlay".to_string(),
            kind: "overlay",
            region: None,
            pane: None,
            plugin_id: plugin_id.clone(),
            screen_id: None,
            overlay_id: overlay_id.clone(),
        },
    }
}

fn projection_to_object(
    projection: &ActiveFocusProjection,
) -> serde_json::Map<String, serde_json::Value> {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "surface".into(),
        serde_json::Value::String(projection.surface.clone()),
    );
    obj.insert(
        "kind".into(),
        serde_json::Value::String(projection.kind.to_string()),
    );
    if let Some(region) = projection.region {
        obj.insert(
            "region".into(),
            serde_json::Value::String(region.to_string()),
        );
    }
    if let Some(pane) = projection.pane {
        obj.insert("pane".into(), serde_json::Value::String(pane.to_string()));
    }
    if let Some(plugin_id) = projection.plugin_id.as_ref() {
        obj.insert(
            "plugin_id".into(),
            serde_json::Value::String(plugin_id.clone()),
        );
    }
    if let Some(screen_id) = projection.screen_id.as_ref() {
        obj.insert(
            "screen_id".into(),
            serde_json::Value::String(screen_id.clone()),
        );
    }
    if let Some(overlay_id) = projection.overlay_id.as_ref() {
        obj.insert(
            "overlay_id".into(),
            serde_json::Value::String(overlay_id.clone()),
        );
    }
    obj
}

pub(crate) fn focus_event_payload(
    prev: &FocusSnapshot,
    next: &FocusSnapshot,
) -> serde_json::Map<String, serde_json::Value> {
    let old_proj = project_active_focus(&prev.surface);
    let new_proj = project_active_focus(&next.surface);
    let mut payload = serde_json::Map::new();
    payload.insert(
        "old_surface".into(),
        serde_json::Value::String(old_proj.surface.clone()),
    );
    payload.insert(
        "new_surface".into(),
        serde_json::Value::String(new_proj.surface.clone()),
    );
    if !matches!(prev.surface, FocusSurface::None) {
        payload.insert(
            "old".into(),
            serde_json::Value::Object(projection_to_object(&old_proj)),
        );
    }
    if !matches!(next.surface, FocusSurface::None) {
        payload.insert(
            "new".into(),
            serde_json::Value::Object(projection_to_object(&new_proj)),
        );
    }
    payload.insert(
        "reason".into(),
        serde_json::Value::String(next.reason.as_str().to_string()),
    );
    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginHost;

    #[test]
    fn default_snapshot_is_none_startup() {
        let snap = FocusSnapshot::default();
        assert_eq!(snap.surface, FocusSurface::None);
        assert_eq!(snap.reason, FocusReason::Startup);
    }

    #[test]
    fn repository_focus_surface_eq() {
        assert_eq!(RepositoryFocusSurface::Graph, RepositoryFocusSurface::Graph);
        assert_ne!(RepositoryFocusSurface::Graph, RepositoryFocusSurface::Diff);
    }

    #[test]
    fn focus_surface_repository_variant_eq() {
        let a = FocusSurface::Repository(RepositoryFocusSurface::Sidebar);
        let b = FocusSurface::Repository(RepositoryFocusSurface::Sidebar);
        let c = FocusSurface::Repository(RepositoryFocusSurface::Details);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn plugin_screen_variant_eq() {
        let a = FocusSurface::PluginScreen {
            plugin_id: "p".into(),
            screen_id: "s".into(),
        };
        let b = FocusSurface::PluginScreen {
            plugin_id: "p".into(),
            screen_id: "s".into(),
        };
        let c = FocusSurface::PluginScreen {
            plugin_id: "p".into(),
            screen_id: "t".into(),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn sync_focus_first_set_returns_true() {
        let mut host = PluginHost::new();
        let snap = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
            reason: FocusReason::Keymap,
        };
        assert!(host.sync_focus(snap));
    }

    #[test]
    fn sync_focus_identical_resync_returns_false() {
        let mut host = PluginHost::new();
        let snap = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
            reason: FocusReason::Keymap,
        };
        assert!(host.sync_focus(snap.clone()));
        assert!(!host.sync_focus(snap));
    }

    #[test]
    fn sync_focus_after_change_returns_true() {
        let mut host = PluginHost::new();
        let first = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
            reason: FocusReason::Keymap,
        };
        let second = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Details),
            reason: FocusReason::Keymap,
        };
        assert!(host.sync_focus(first));
        assert!(host.sync_focus(second));
    }

    #[test]
    fn focus_event_payload_has_expected_top_level_strings() {
        let prev = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
            reason: FocusReason::Startup,
        };
        let next = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Details),
            reason: FocusReason::Keymap,
        };
        let payload = focus_event_payload(&prev, &next);
        assert_eq!(
            payload.get("old_surface").and_then(|v| v.as_str()),
            Some("repository.graph")
        );
        assert_eq!(
            payload.get("new_surface").and_then(|v| v.as_str()),
            Some("repository.details")
        );
        assert_eq!(
            payload.get("reason").and_then(|v| v.as_str()),
            Some("keymap")
        );
    }

    #[test]
    fn focus_event_payload_includes_old_and_new_projection_objects() {
        let prev = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
            reason: FocusReason::Startup,
        };
        let next = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Details),
            reason: FocusReason::Keymap,
        };
        let payload = focus_event_payload(&prev, &next);
        let old = payload.get("old").and_then(|v| v.as_object()).unwrap();
        assert_eq!(
            old.get("surface").and_then(|v| v.as_str()),
            Some("repository.graph")
        );
        assert_eq!(old.get("kind").and_then(|v| v.as_str()), Some("repository"));
        assert_eq!(old.get("pane").and_then(|v| v.as_str()), Some("graph"));
        assert_eq!(
            old.get("region").and_then(|v| v.as_str()),
            Some("repository")
        );
        let new = payload.get("new").and_then(|v| v.as_object()).unwrap();
        assert_eq!(
            new.get("surface").and_then(|v| v.as_str()),
            Some("repository.details")
        );
        assert_eq!(new.get("pane").and_then(|v| v.as_str()), Some("details"));
    }

    #[test]
    fn focus_event_payload_omits_old_when_previous_is_none() {
        let prev = FocusSnapshot::default();
        let next = FocusSnapshot {
            surface: FocusSurface::Repository(RepositoryFocusSurface::Graph),
            reason: FocusReason::Startup,
        };
        let payload = focus_event_payload(&prev, &next);
        assert_eq!(
            payload.get("old_surface").and_then(|v| v.as_str()),
            Some("none")
        );
        assert!(!payload.contains_key("old"));
        assert!(payload.contains_key("new"));
    }
}

use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UiDependency {
    PluginState,
    Repository,
    Tab,
    Selection,
    Diff,
    Theme,
    Layout,
    Focus,
}

impl UiDependency {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PluginState => "plugin_state",
            Self::Repository => "repository",
            Self::Tab => "tab",
            Self::Selection => "selection",
            Self::Diff => "diff",
            Self::Theme => "theme",
            Self::Layout => "layout",
            Self::Focus => "focus",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "plugin_state" | "plugin" | "state" => Some(Self::PluginState),
            "repository" => Some(Self::Repository),
            "tab" | "tabs" => Some(Self::Tab),
            "selection" => Some(Self::Selection),
            "diff" | "diff_loaded" => Some(Self::Diff),
            "theme" => Some(Self::Theme),
            "layout" => Some(Self::Layout),
            "focus" => Some(Self::Focus),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiInvalidationCause {
    InitialLoad,
    PluginStateChanged,
    RepositoryChanged,
    TabChanged,
    SelectionChanged,
    DiffLoaded,
    ThemeChanged,
    LayoutChanged,
    FocusChanged,
    TimerCallbackChangedPluginState,
    JobCallbackChangedPluginState,
    WatchCallbackChangedPluginState,
}

impl UiInvalidationCause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InitialLoad => "initial_load",
            Self::PluginStateChanged => "plugin_state_changed",
            Self::RepositoryChanged => "repository_changed",
            Self::TabChanged => "tab_changed",
            Self::SelectionChanged => "selection_changed",
            Self::DiffLoaded => "diff_loaded",
            Self::ThemeChanged => "theme_changed",
            Self::LayoutChanged => "layout_changed",
            Self::FocusChanged => "focus_changed",
            Self::TimerCallbackChangedPluginState => "timer_callback_changed_plugin_state",
            Self::JobCallbackChangedPluginState => "job_callback_changed_plugin_state",
            Self::WatchCallbackChangedPluginState => "watch_callback_changed_plugin_state",
        }
    }

    pub fn dependency(self) -> Option<UiDependency> {
        match self {
            Self::InitialLoad => None,
            Self::PluginStateChanged
            | Self::TimerCallbackChangedPluginState
            | Self::JobCallbackChangedPluginState
            | Self::WatchCallbackChangedPluginState => Some(UiDependency::PluginState),
            Self::RepositoryChanged => Some(UiDependency::Repository),
            Self::TabChanged => Some(UiDependency::Tab),
            Self::SelectionChanged => Some(UiDependency::Selection),
            Self::DiffLoaded => Some(UiDependency::Diff),
            Self::ThemeChanged => Some(UiDependency::Theme),
            Self::LayoutChanged => Some(UiDependency::Layout),
            Self::FocusChanged => Some(UiDependency::Focus),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DynamicWidgetTelemetry {
    pub refresh_count: u64,
    pub skipped_count: u64,
    pub stale_count: u64,
    pub last_duration_ms: u128,
    pub last_causes: Vec<String>,
    pub last_status: String,
    pub last_error: Option<String>,
    pub diagnostic_badge: bool,
}

impl Default for DynamicWidgetTelemetry {
    fn default() -> Self {
        Self {
            refresh_count: 0,
            skipped_count: 0,
            stale_count: 0,
            last_duration_ms: 0,
            last_causes: Vec::new(),
            last_status: "never".to_string(),
            last_error: None,
            diagnostic_badge: false,
        }
    }
}

pub fn default_dependencies_for_region(region: &str) -> Vec<UiDependency> {
    match region {
        "tab_bar" => vec![
            UiDependency::PluginState,
            UiDependency::Repository,
            UiDependency::Tab,
            UiDependency::Theme,
        ],
        "repository" => vec![
            UiDependency::PluginState,
            UiDependency::Repository,
            UiDependency::Selection,
            UiDependency::Diff,
            UiDependency::Theme,
            UiDependency::Layout,
            UiDependency::Focus,
        ],
        _ => vec![
            UiDependency::PluginState,
            UiDependency::Repository,
            UiDependency::Tab,
            UiDependency::Theme,
            UiDependency::Focus,
        ],
    }
}

pub fn dependencies_match(dependencies: &[UiDependency], causes: &[UiInvalidationCause]) -> bool {
    if causes
        .iter()
        .any(|cause| matches!(cause, UiInvalidationCause::InitialLoad))
    {
        return true;
    }
    let deps: HashSet<UiDependency> = dependencies.iter().copied().collect();
    causes
        .iter()
        .filter_map(|cause| cause.dependency())
        .any(|dep| deps.contains(&dep))
}

pub fn cause_labels(causes: &[UiInvalidationCause]) -> Vec<String> {
    causes
        .iter()
        .map(|cause| cause.as_str().to_string())
        .collect()
}

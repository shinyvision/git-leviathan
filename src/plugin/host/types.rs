use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;

use git_leviathan_plugin_api::manifest::PluginManifest;
use mlua::{Lua, RegistryKey};

use crate::plugin::api::git::PendingGitEvents;
use crate::plugin::api::{
    async_api::JobCallbacks, DeferredQueue, DockPanelDef, HealthCheckRegistration, ScreenDef,
    SettingsPanelDef, UserCommands,
};
use crate::plugin::async_jobs::AsyncJobRegistry;
use crate::plugin::audit::AuditLog;
use crate::plugin::capability_grants::{AutoGrantPolicy, GrantStore};
use crate::plugin::commands::{CommandPluginRegistry, CommandRegistry, PendingCommandEvents};
use crate::plugin::core_commands::CoreCommandActions;
use crate::plugin::devtools::ReloadEventSummary;
use crate::plugin::diagnostic::DiagnosticStore;
use crate::plugin::dock::DockManager;
use crate::plugin::events::EventBus;
use crate::plugin::extensions::{DecorationProviderCallbacks, ExtensionRegistry, OverlayCallbacks};
use crate::plugin::generation::PluginGeneration;
use crate::plugin::git_ops::{ActiveRepositoryGateway, DestructiveConfirmPolicy, PendingGitWrites};
use crate::plugin::keymap::KeymapRegistry;
use crate::plugin::lua_loader::LuaLoader;
use crate::plugin::navigation::PluginNavigationEffect;
use crate::plugin::performance::BudgetTracker;
use crate::plugin::resources::ResourceLedger;
use crate::plugin::runtime_path::RuntimePathRegistry;
use crate::plugin::services::ServiceRegistry;
use crate::plugin::slots::SlotAddress;
use crate::plugin::storage::PluginStorageRoots;
use crate::plugin::tab_snapshot::{TabRegistryOp, TabsSnapshot};
use crate::plugin::timers::{PluginTimerCallbacks, TimerRegistry};
use crate::plugin::ui::chrome::ChromeRegistration;
use crate::plugin::ui::context::UiContextStore;
use crate::plugin::ui::contribution_overrides::ContributionOverrides;
use crate::plugin::ui::focus::FocusSnapshot;
use crate::plugin::ui::main_bar_slots::{DynamicWidgetRegistration, PreparedSlotOp};
use crate::plugin::ui::widget_ast::WidgetAst;
use crate::plugin::watchers::{FileWatcherRegistry, PluginWatcherCallbacks};
use crate::plugin::{activation, dependency, devtools_commands};

#[derive(Debug)]
pub enum PluginLoadError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    Lua(mlua::Error),
    BadManifest(String),
    Plugin(git_leviathan_plugin_api::error::PluginError),
    Disabled(String),
}

impl From<std::io::Error> for PluginLoadError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<toml::de::Error> for PluginLoadError {
    fn from(e: toml::de::Error) -> Self {
        Self::Toml(e)
    }
}

impl From<mlua::Error> for PluginLoadError {
    fn from(e: mlua::Error) -> Self {
        Self::Lua(e)
    }
}

impl std::fmt::Display for PluginLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Toml(e) => write!(f, "toml: {e}"),
            Self::Lua(e) => write!(f, "lua: {e}"),
            Self::BadManifest(m) => write!(f, "bad manifest: {m}"),
            Self::Plugin(e) => write!(f, "{e}"),
            Self::Disabled(m) => write!(f, "disabled: {m}"),
        }
    }
}

impl std::error::Error for PluginLoadError {}

pub(super) struct LoadedPlugin {
    pub(super) generation: PluginGeneration,
    pub(super) root: PathBuf,
    pub(super) manifest: PluginManifest,
    pub(super) slot_handlers: HashMap<SlotAddress, RegistryKey>,
    pub(super) overlay_callbacks: Rc<RefCell<OverlayCallbacks>>,
    pub(super) dialog_callbacks: Rc<RefCell<crate::plugin::ui::dialog::DialogCallbacks>>,
    pub(super) decoration_provider_callbacks: Rc<RefCell<DecorationProviderCallbacks>>,
    pub(super) screens: HashMap<String, ScreenDef>,
    pub(super) dock_panels: HashMap<String, DockPanelDef>,
    pub(super) settings_panel: Option<SettingsPanelDef>,
    pub(super) screen_state: HashMap<String, RegistryKey>,
    pub(super) dynamic_widgets: HashMap<SlotAddress, DynamicWidgetRegistration>,
    pub(super) chrome_widgets: Rc<RefCell<HashMap<String, ChromeRegistration>>>,
    pub(super) ui_context: UiContextStore,
    pub(super) deferred: Rc<RefCell<DeferredQueue>>,
    pub(super) user_commands: Rc<RefCell<UserCommands>>,
    pub(super) health_checks: Vec<HealthCheckRegistration>,
    pub(super) lua_loader: LuaLoader,
    pub(super) job_callbacks: Rc<RefCell<JobCallbacks>>,
    pub(super) timer_callbacks: Rc<RefCell<PluginTimerCallbacks>>,
    pub(super) watcher_callbacks: Rc<RefCell<PluginWatcherCallbacks>>,
}

impl LoadedPlugin {
    pub(super) fn id(&self) -> &str {
        self.generation.plugin_id.as_str()
    }

    pub(super) fn lua(&self) -> &Lua {
        &self.generation.lua
    }

    pub(super) fn lua_rc(&self) -> Rc<Lua> {
        Rc::clone(&self.generation.lua)
    }

    pub(super) fn ledger(&self) -> ResourceLedger {
        self.generation.ledger.clone()
    }
}

pub(super) struct SplitDragInfo {
    pub(super) split_key: String,
    pub(super) divider_index: usize,
    pub(super) initial_sizes: Vec<f32>,
    pub(super) initial_pointer: Option<f32>,
    pub(super) is_vertical: bool,
    pub(super) limits: Vec<(f32, f32)>,
}

pub struct PluginHost {
    pub(super) plugins: HashMap<String, LoadedPlugin>,
    pub(super) slot_ops: Vec<PreparedSlotOp>,
    pub(super) slot_ops_revision: u64,
    pub(super) active_screen: Option<(String, String)>,
    pub(super) widget_tree: Option<WidgetAst>,
    pub(super) split_sizes: HashMap<String, Vec<f32>>,
    pub(super) split_drag: Option<SplitDragInfo>,
    pub(super) event_bus: EventBus,
    pub(super) last_repository_hash: Option<u64>,
    pub(super) last_tab_snapshot: TabsSnapshot,
    pub(super) pending_tab_ops: Rc<RefCell<Vec<TabRegistryOp>>>,
    pub(super) audit_log: AuditLog,
    pub(super) last_reload_errors: HashMap<String, String>,
    pub(super) service_registry: Rc<RefCell<ServiceRegistry>>,
    pub(super) next_generation_ids: HashMap<String, u64>,
    pub(super) diagnostics: DiagnosticStore,
    pub(super) runtime_path_registry: RuntimePathRegistry,
    pub(super) reload_history: HashMap<String, VecDeque<ReloadEventSummary>>,
    pub(super) command_registry: Rc<RefCell<CommandRegistry>>,
    pub(super) command_plugin_registry: CommandPluginRegistry,
    pub(super) pending_command_events: PendingCommandEvents,
    pub(super) command_active_context: Rc<RefCell<serde_json::Value>>,
    pub(super) keymap_registry: Rc<RefCell<KeymapRegistry>>,
    pub(super) grant_store: GrantStore,
    pub(super) auto_grant_policy: AutoGrantPolicy,
    pub(super) active_gateway: ActiveRepositoryGateway,
    pub(super) destructive_policy: DestructiveConfirmPolicy,
    pub(super) pending_git_writes: PendingGitWrites,
    pub(super) pending_git_events: PendingGitEvents,
    pub(super) pending_ui_effects: crate::plugin::ui::effects::PendingUiEffects,
    pub(super) async_jobs: AsyncJobRegistry,
    pub(super) timers: TimerRegistry,
    pub(super) watchers: FileWatcherRegistry,
    pub(super) storage_roots: PluginStorageRoots,
    pub(super) dependency_graph: Vec<dependency::DependencySummary>,
    pub(super) lazy_registry: activation::LazyRegistry,
    pub(super) lazy_ledgers: HashMap<String, ResourceLedger>,
    pub(super) last_repository_shape: Option<RepositoryShapeFacts>,
    pub(super) last_selection_snapshot: crate::plugin::ui::context::SelectionContextSnapshot,
    pub(super) last_toolbar_dialog_snapshot:
        crate::plugin::ui::context::ToolbarDialogContextSnapshot,
    pub(super) extension_registry: ExtensionRegistry,
    pub(super) dock_manager: DockManager,
    pub(super) contribution_overrides: ContributionOverrides,
    pub(super) budget_tracker: BudgetTracker,
    pub(super) disabled_plugins: HashSet<String>,
    pub(super) last_plugin_roots: HashMap<String, PathBuf>,
    pub(super) devtools_action_queue: devtools_commands::DevtoolsActionQueue,
    pub(super) last_devtools_result: Option<serde_json::Value>,
    pub(super) core_command_actions: CoreCommandActions,
    pub(super) pending_navigation_effects: Vec<PluginNavigationEffect>,
    pub(super) last_focus_snapshot: FocusSnapshot,
    /// Monotonic counter bumped whenever the host enters Lua through a
    /// path that can mutate plugin screen state without the app's own
    /// dirty tracking noticing: autocmd/event dispatch, command
    /// invocation, and timer / async-job / watcher / deferred callbacks.
    /// `App` compares this against its last-seen value to know whether
    /// plugin-tab state may have changed and needs re-persisting.
    pub(super) lua_activity: Cell<u64>,
}

/// Cached repository facts evaluated against lazy activation predicates.
#[derive(Debug, Clone)]
pub(super) struct RepositoryShapeFacts {
    pub(super) repo_name: String,
    pub(super) current_branch: String,
    pub(super) head_hash: String,
    pub(super) default_remote: String,
    pub(super) has_remote: bool,
    pub(super) workdir: PathBuf,
}

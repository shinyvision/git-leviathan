//! Plugin host — loads plugins, owns their Lua states, dispatches callbacks.
//!
//! Design:
//! - Each plugin gets its own `mlua::Lua` state. All calls happen on the main
//!   thread from `App::update`.
//! - During `init.lua` execution, a `BuildState` collected via `Rc<RefCell<_>>`
//!   captures registrations (main-bar buttons, screens). After `exec()`, the
//!   host drains the BuildState into its permanent `LoadedPlugin`.
//! - Runtime effects flow through callback return values: screen `update`
//!   returns `{ state = ..., navigate = ... }` tables. The host interprets.
//! - Widget trees are cached as a typed `Option<WidgetAst>` and
//!   refreshed after every dispatch (open_screen / dispatch_event).
//!   The boundary decoder converts the plugin's Lua return into the AST
//!   once, so the renderer never sees a raw JSON value.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use git_leviathan_plugin_api::api_version::HOST_API_VERSION;
use git_leviathan_plugin_api::descriptor::api as event_descriptor;
use git_leviathan_plugin_api::manifest::PluginManifest;
use mlua::{
    Function, Lua, LuaSerdeExt, RegistryKey, Table, Thread, ThreadStatus, Value as LuaValue,
};

use crate::plugin::api::health::{HealthContext, HealthItem, HealthReport, PluginHealth};
use crate::plugin::api::{
    self, async_api::JobCallbacks, AsyncRuntimeContext, BuildState, DeferredCallback,
    DeferredQueue, HealthCheckRegistration, PersistContext, RawSlotOp, RawSlotSpec, ScreenDef,
    ServicesContext, UserCommands, WidgetSource,
};
use crate::plugin::async_jobs::{AsyncJobRegistry, JobOutcome};
use crate::plugin::audit::AuditLog;
use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::capability_grants::{
    audit_grant_event, canonicalize_requested, prompt_for_undecided, unknown_requested,
    AutoGrantPolicy, CapabilityGrantSummary, GrantStore, PendingPromptSummary,
};
use crate::plugin::commands::{
    self, CommandBody, CommandContext, CommandDescriptor, CommandDispatchEnv, CommandPluginContext,
    CommandPluginRegistry, CommandRegistry, InvokeOutcome, PendingCommandEvents, RawCommand,
    HOST_COMMAND_PLUGIN_ID,
};
use crate::plugin::devtools::{ReloadEventSummary, ReloadOutcome};
use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::events::{
    self, AutocmdOptions, DispatchOutcome, EventBus, EventPayload, GroupId,
    MAX_CONSECUTIVE_FAILURES,
};
use crate::plugin::generation::PluginGeneration;
use crate::plugin::git_ops::{
    ActiveRepositoryGateway, DestructiveConfirmPolicy, GitOpsContext, PendingGitWrite,
    PendingGitWrites,
};
use crate::plugin::keymap::{self as keymap_mod, KeymapDispatchOutcome, KeymapRegistry, Keystroke};
use crate::plugin::lua_loader::{install_runtime_module, LuaLoader};
use crate::plugin::performance::{BudgetTracker, CallbackKind, Outcome as PerfOutcome};
use crate::plugin::resources::{
    GenerationId, PluginId, PluginResourceKind, ResourceCleaner, ResourceLedger, ResourceRecord,
};
use crate::plugin::runtime_path::{PluginRuntimePath, RuntimePathRegistry};
use crate::plugin::services::{dependency_statuses, ServiceRegistry};
use crate::plugin::slots::{IsSlot, SlotRegistry};
use crate::plugin::staged_reload::{
    self, snapshot_previous_screens, stage_reload, ReloadStage, StageInputs, StagingArtifacts,
    StagingFailure,
};
use crate::plugin::storage::{PluginStoragePaths, PluginStorageRoots, StorageSurface};
use crate::plugin::tab_snapshot::{TabChange, TabRegistryOp, TabsSnapshot};
use crate::plugin::timers::{PluginTimerCallbacks, TimerRegistry};
use crate::plugin::ui::main_bar_slots::{
    parse_container, DynamicAstCache, PreparedSlot, PreparedSlotOp, SlotWidget,
};
use crate::plugin::ui::split;
use crate::plugin::ui::widget_ast::{self, WidgetAst};
use crate::plugin::watchers::{FileWatcherRegistry, PluginWatcherCallbacks};
use crate::services::RepoRef;
use crate::widgets::chrome::main_bar::MainBarRegistry;
use crate::widgets::chrome::repo_region::RepoRegionRegistry;
use crate::widgets::chrome::tab_bar_slots::TabBarRegistry;

#[derive(Debug)]
pub enum PluginLoadError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    Lua(mlua::Error),
    BadManifest(String),
    Plugin(git_leviathan_plugin_api::error::PluginError),
    /// Phase 19: the plugin id is on the host's
    /// disabled-plugins set. Surfaced from
    /// [`PluginHost::load_plugin`] when a `Plugin: Disable`
    /// devtools command (or test fixture) marked the plugin as
    /// disabled before this load.
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

struct LoadedPlugin {
    generation: PluginGeneration,
    /// Absolute path to the plugin's directory. Used as sandbox root when
    /// resolving plugin-bundled assets (icons, etc).
    root: PathBuf,
    /// Parsed manifest stashed at load time. Read by `introspect()` to
    /// surface plugin metadata (name, version, capabilities, declared
    /// services) without re-parsing `plugin.toml` on every devtools open.
    manifest: PluginManifest,
    /// `main_bar.add` / `main_bar.replace` handlers. Keyed by `slot_id`
    /// (the full registry id the plugin declared).
    slot_handlers: HashMap<String, RegistryKey>,
    screens: HashMap<String, ScreenDef>,
    screen_state: HashMap<String, RegistryKey>,
    /// Dynamic (function-backed) main-bar slot widgets. Each entry is the
    /// Lua function registry key plus the shared cache cell the slot's
    /// builder reads from. Populated on load; refreshed after every
    /// autocmd fire affecting this plugin.
    dynamic_widgets: HashMap<String, (RegistryKey, DynamicAstCache)>,
    /// Per-plugin deferred-call queue. `leviathan.api.schedule(fn)` and
    /// `defer_fn(ms, fn)` push into this; `PluginHost::tick` drains it.
    /// Coroutines that yielded mid-resume also live here so subsequent
    /// ticks can resume them.
    deferred: Rc<RefCell<DeferredQueue>>,
    /// Plugin-registered named commands (`leviathan.command.create`).
    /// Looked up by `PluginHost::invoke_user_command`.
    user_commands: Rc<RefCell<UserCommands>>,
    /// Health-check callbacks registered via `leviathan.health.register`.
    /// Drained per-plugin by `PluginHost::run_health_checks`. Plugins
    /// without any registration are absent from the resulting report.
    health_checks: Vec<HealthCheckRegistration>,
    /// Phase 5 Lua loader. Owns the per-generation module cache and
    /// the resolved runtime path. Held here so `module_graph()` and
    /// `runtime_paths` introspection can read live state, and so the
    /// cache drops when this `LoadedPlugin` (and its generation) is
    /// dropped.
    lua_loader: LuaLoader,
    /// Phase 12 per-plugin async-job on-complete callbacks. Keyed by
    /// `JobId`; consumed by `tick()` when the worker thread finishes.
    job_callbacks: Rc<RefCell<JobCallbacks>>,
    /// Phase 12 per-plugin timer callbacks. Keyed by `TimerId`; the
    /// timer registry owns lifecycle, this map owns the Lua refs.
    timer_callbacks: Rc<RefCell<PluginTimerCallbacks>>,
    /// Phase 12 per-plugin file-watcher callbacks. Keyed by
    /// `WatchId`. Drained when the watcher fires or is cancelled.
    watcher_callbacks: Rc<RefCell<PluginWatcherCallbacks>>,
}

impl LoadedPlugin {
    fn id(&self) -> &str {
        self.generation.plugin_id.as_str()
    }

    fn lua(&self) -> &Lua {
        &self.generation.lua
    }

    fn lua_rc(&self) -> Rc<Lua> {
        Rc::clone(&self.generation.lua)
    }

    fn ledger(&self) -> ResourceLedger {
        self.generation.ledger.clone()
    }
}

struct SplitDragInfo {
    split_key: String,
    divider_index: usize,
    initial_sizes: Vec<f32>,
    initial_pointer: Option<f32>,
    is_vertical: bool,
    limits: Vec<(f32, f32)>,
}

struct HostResourceCleaner<'a> {
    slot_ops: &'a mut Vec<PreparedSlotOp>,
    event_bus: &'a mut EventBus,
    service_registry: Rc<RefCell<ServiceRegistry>>,
}

impl ResourceCleaner for HostResourceCleaner<'_> {
    fn cleanup_resource(&mut self, resource: &ResourceRecord) -> Result<(), String> {
        let plugin_id = resource.plugin_id.as_str();
        match resource.kind {
            PluginResourceKind::Slot => {
                self.slot_ops
                    .retain(|op| !op_matches_slot_resource(op, plugin_id, &resource.handle));
            }
            PluginResourceKind::Autocmd => {
                // Each autocmd resource record corresponds to one
                // EventBus row keyed by (plugin_id, generation_id,
                // event-name = resource.handle). Drop the first
                // match so multi-row registrations (one per event in
                // a string array) reap one-for-one.
                let entries = self.event_bus.entries_mut();
                if let Some(idx) = entries.iter().position(|entry| {
                    entry.plugin_id == plugin_id
                        && entry.generation_id == resource.generation_id
                        && entry.event == resource.handle.as_str()
                }) {
                    entries.remove(idx);
                }
            }
            PluginResourceKind::ServiceRegistration => {
                self.service_registry
                    .borrow_mut()
                    .unregister(&resource.handle, plugin_id);
            }
            _ => {}
        }
        Ok(())
    }
}

pub struct PluginHost {
    plugins: HashMap<String, LoadedPlugin>,
    /// Ordered hook operations across every region.
    slot_ops: Vec<PreparedSlotOp>,
    active_screen: Option<(String, String)>,
    widget_tree: Option<WidgetAst>,
    split_sizes: HashMap<String, Vec<f32>>,
    split_drag: Option<SplitDragInfo>,
    /// Phase 7 typed-event registry. Owns every autocmd row and the
    /// host-side virtual clock used for debounce. Replaces the
    /// pre-Phase-7 `HashMap<event, Vec<(plugin, key)>>` table.
    event_bus: EventBus,
    /// Hash of the last `(current_branch, refs)` pushed to every plugin's
    /// `leviathan.repository`. `None` means "not yet synced" — the first
    /// `sync_repository` call always pushes. Used to suppress redundant
    /// table rebuilds + `BranchChanged` fires on unchanged snapshots.
    last_repository_hash: Option<u64>,
    /// Last `TabsSnapshot` pushed into every plugin's `leviathan.tab_registry`.
    /// Diffed against fresh snapshots in `sync_tab_registry` to elide
    /// redundant Lua syncs and to compute the precise `TabChange` returned
    /// to the app for event dispatch. Default-initialized so the first
    /// non-empty snapshot is treated as a change.
    last_tab_snapshot: TabsSnapshot,
    /// Shared between every plugin's Lua state. `tab_registry.{add,
    /// remove, select}` push into this; `App::update` drains via
    /// `take_pending_tab_ops`.
    pending_tab_ops: Rc<RefCell<Vec<TabRegistryOp>>>,
    /// Per-host capability audit log. Cloned (cheap, Arc-backed) into
    /// every plugin's `CapabilityGuard` so allow/deny events from all
    /// plugins land in the same log. Read by the devtools panel
    /// (Phase 6).
    audit_log: AuditLog,
    /// Last failed `reload_plugin` error per plugin. Cleared on a
    /// successful reload. Devtools surfaces this so a hot-reload
    /// failure is visible without scraping stderr.
    last_reload_errors: HashMap<String, String>,
    /// Inter-plugin service registry. Populated by
    /// `leviathan.services.register` calls from each plugin's init.lua;
    /// queried by `leviathan.services.get`.
    service_registry: Rc<RefCell<ServiceRegistry>>,
    next_generation_ids: HashMap<String, u64>,
    /// Phase 3 typed diagnostics. Every host-side error path that used
    /// to `eprintln!` records a `PluginDiagnostic` here. Cheap-clone
    /// (Arc-backed); shared into the devtools snapshot.
    diagnostics: DiagnosticStore,
    /// Phase 5 host-wide map of `plugin_id -> root directory`. Read by
    /// `PluginRuntimePath::resolve` so a plugin's declared
    /// `requires_plugins` resolve to absolute `lua/<dep_id>/` roots.
    /// Cheap-clone (Rc-backed); kept on the host so reload/unload can
    /// keep it in sync with the live plugin set.
    runtime_path_registry: RuntimePathRegistry,
    /// Phase 6 reload-history per plugin, capped at the most recent
    /// `RELOAD_HISTORY_CAP` entries. Drained into the devtools
    /// `InspectorSnapshot::reload_history` view (oldest first).
    reload_history: HashMap<String, VecDeque<ReloadEventSummary>>,
    /// Phase 8 unified command registry. Holds host-side built-in
    /// commands (registered at construction with
    /// [`HOST_COMMAND_PLUGIN_ID`]) and every plugin-registered Lua
    /// command. Dispatch flows through
    /// [`commands::dispatch_command`] so the Lua API
    /// `leviathan.command.invoke` and the Rust entry
    /// [`PluginHost::invoke_command`] resolve the same registry.
    command_registry: Rc<RefCell<CommandRegistry>>,
    /// Per-plugin Lua state + capability guard handles consulted by
    /// the command dispatcher. Updated on load / reload-commit /
    /// unload so the registry can resolve a command's owner without
    /// borrowing `&mut self` at dispatch time.
    command_plugin_registry: CommandPluginRegistry,
    /// Queue of `CommandExecuted` events the dispatcher pushes from
    /// either entry path. Drained during `tick` and routed through
    /// `fire_event_typed` so subscribers see the same Phase 7 event
    /// shape regardless of where the dispatch originated.
    pending_command_events: PendingCommandEvents,
    /// Phase 9 unified keymap registry. Holds built-in, user, and
    /// plugin-registered bindings. Dispatch routes through
    /// [`PluginHost::dispatch_key`] which calls
    /// [`commands::dispatch_command`] for matched chords so the same
    /// Phase 8 funnel runs.
    keymap_registry: Rc<RefCell<KeymapRegistry>>,
    /// Phase 10 grant store. Persists every `(plugin_id,
    /// plugin_version, capability)` decision and is the only thing
    /// every sensitive API call consults at use time. Cheap-clone
    /// (Arc-backed); shared into every `CapabilityGuard`.
    grant_store: GrantStore,
    /// Phase 10 auto-grant policy. Bundled plugins (those loaded from
    /// a path under a trusted root) get their requested capabilities
    /// auto-allowed with `decided_by = "default"`. Non-bundled plugins
    /// go through the prompt path.
    auto_grant_policy: AutoGrantPolicy,
    /// Phase 11 active repository gateway handle. The app calls
    /// [`PluginHost::set_repository_gateway`] on tab switches to keep
    /// this current. Plugin git read/write APIs route through this
    /// handle, which is the same gateway built-in UI uses.
    active_gateway: ActiveRepositoryGateway,
    /// Phase 11 destructive-op confirmation policy. Defaults reject
    /// every destructive op until [`DestructiveConfirmPolicy::approve_next`]
    /// is called.
    destructive_policy: DestructiveConfirmPolicy,
    /// Phase 11 ring of recent / in-flight Git writes. Capped at
    /// [`PendingGitWrites::CAP`].
    pending_git_writes: PendingGitWrites,
    /// Phase 11 outbound queue of typed events the git Lua API queued
    /// up for the host to fire. Drained synchronously after each
    /// `leviathan.git.*` call (see [`Self::flush_pending_git_events`])
    /// and on every `tick`.
    pending_git_events: crate::plugin::api::git::PendingGitEvents,
    /// Phase 12 host-wide async-job registry. Cheap-clone (Arc-backed).
    async_jobs: AsyncJobRegistry,
    /// Phase 12 host-wide timer registry. Cheap-clone (Arc-backed).
    timers: TimerRegistry,
    /// Phase 12 host-wide file-watcher registry. Cheap-clone
    /// (Arc-backed). Drained per `tick` to dispatch buffered notify
    /// events to the matching plugin's Lua callback.
    watchers: FileWatcherRegistry,
    /// Phase 13 storage roots. Production uses OS dirs; tests can
    /// replace these with a temp-root before loading plugins.
    storage_roots: PluginStorageRoots,
    /// Phase 15 cached dependency graph from the last
    /// [`PluginHost::resolve_and_load`] (or `load_from_dir`) call.
    /// Surfaced by `introspect()` so devtools can render the same
    /// structure the resolver computed.
    dependency_graph: Vec<crate::plugin::dependency::DependencySummary>,
    /// Phase 16 lazy-activation registry. Plugins whose manifests
    /// declared an `[activation]` section are parked here until
    /// their first matching trigger fires; the host then runs
    /// `load_plugin` and re-dispatches the trigger. Stubs are
    /// recorded against synthetic `(plugin_id, gen=0)` ledgers so
    /// `unload_plugin` reaps them like real registrations.
    lazy_registry: crate::plugin::activation::LazyRegistry,
    /// Phase 16 ledgers for lazy-stub bookkeeping. One per parked
    /// plugin id; dropped (and its records released) when the
    /// plugin activates or is unloaded.
    lazy_ledgers: HashMap<String, ResourceLedger>,
    /// Phase 16 latest repository shape pushed in via
    /// [`PluginHost::sync_repository`]. Used to evaluate
    /// `activation.repository_shape` predicates against.
    last_repository_shape: Option<RepositoryShapeFacts>,
    /// Phase 17 host-wide extension registry. Owns overlays,
    /// context-menu items, and graph / diff decorations contributed
    /// via the new `leviathan.ui.*` extension APIs. Cheap-clone
    /// (Rc-backed); shared into every plugin's Lua factory.
    extension_registry: crate::plugin::extensions::ExtensionRegistry,
    /// Phase 18 performance / circuit-breaker tracker. Cheap-clone
    /// (Arc-Mutex). Every plugin callback dispatch routes through
    /// `track_call` against this tracker; the host's devtools snapshot
    /// projects the recent traces and per-callback breaker rows.
    budget_tracker: BudgetTracker,
    /// Phase 19 disabled-plugins set. Set by the
    /// `Plugin: Disable` devtools command and consulted by
    /// [`PluginHost::load_plugin`] before staging — a disabled id
    /// short-circuits load with a `plugin.disabled` diagnostic. The
    /// `Plugin: Enable` command removes the entry and re-loads the
    /// plugin from its known root.
    disabled_plugins: std::collections::HashSet<String>,
    /// Phase 19 last-known plugin root per id. Populated by every
    /// successful `load_plugin` so `Plugin: Enable` can re-load the
    /// plugin from disk after a `Plugin: Disable` that unloaded it.
    /// Survives unload/disable so re-enable works in a single step.
    last_plugin_roots: HashMap<String, PathBuf>,
    /// Phase 19 shared queue of pending devtools actions. Each
    /// devtools command body captures a clone of this `Rc` and pushes
    /// a [`crate::plugin::devtools_commands::DevtoolsAction`] when
    /// dispatched; [`PluginHost::invoke_command`] drains the queue
    /// after dispatch returns and applies each action against
    /// `&mut self`, producing the structured JSON result.
    devtools_action_queue: crate::plugin::devtools_commands::DevtoolsActionQueue,
    /// Phase 19 last devtools-action result, populated by the
    /// drain pass and consumed by [`PluginHost::invoke_devtools_command`].
    /// Cleared on every read.
    last_devtools_result: Option<serde_json::Value>,
}

/// Phase 16 cached repository facts the host evaluates against
/// `[activation.repository_shape]` predicates. Populated by
/// `sync_repository`; absent before the first repo open.
#[derive(Debug, Clone)]
struct RepositoryShapeFacts {
    current_branch: String,
    has_remote: bool,
    workdir: PathBuf,
}

const RELOAD_HISTORY_CAP: usize = 64;

/// Phase 19 helper used by `build_diagnostic_bundle` to inline a
/// surface's contents into the JSON state map. For files, parses as
/// JSON when possible (so the bundle preserves structure) and falls
/// back to a `{ "raw": "<size> bytes" }` placeholder for non-JSON
/// content. Walks directories recursively. Never reads files larger
/// than 256 KiB to keep bundles tractable; oversized files appear as
/// `{ "skipped": "size_exceeded", "bytes": <n> }`.
fn read_state_tree(path: &Path) -> serde_json::Value {
    const MAX_INLINE_BYTES: u64 = 256 * 1024;
    let Ok(meta) = std::fs::metadata(path) else {
        return serde_json::Value::Null;
    };
    if meta.is_file() {
        if meta.len() > MAX_INLINE_BYTES {
            return serde_json::json!({
                "skipped": "size_exceeded",
                "bytes": meta.len(),
            });
        }
        return match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str::<serde_json::Value>(&raw)
                .unwrap_or(serde_json::Value::String(raw)),
            Err(e) => serde_json::json!({
                "error": e.to_string(),
            }),
        };
    }
    if !meta.is_dir() {
        return serde_json::Value::Null;
    }
    let Ok(rd) = std::fs::read_dir(path) else {
        return serde_json::Value::Null;
    };
    let mut out = serde_json::Map::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        out.insert(name, read_state_tree(&entry.path()));
    }
    serde_json::Value::Object(out)
}

impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHost {
    pub fn new() -> Self {
        let mut host = Self {
            plugins: HashMap::new(),
            slot_ops: Vec::new(),
            active_screen: None,
            widget_tree: None,
            split_sizes: HashMap::new(),
            split_drag: None,
            event_bus: EventBus::new(),
            last_repository_hash: None,
            last_tab_snapshot: TabsSnapshot::default(),
            pending_tab_ops: Rc::new(RefCell::new(Vec::new())),
            audit_log: AuditLog::new(),
            last_reload_errors: HashMap::new(),
            service_registry: Rc::new(RefCell::new(ServiceRegistry::new())),
            next_generation_ids: HashMap::new(),
            diagnostics: DiagnosticStore::default(),
            runtime_path_registry: RuntimePathRegistry::new(),
            reload_history: HashMap::new(),
            command_registry: Rc::new(RefCell::new(CommandRegistry::new())),
            command_plugin_registry: CommandPluginRegistry::new(),
            pending_command_events: PendingCommandEvents::new(),
            keymap_registry: Rc::new(RefCell::new(KeymapRegistry::new())),
            grant_store: GrantStore::new_in_memory(),
            auto_grant_policy: AutoGrantPolicy::new(),
            active_gateway: ActiveRepositoryGateway::new(),
            destructive_policy: DestructiveConfirmPolicy::new(),
            pending_git_writes: PendingGitWrites::new(),
            pending_git_events: crate::plugin::api::git::PendingGitEvents::new(),
            async_jobs: AsyncJobRegistry::new(),
            timers: TimerRegistry::new(),
            watchers: FileWatcherRegistry::new(),
            storage_roots: PluginStorageRoots::os_default(),
            dependency_graph: Vec::new(),
            lazy_registry: crate::plugin::activation::LazyRegistry::new(),
            lazy_ledgers: HashMap::new(),
            last_repository_shape: None,
            extension_registry: crate::plugin::extensions::ExtensionRegistry::new(),
            budget_tracker: BudgetTracker::new(DiagnosticStore::default()),
            disabled_plugins: std::collections::HashSet::new(),
            last_plugin_roots: HashMap::new(),
            devtools_action_queue: Rc::new(RefCell::new(Vec::new())),
            last_devtools_result: None,
        };
        host.register_builtin_host_commands();
        host.register_builtin_devtools_commands();
        host
    }

    /// Phase 12 cheap-clone snapshot of the async-job registry.
    /// Devtools renders these directly.
    pub fn async_jobs(&self) -> AsyncJobRegistry {
        self.async_jobs.clone()
    }

    /// Phase 12 cheap-clone handle to the timer registry.
    pub fn timers(&self) -> TimerRegistry {
        self.timers.clone()
    }

    /// Phase 12 cheap-clone handle to the file-watcher registry.
    pub fn watchers(&self) -> FileWatcherRegistry {
        self.watchers.clone()
    }

    /// The app uses this to enable the plugin runtime tick only when
    /// there is Lua state that can own jobs, timers, watchers, or
    /// scheduled callbacks.
    pub fn has_loaded_plugins(&self) -> bool {
        !self.plugins.is_empty()
    }

    /// Build the [`AsyncRuntimeContext`] handed to every plugin's API
    /// install. Job/timer/watcher *callback maps* are per-plugin, so
    /// fresh ones are minted per call. The registries themselves are
    /// shared.
    fn build_async_runtime_context(
        &self,
        job_callbacks: Rc<RefCell<JobCallbacks>>,
        timer_callbacks: Rc<RefCell<PluginTimerCallbacks>>,
        watcher_callbacks: Rc<RefCell<PluginWatcherCallbacks>>,
    ) -> AsyncRuntimeContext {
        AsyncRuntimeContext {
            jobs: self.async_jobs.clone(),
            timers: self.timers.clone(),
            watchers: self.watchers.clone(),
            job_callbacks,
            timer_callbacks,
            watcher_callbacks,
        }
    }

    /// Phase 11 setter the app calls to keep the host's view of the
    /// active repository in sync. Pass `None` when no repository is
    /// open. The same gateway powers built-in UI ops, so plugin git
    /// reads/writes route through the existing pipeline.
    pub fn set_repository_gateway(
        &mut self,
        gateway: Option<crate::services::SharedRepositoryGateway>,
    ) {
        self.active_gateway.set(gateway);
    }

    /// Phase 11 destructive-policy handle. Devtools / tests use this
    /// to pre-approve a destructive op before invoking it. Cheap-clone.
    pub fn destructive_policy(&self) -> DestructiveConfirmPolicy {
        self.destructive_policy.clone()
    }

    /// Phase 11 cheap-clone snapshot of the recent + in-flight git
    /// write entries. Devtools renders these directly.
    pub fn pending_git_writes(&self) -> Vec<PendingGitWrite> {
        self.pending_git_writes.entries()
    }

    /// Build the [`GitOpsContext`] handed to every plugin's API
    /// install. Cheap-clone (every member is `Arc`-backed).
    fn git_ops_context(&self) -> GitOpsContext {
        GitOpsContext {
            gateway: self.active_gateway.clone(),
            destructive: self.destructive_policy.clone(),
            pending: self.pending_git_writes.clone(),
            audit: self.audit_log.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }

    /// Phase 11: drain queued git typed events and fire them through
    /// the existing typed-event funnel. Called after every
    /// `leviathan.git.*` invocation surface and once per `tick()`.
    fn flush_pending_git_events(&mut self) {
        let events = self.pending_git_events.drain();
        for (event, payload) in events {
            self.fire_event_typed(event, payload);
        }
    }

    /// Replace the in-memory grant store with one backed by a JSON file
    /// at `path`. Read failures surface as `capability.persistence_failed`
    /// warnings; the store still loads (empty) so subsequent decisions
    /// can be recorded. The host's existing in-memory store is dropped.
    pub fn use_grant_store_at(&mut self, path: PathBuf) {
        let (store, warnings) = GrantStore::with_path(path);
        for warn in warnings {
            self.diagnostics.record(PluginDiagnostic::new(
                PluginId::from("<host>"),
                DiagnosticSeverity::Warning,
                "capability.persistence_failed",
                warn,
            ));
        }
        self.grant_store = store;
    }

    /// Cheap-clone handle to the grant store. Devtools / inspectors
    /// use this to call `revoke` and to enumerate the live rows.
    pub fn grant_store(&self) -> GrantStore {
        self.grant_store.clone()
    }

    /// Mark `root` as a trusted bundled-plugin directory. Plugins
    /// whose `dir` canonicalises under this root get their requested
    /// capabilities auto-allowed at load (`decided_by = "default"`).
    pub fn trust_bundled_plugin_root(&mut self, root: impl Into<PathBuf>) {
        self.auto_grant_policy.trust_bundled_root(root);
    }

    /// Phase 10 capability-prompt resolution entry point used by the
    /// (future) devtools modal. Hands the prompt back to the user
    /// code with the current pending decisions; the caller calls
    /// [`crate::plugin::capability_grants::PromptState::decide`] for
    /// each row, then submits via [`Self::commit_capability_prompt`].
    /// Returns `None` when no prompt is pending for that plugin
    /// version.
    pub fn open_capability_prompt(
        &self,
        plugin_id: &str,
        plugin_version: &str,
    ) -> Option<crate::plugin::capability_grants::OverlayDescriptor> {
        for prompt in self.grant_store.pending_prompts() {
            if let Some(first) = prompt.pending().first() {
                if first.plugin_id == plugin_id && first.plugin_version == plugin_version {
                    return crate::plugin::capability_grants::OverlayDescriptor::from_prompt(
                        &prompt,
                    );
                }
            }
        }
        None
    }

    /// Commit a resolved prompt. The grant store records every
    /// decision and the host emits per-row `grant.allowed` /
    /// `grant.denied` audit entries.
    pub fn commit_capability_prompt(
        &mut self,
        plugin_id: &str,
        plugin_version: &str,
    ) -> Result<(), String> {
        let rows = self.grant_store.resolve_prompt(plugin_id, plugin_version)?;
        for row in rows {
            let code = match row.decision {
                crate::plugin::capability_grants::Decision::Allow => "grant.allowed",
                crate::plugin::capability_grants::Decision::Deny
                | crate::plugin::capability_grants::Decision::Pending => "grant.denied",
            };
            audit_grant_event(
                &self.audit_log,
                &row.plugin_id,
                code,
                &row.capability,
                Some(row.decided_by),
            );
        }
        Ok(())
    }

    /// Phase 10 revoke entry point used by devtools / settings UI.
    /// Records a `Deny` row, audits `grant.revoked`, and emits a
    /// `capability.revoked` info diagnostic so the next sensitive
    /// call's denial is traceable to this revoke.
    pub fn revoke_capability(
        &mut self,
        plugin_id: &str,
        plugin_version: &str,
        capability: &str,
    ) -> Result<(), String> {
        self.grant_store
            .revoke(plugin_id, plugin_version, capability)?;
        audit_grant_event(
            &self.audit_log,
            plugin_id,
            "grant.revoked",
            capability,
            None,
        );
        self.diagnostics.record(
            PluginDiagnostic::new(
                PluginId::from(plugin_id),
                DiagnosticSeverity::Info,
                "capability.revoked",
                format!("capability `{capability}` revoked for {plugin_id}@{plugin_version}"),
            )
            .with_context(serde_json::json!({
                "capability": capability,
                "plugin_version": plugin_version,
            })),
        );
        Ok(())
    }

    /// Build the [`CommandDispatchEnv`] handle plugins are handed at
    /// API install time. Cheap-clone (every member is `Rc`-backed); the
    /// host keeps its own clones in `Self`.
    fn command_dispatch_env(&self) -> CommandDispatchEnv {
        CommandDispatchEnv {
            commands: Rc::clone(&self.command_registry),
            plugin_registry: self.command_plugin_registry.clone(),
            diagnostics: self.diagnostics.clone(),
            pending_events: self.pending_command_events.clone(),
            budget_tracker: self.budget_tracker.clone(),
        }
    }

    /// Seed three representative built-in repository commands into the
    /// unified registry. Phase 8 keeps the wiring narrow: each
    /// built-in is a no-op host body that emits a structured info
    /// diagnostic so a `/repository.refresh` palette invocation does
    /// _something_ visible, but the full migration of every existing
    /// `RepositoryMessage` family to the registry is left to Phase 9
    /// (which also wires keymaps into the dispatcher). The acceptance
    /// gate only requires that one registry can hold both kinds —
    /// that's proven by registering at least one host command here
    /// and exercising it through `invoke_command` in tests.
    fn register_builtin_host_commands(&mut self) {
        let commands_to_register = [
            (
                "repository.fetch",
                "Repository: Fetch",
                "Fetch from the active remote.",
            ),
            (
                "repository.refresh",
                "Repository: Refresh",
                "Refresh the active repository projection.",
            ),
            (
                "repository.open",
                "Repository: Open",
                "Open a repository tab by path.",
            ),
        ];
        for (name, title, description) in commands_to_register {
            let descriptor = CommandDescriptor {
                name: name.into(),
                title: title.into(),
                description: description.into(),
                plugin_id: HOST_COMMAND_PLUGIN_ID.into(),
                generation_id: None,
                context: CommandContext::GLOBAL.into(),
                args: Vec::new(),
                destructive: false,
                capabilities: Vec::new(),
                run: CommandBody::Host(Box::new(|_args| Ok(()))),
            };
            self.command_registry
                .borrow_mut()
                .register(descriptor, &self.diagnostics);
        }

        // Phase 11: register `git.<op>` host commands so the palette
        // can list them and so the keymap-bound dispatcher can route
        // through `leviathan.command.invoke("git.checkout", ...)`. The
        // bodies route through the same [`GitOpsContext::execute`]
        // funnel the Lua `leviathan.git.*` API uses; capability checks
        // happen inside `execute` against an "unrestricted" host
        // closure (host commands have no plugin guard — the palette
        // user IS the guard).
        let git_specs: &[(&str, &str, &str, &str, bool)] = &[
            (
                "git.checkout",
                "Git: Checkout",
                "Check out a ref.",
                "ref",
                false,
            ),
            (
                "git.create_branch",
                "Git: Create Branch",
                "Create a branch at start_point.",
                "name",
                false,
            ),
            (
                "git.delete_branch",
                "Git: Delete Branch",
                "Delete a branch.",
                "name",
                true,
            ),
            (
                "git.create_tag",
                "Git: Create Tag",
                "Create a tag at target.",
                "name",
                false,
            ),
            (
                "git.delete_tag",
                "Git: Delete Tag",
                "Delete a tag locally.",
                "name",
                true,
            ),
            (
                "git.commit",
                "Git: Commit",
                "Commit staged changes.",
                "message",
                false,
            ),
            (
                "git.stash_push",
                "Git: Stash Push",
                "Stash dirty changes.",
                "message",
                false,
            ),
            (
                "git.stash_pop",
                "Git: Stash Pop",
                "Pop most recent stash.",
                "index",
                false,
            ),
            (
                "git.reset",
                "Git: Reset",
                "Reset current branch.",
                "ref",
                true,
            ),
            (
                "git.fetch",
                "Git: Fetch",
                "Fetch from remote.",
                "remote",
                false,
            ),
            (
                "git.push",
                "Git: Push",
                "Push current branch.",
                "remote",
                false,
            ),
            (
                "git.merge",
                "Git: Merge",
                "Merge a ref into current.",
                "ref",
                true,
            ),
            (
                "git.rebase",
                "Git: Rebase",
                "Rebase onto a ref.",
                "ref",
                true,
            ),
        ];
        for (name, title, doc, primary_arg, destructive) in git_specs.iter().copied() {
            let descriptor = CommandDescriptor {
                name: name.into(),
                title: title.into(),
                description: doc.into(),
                plugin_id: HOST_COMMAND_PLUGIN_ID.into(),
                generation_id: None,
                context: CommandContext::GLOBAL.into(),
                args: vec![commands::CommandArg {
                    name: primary_arg.into(),
                    ty: commands::CommandArgType::String,
                    required: false,
                    default: None,
                    doc: format!("Primary argument: {primary_arg}."),
                }],
                destructive,
                capabilities: Vec::new(),
                run: CommandBody::Host(Box::new(|_args| Ok(()))),
            };
            self.command_registry
                .borrow_mut()
                .register(descriptor, &self.diagnostics);
        }
    }

    /// Phase 19: register the ten devtools commands. Each body
    /// captures the host's
    /// [`crate::plugin::devtools_commands::DevtoolsActionQueue`] so
    /// the body can queue an action for the host to apply (with
    /// `&mut self`) after dispatch returns. Called once from
    /// [`PluginHost::new`] after the Phase 8 / 11 builtin commands.
    fn register_builtin_devtools_commands(&mut self) {
        crate::plugin::devtools_commands::register(
            &mut self.command_registry.borrow_mut(),
            Rc::clone(&self.devtools_action_queue),
            &self.diagnostics,
        );
    }

    /// Phase 19 entry point: invoke a devtools command by name and
    /// return its structured JSON result. Routes through the same
    /// [`PluginHost::invoke_command`] funnel as every other
    /// dispatched command so palette listing, schema validation,
    /// capability gating, audit + diagnostic emission stay uniform.
    /// After dispatch returns, queued
    /// [`crate::plugin::devtools_commands::DevtoolsAction`]s are
    /// drained and applied with `&mut self`; this wrapper keeps the
    /// last action's result and returns it alongside the outcome.
    /// Returns `(InvokeOutcome, Value::Null)` when the dispatch
    /// failed before the body ran.
    pub fn invoke_devtools_command(
        &mut self,
        name: &str,
        args: serde_json::Value,
    ) -> (InvokeOutcome, serde_json::Value) {
        let outcome = self.invoke_command(name, args);
        let value = self
            .last_devtools_result
            .take()
            .unwrap_or(serde_json::Value::Null);
        (outcome, value)
    }

    /// Apply every queued devtools action against `&mut self`. The
    /// last action's result is stashed in `self.last_devtools_result`
    /// for `invoke_devtools_command` to return. Empty queue is a
    /// no-op (e.g. the body short-circuited on bad args).
    fn drain_devtools_actions(&mut self) {
        let actions = std::mem::take(&mut *self.devtools_action_queue.borrow_mut());
        for action in actions {
            let result = self.apply_devtools_action(action);
            self.last_devtools_result = Some(result);
        }
    }

    fn apply_devtools_action(
        &mut self,
        action: crate::plugin::devtools_commands::DevtoolsAction,
    ) -> serde_json::Value {
        match action.name.as_str() {
            "plugin.reload" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                match self.reload_plugin(&plugin_id) {
                    Ok(()) => serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                    }),
                    Err(e) => serde_json::json!({
                        "ok": false,
                        "plugin_id": plugin_id,
                        "error": e.to_string(),
                    }),
                }
            }
            "plugin.disable" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                let unloaded = self.disable_plugin(&plugin_id);
                serde_json::json!({
                    "ok": true,
                    "plugin_id": plugin_id,
                    "unloaded": unloaded,
                })
            }
            "plugin.enable" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                match self.enable_plugin(&plugin_id) {
                    Ok(reloaded) => serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                        "reloaded": reloaded,
                    }),
                    Err(e) => serde_json::json!({
                        "ok": false,
                        "plugin_id": plugin_id,
                        "error": e,
                    }),
                }
            }
            "plugin.open_log" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                match self.plugin_log_path(&plugin_id) {
                    Some(p) => serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                        "path": p,
                    }),
                    None => {
                        self.record_unknown_plugin("plugin.open_log", &plugin_id);
                        serde_json::json!({
                            "ok": false,
                            "plugin_id": plugin_id,
                            "error": "plugin not loaded",
                        })
                    }
                }
            }
            "plugin.inspect_ui_tree" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                match self.widget_ast_inventory(&plugin_id) {
                    Some(value) => serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                        "inventory": value,
                    }),
                    None => {
                        self.record_unknown_plugin("plugin.inspect_ui_tree", &plugin_id);
                        serde_json::json!({
                            "ok": false,
                            "plugin_id": plugin_id,
                            "error": "plugin not loaded",
                        })
                    }
                }
            }
            "plugin.run_health_check" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                if !self.plugins.contains_key(&plugin_id) {
                    self.record_unknown_plugin("plugin.run_health_check", &plugin_id);
                    serde_json::json!({
                        "ok": false,
                        "plugin_id": plugin_id,
                        "error": "plugin not loaded",
                    })
                } else {
                    let report = self.run_health_checks();
                    let items = report
                        .for_plugin(&plugin_id)
                        .map(|h| {
                            h.items
                                .iter()
                                .map(|i| {
                                    serde_json::json!({
                                        "severity": match i.severity {
                                            crate::plugin::api::health::Severity::Ok => "ok",
                                            crate::plugin::api::health::Severity::Info => "info",
                                            crate::plugin::api::health::Severity::Warn => "warn",
                                            crate::plugin::api::health::Severity::Error => "error",
                                        },
                                        "message": i.message,
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                        "items": items,
                    })
                }
            }
            "plugin.clear_state" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                let surfaces_raw = action.arg_str("surfaces").unwrap_or("state,cache");
                let surfaces: Vec<String> = surfaces_raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                self.devtools_clear_state(&plugin_id, &surfaces)
            }
            "plugin.export_diagnostic_bundle" => {
                let plugin_id = action.arg_str("plugin_id").map(str::to_string);
                let include_state = action
                    .args
                    .get("include_state")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let include_secrets = action
                    .args
                    .get("include_secrets")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.build_diagnostic_bundle(plugin_id.as_deref(), include_state, include_secrets)
            }
            "plugin.show_capability_audit" => {
                let plugin_id = action.arg_str("plugin_id").map(str::to_string);
                let limit = action
                    .args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50)
                    .min(usize::MAX as u64) as usize;
                let entries = self.audit_log.entries();
                let filtered: Vec<crate::plugin::audit::AuditEntry> = entries
                    .into_iter()
                    .filter(|e| {
                        plugin_id
                            .as_deref()
                            .map(|pid| e.plugin_id == pid)
                            .unwrap_or(true)
                    })
                    .collect();
                let n = filtered.len();
                let start = n.saturating_sub(limit);
                let rows: Vec<serde_json::Value> = filtered[start..]
                    .iter()
                    .map(|e| {
                        let ts = e
                            .timestamp
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or(0);
                        serde_json::json!({
                            "plugin_id": e.plugin_id,
                            "capability": e.capability,
                            "target": e.target,
                            "outcome": match e.outcome {
                                crate::plugin::audit::AuditOutcome::Allowed => "allowed",
                                crate::plugin::audit::AuditOutcome::Denied => "denied",
                            },
                            "timestamp_unix_ms": ts,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "ok": true,
                    "filter_plugin_id": plugin_id,
                    "limit": limit,
                    "entries": rows,
                })
            }
            "plugin.show_runtime_path" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                match self.runtime_path_entries_for(&plugin_id) {
                    Some(rows) => serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                        "entries": rows,
                    }),
                    None => {
                        self.record_unknown_plugin("plugin.show_runtime_path", &plugin_id);
                        serde_json::json!({
                            "ok": false,
                            "plugin_id": plugin_id,
                            "error": "plugin not loaded",
                        })
                    }
                }
            }
            other => {
                // Defensive: an unknown action name slipped past the
                // body's allow-list. Surface as a diagnostic so the
                // mismatch is visible.
                let msg = format!("unknown devtools action `{other}`");
                self.diagnostics.record(PluginDiagnostic::new(
                    PluginId::from(HOST_COMMAND_PLUGIN_ID),
                    DiagnosticSeverity::Error,
                    "devtools.command.unknown",
                    msg.clone(),
                ));
                serde_json::json!({
                    "ok": false,
                    "error": msg,
                })
            }
        }
    }

    /// Emit a `devtools.command.unknown_plugin` diagnostic when an
    /// action references a plugin id that isn't loaded (or in the
    /// disabled set).
    fn record_unknown_plugin(&self, command: &str, plugin_id: &str) {
        self.diagnostics.record(
            PluginDiagnostic::new(
                PluginId::from(HOST_COMMAND_PLUGIN_ID),
                DiagnosticSeverity::Warning,
                "devtools.command.unknown_plugin",
                format!("devtools command `{command}` references unknown plugin `{plugin_id}`"),
            )
            .with_context(serde_json::json!({
                "command": command,
                "plugin_id": plugin_id,
            })),
        );
    }

    // ----- Phase 19 host helpers consumed by devtools commands -----

    /// Mark `plugin_id` as disabled. If the plugin is currently
    /// loaded, unload it as part of the disable so the user sees
    /// the effect immediately. Returns true when the plugin had to
    /// be unloaded; false when it was already absent.
    pub fn disable_plugin(&mut self, plugin_id: &str) -> bool {
        self.disabled_plugins.insert(plugin_id.to_string());
        self.diagnostics.record(PluginDiagnostic::new(
            PluginId::from(plugin_id),
            DiagnosticSeverity::Info,
            "plugin.disable",
            format!("plugin `{plugin_id}` marked disabled"),
        ));
        if self.plugins.contains_key(plugin_id) {
            // Best effort — unload errors land in diagnostics.
            let _ = self.unload_plugin(plugin_id);
            true
        } else {
            false
        }
    }

    /// Remove `plugin_id` from the disabled set. If the plugin's
    /// last-known root is still recorded, attempt a fresh load.
    /// Returns `Ok(true)` when a reload happened; `Ok(false)` when
    /// the disabled flag was cleared but no root was on file (so the
    /// plugin will be loaded on the next discovery pass).
    pub fn enable_plugin(&mut self, plugin_id: &str) -> Result<bool, String> {
        let was_disabled = self.disabled_plugins.remove(plugin_id);
        self.diagnostics.record(PluginDiagnostic::new(
            PluginId::from(plugin_id),
            DiagnosticSeverity::Info,
            "plugin.enable",
            format!("plugin `{plugin_id}` enabled (was_disabled={was_disabled})"),
        ));
        let Some(root) = self.last_plugin_roots.get(plugin_id).cloned() else {
            return Ok(false);
        };
        if self.plugins.contains_key(plugin_id) {
            return Ok(false);
        }
        self.load_plugin(&root)
            .map(|()| true)
            .map_err(|e| e.to_string())
    }

    /// Returns true when `plugin_id` is currently on the disabled
    /// list.
    pub fn is_plugin_disabled(&self, plugin_id: &str) -> bool {
        self.disabled_plugins.contains(plugin_id)
    }

    /// Cheap-clone snapshot of the disabled-plugins set. Sorted for
    /// stable test output.
    pub fn disabled_plugin_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.disabled_plugins.iter().cloned().collect();
        ids.sort();
        ids
    }

    /// Best-effort log path for a plugin. Today the host doesn't
    /// emit per-plugin log files; the diagnostic store *is* the log,
    /// so we return the storage state path (which devtools can use
    /// as a near-the-plugin location to dump a `diagnostic.log`).
    /// Returns `None` for unknown plugin ids.
    pub fn plugin_log_path(&self, plugin_id: &str) -> Option<String> {
        let plugin = self.plugins.get(plugin_id)?;
        let storage = self.storage_paths(plugin_id, plugin.root.clone());
        Some(
            storage
                .state_dir
                .join("diagnostic.log")
                .display()
                .to_string(),
        )
    }

    /// Read-only manifest accessor. Returns `None` when the plugin
    /// is not loaded.
    pub fn plugin_manifest(
        &self,
        plugin_id: &str,
    ) -> Option<&git_leviathan_plugin_api::manifest::PluginManifest> {
        self.plugins.get(plugin_id).map(|p| &p.manifest)
    }

    /// JSON projection of a plugin's widget AST inventory. Includes
    /// every slot, the active screen widget tree (if any), and every
    /// overlay / context-menu / decoration this plugin owns.
    /// Returns `None` for unknown plugin ids.
    pub fn widget_ast_inventory(&self, plugin_id: &str) -> Option<serde_json::Value> {
        if !self.plugins.contains_key(plugin_id) {
            return None;
        }
        // Slots owned by this plugin. We walk `slot_ops` the same way
        // `introspect()` does to materialise the live set, then filter
        // to `plugin_id`.
        let mut slot_map: std::collections::BTreeMap<(String, String, String), serde_json::Value> =
            std::collections::BTreeMap::new();
        for op in &self.slot_ops {
            match op {
                PreparedSlotOp::Add(p) if p.plugin_id == plugin_id => {
                    let key = (p.region.clone(), p.container.key(), p.id.clone());
                    slot_map.insert(
                        key,
                        serde_json::json!({
                            "id": p.id,
                            "region": p.region,
                            "container": p.container.key(),
                            "priority": p.priority,
                            "kind": match &p.widget {
                                crate::plugin::ui::main_bar_slots::SlotWidget::Static(_) => "static",
                                crate::plugin::ui::main_bar_slots::SlotWidget::Dynamic(_) => "dynamic",
                            },
                        }),
                    );
                }
                PreparedSlotOp::Replace {
                    region,
                    container,
                    id,
                    spec,
                } if spec.plugin_id == plugin_id => {
                    let key = (region.clone(), container.key(), id.clone());
                    slot_map.insert(
                        key,
                        serde_json::json!({
                            "id": id,
                            "region": region,
                            "container": container.key(),
                            "priority": spec.priority,
                            "kind": match &spec.widget {
                                crate::plugin::ui::main_bar_slots::SlotWidget::Static(_) => "static",
                                crate::plugin::ui::main_bar_slots::SlotWidget::Dynamic(_) => "dynamic",
                            },
                        }),
                    );
                }
                _ => {}
            }
        }
        let slots: Vec<serde_json::Value> = slot_map.into_values().collect();
        let overlays: Vec<serde_json::Value> = self
            .extension_registry
            .overlays()
            .into_iter()
            .filter(|o| o.plugin_id == plugin_id)
            .map(|o| {
                serde_json::json!({
                    "id": o.id,
                    "priority": o.priority,
                    "dismissible": o.dismissible,
                    "widget_kind": o.widget.node.kind(),
                })
            })
            .collect();
        let context_menu: Vec<serde_json::Value> = self
            .extension_registry
            .all_context_menu_items()
            .into_iter()
            .filter(|c| c.plugin_id == plugin_id)
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "region": c.region,
                    "label": c.label,
                    "command": c.command,
                    "priority": c.priority,
                })
            })
            .collect();
        let active_screen = self
            .active_screen
            .as_ref()
            .filter(|(pid, _)| pid == plugin_id)
            .map(|(_, screen)| {
                serde_json::json!({
                    "screen_id": screen,
                    "has_widget_tree": self.widget_tree.is_some(),
                    "widget_kind": self.widget_tree.as_ref().map(|t| t.node.kind()),
                })
            });
        Some(serde_json::json!({
            "slots": slots,
            "overlays": overlays,
            "context_menu": context_menu,
            "active_screen": active_screen,
        }))
    }

    /// Apply a `Plugin: Clear State` request. Iterates the requested
    /// surfaces, clearing each. Even when `secrets` appears in the
    /// list, the clear path goes through `reset_plugin_storage` so
    /// the same audit / fs path used by every other surface applies.
    fn devtools_clear_state(&mut self, plugin_id: &str, surfaces: &[String]) -> serde_json::Value {
        if !self.plugins.contains_key(plugin_id) {
            self.record_unknown_plugin("plugin.clear_state", plugin_id);
            return serde_json::json!({
                "ok": false,
                "plugin_id": plugin_id,
                "error": "plugin not loaded",
            });
        }
        let mut cleared: Vec<String> = Vec::new();
        let mut errors: Vec<serde_json::Value> = Vec::new();
        for surface in surfaces {
            match self.reset_plugin_storage(plugin_id, surface) {
                Ok(()) => cleared.push(surface.clone()),
                Err(e) => errors.push(serde_json::json!({
                    "surface": surface,
                    "error": e,
                })),
            }
        }
        serde_json::json!({
            "ok": errors.is_empty(),
            "plugin_id": plugin_id,
            "cleared": cleared,
            "errors": errors,
        })
    }

    /// Build a [`crate::plugin::diagnostic_bundle::DiagnosticBundle`]
    /// for `plugin_id` (or the host as a whole when `None`). Always
    /// excludes raw secret values; with `include_secrets = true`,
    /// only secret *keys* (metadata) are added.
    pub fn build_diagnostic_bundle(
        &self,
        plugin_id: Option<&str>,
        include_state: bool,
        include_secrets: bool,
    ) -> serde_json::Value {
        use crate::plugin::audit::AuditOutcome;
        let snap = self.introspect();
        let generated_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let host_version = env!("CARGO_PKG_VERSION").to_string();

        let manifest_json = plugin_id.and_then(|pid| {
            self.plugin_manifest(pid)
                .and_then(|m| serde_json::to_value(m).ok())
        });
        let api_version = plugin_id.and_then(|pid| {
            self.plugin_manifest(pid)
                .map(|m| format!("{}.{}", m.api_version.major, m.api_version.minor))
        });
        let installed_capabilities: Vec<String> = match plugin_id {
            Some(pid) => self
                .plugin_manifest(pid)
                .map(|m| m.capabilities.iter().cloned().map(String::from).collect())
                .unwrap_or_default(),
            None => snap
                .plugins
                .iter()
                .flat_map(|p| p.capabilities.iter().cloned())
                .collect(),
        };
        let diagnostics: Vec<serde_json::Value> = snap
            .diagnostics
            .iter()
            .filter(|d| {
                plugin_id
                    .map(|pid| d.plugin_id == pid || d.plugin_id == HOST_COMMAND_PLUGIN_ID)
                    .unwrap_or(true)
            })
            .map(|d| {
                serde_json::json!({
                    "plugin_id": d.plugin_id,
                    "generation_id": d.generation_id,
                    "severity": d.severity,
                    "code": d.code,
                    "message": d.message,
                    "source": d.source,
                    "context": d.context,
                    "timestamp_unix_ms": d.timestamp_unix_ms,
                })
            })
            .collect();
        let recent_audit: Vec<serde_json::Value> = snap
            .audit_recent
            .iter()
            .filter(|e| plugin_id.map(|pid| e.plugin_id == pid).unwrap_or(true))
            .map(|e| {
                let ts = e
                    .timestamp
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                serde_json::json!({
                    "plugin_id": e.plugin_id,
                    "capability": e.capability,
                    "target": e.target,
                    "outcome": match e.outcome {
                        AuditOutcome::Allowed => "allowed",
                        AuditOutcome::Denied => "denied",
                    },
                    "timestamp_unix_ms": ts,
                })
            })
            .collect();
        let reload_history: Vec<serde_json::Value> = snap
            .reload_history
            .iter()
            .filter(|e| plugin_id.map(|pid| e.plugin_id == pid).unwrap_or(true))
            .map(|e| {
                serde_json::json!({
                    "plugin_id": e.plugin_id,
                    "from_generation_id": e.from_generation_id,
                    "to_generation_id": e.to_generation_id,
                    "outcome": e.outcome.as_str(),
                    "duration_ms": e.duration_ms,
                    "stage_reached": e.stage_reached,
                    "error_code": e.error_code,
                    "error_message": e.error_message,
                    "timestamp_unix_ms": e.timestamp_unix_ms,
                })
            })
            .collect();
        let performance_traces: Vec<serde_json::Value> = snap
            .performance_traces
            .iter()
            .filter(|t| plugin_id.map(|pid| t.plugin_id == pid).unwrap_or(true))
            .map(|t| {
                serde_json::json!({
                    "plugin_id": t.plugin_id,
                    "generation_id": t.generation_id,
                    "callback_id": t.callback_id,
                    "kind": t.kind,
                    "duration_ms": t.duration_ms,
                    "ok": t.ok,
                    "timestamp_unix_ms": t.timestamp_unix_ms,
                })
            })
            .collect();
        let circuit_breakers: Vec<serde_json::Value> = snap
            .circuit_breakers
            .iter()
            .filter(|c| plugin_id.map(|pid| c.plugin_id == pid).unwrap_or(true))
            .map(|c| {
                serde_json::json!({
                    "plugin_id": c.plugin_id,
                    "generation_id": c.generation_id,
                    "callback_id": c.callback_id,
                    "kind": c.kind,
                    "state": c.state,
                    "consecutive_failures": c.consecutive_failures,
                    "count": c.count,
                    "ok_count": c.ok_count,
                    "err_count": c.err_count,
                    "p50_ms": c.p50_ms,
                    "p95_ms": c.p95_ms,
                    "last_failure": c.last_failure,
                })
            })
            .collect();

        // Plugin state (when requested). Walks every storage surface
        // EXCEPT secrets; secret values must never appear in the
        // bundle. The walk reads the surface's path through
        // `surface_dir` and collects file paths + sizes; values for
        // small JSON files are inlined.
        let state = if include_state {
            Some(self.collect_plugin_state(plugin_id))
        } else {
            None
        };

        // Secret metadata (when requested). Only the path + key
        // names are copied; secret *values* are never read into the
        // bundle. This is the same shape `SecretSummary` already
        // exposes through `introspect()`.
        let secret_metadata: Vec<serde_json::Value> = if include_secrets {
            snap.secrets
                .iter()
                .filter(|s| plugin_id.map(|pid| s.plugin_id == pid).unwrap_or(true))
                .map(|s| {
                    serde_json::json!({
                        "plugin_id": s.plugin_id,
                        "path": s.path,
                        "key_count": s.key_count,
                        "keys": s.keys,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        serde_json::json!({
            "generated_at_unix_ms": generated_at_unix_ms,
            "host_version": host_version,
            "plugin_id": plugin_id,
            "manifest": manifest_json,
            "api_version": api_version,
            "installed_capabilities": installed_capabilities,
            "diagnostics": diagnostics,
            "recent_audit": recent_audit,
            "reload_history": reload_history,
            "performance_traces": performance_traces,
            "circuit_breakers": circuit_breakers,
            "state": state,
            "include_secrets": include_secrets,
            "secret_metadata": secret_metadata,
        })
    }

    fn collect_plugin_state(&self, plugin_id: Option<&str>) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        for (id, plugin) in &self.plugins {
            if let Some(pid) = plugin_id {
                if id != pid {
                    continue;
                }
            }
            let storage = self.storage_paths(id, plugin.root.clone());
            let mut surfaces = serde_json::Map::new();
            for surface in [
                StorageSurface::State,
                StorageSurface::Cache,
                StorageSurface::Config,
                StorageSurface::Repo,
                StorageSurface::Settings,
            ] {
                let path = match surface {
                    StorageSurface::Settings => storage.settings_path(),
                    _ => storage.surface_dir(surface),
                };
                let value = read_state_tree(&path);
                surfaces.insert(surface.as_str().to_string(), value);
            }
            out.insert(id.clone(), serde_json::Value::Object(surfaces));
        }
        serde_json::Value::Object(out)
    }

    /// Runtime-path rows for `plugin_id`. Returns `None` when the
    /// plugin is not loaded.
    fn runtime_path_entries_for(&self, plugin_id: &str) -> Option<Vec<serde_json::Value>> {
        let plugin = self.plugins.get(plugin_id)?;
        Some(
            plugin
                .lua_loader
                .runtime_path()
                .entries()
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "entry_plugin_id": entry.plugin_id,
                        "kind": entry.kind.as_str(),
                        "root": entry.lua_root.display().to_string(),
                    })
                })
                .collect(),
        )
    }

    /// Public Rust dispatch entry point used by UI buttons, keymaps
    /// (Phase 9), and tests. Routes through the same
    /// [`commands::dispatch_command`] funnel as the Lua API so all
    /// entry paths share a single dispatcher (Phase 8 acceptance
    /// gate). Returns the structured outcome unchanged.
    pub fn invoke_command(&mut self, name: &str, args: serde_json::Value) -> InvokeOutcome {
        // Phase 16: when no live command matches but a lazy plugin
        // declared this command, activate the plugin and re-dispatch
        // through the now-live registry. The activation entry path
        // does its own re-dispatch and returns the final outcome.
        // Gated to test builds: the binary doesn't yet wire commands
        // through this entry point, and `activate_plugin_for_command`
        // is only reachable from tests.
        #[cfg(test)]
        {
            let live_has = self
                .command_registry
                .borrow()
                .entries()
                .iter()
                .any(|e| e.descriptor.name == name);
            let lazy_has = self
                .lazy_registry
                .entries()
                .iter()
                .filter(|e| e.status == crate::plugin::activation::LazyStatus::Lazy)
                .any(|e| e.commands.iter().any(|c| c == name));
            if !live_has && lazy_has {
                if let Some(outcome) = self.activate_plugin_for_command(name, args.clone()) {
                    return outcome;
                }
            }
        }
        let env = self.command_dispatch_env();
        let outcome = commands::dispatch_command(&env, name, args);
        // Drain queued events immediately so tests that observe
        // `CommandExecuted` after a single call see the firing
        // synchronously without needing a separate `tick()`.
        self.flush_pending_command_events();
        // Phase 11: any host git command may have queued typed events
        // (HeadChanged etc.) — fire them in the same flush so plugin
        // autocmds see post-op state before this returns.
        self.flush_pending_git_events();
        // Phase 19: when the dispatched command was a host devtools
        // command, drain queued devtools actions and apply each
        // against `&mut self`. This wires the devtools surface into
        // the same `invoke_command` entry point every other caller
        // uses (palette / keymap / Lua API) so the production bin
        // automatically picks up devtools functionality without a
        // separate dispatch path.
        if crate::plugin::devtools_commands::devtools_command_names().contains(&name) {
            self.drain_devtools_actions();
        }
        outcome
    }

    fn flush_pending_command_events(&mut self) {
        let events = self.pending_command_events.drain();
        for event in events {
            let mut payload = EventPayload::new();
            payload.insert("name".into(), serde_json::Value::String(event.name.clone()));
            payload.insert(
                "plugin_id".into(),
                serde_json::Value::String(event.plugin_id.clone()),
            );
            payload.insert("ok".into(), serde_json::Value::Bool(event.ok));
            // `duration_ms` isn't in the descriptor's required field
            // set, but plugins can still observe it through the
            // generic payload pass-through.
            payload.insert(
                "duration_ms".into(),
                serde_json::Value::Number(serde_json::Number::from(event.duration_ms as u64)),
            );
            self.fire_event_typed("CommandExecuted", payload);
        }
    }

    /// Cheap-clone handle to the unified command registry. Tests use
    /// it to project summaries; the palette will too. Plugin Lua code
    /// goes through `leviathan.command.list` instead.
    pub fn command_registry(&self) -> Rc<RefCell<CommandRegistry>> {
        Rc::clone(&self.command_registry)
    }

    /// Cheap-clone handle to the unified keymap registry. Used by
    /// devtools, tests, and the Lua `list` shim.
    pub fn keymap_registry(&self) -> Rc<RefCell<KeymapRegistry>> {
        Rc::clone(&self.keymap_registry)
    }

    /// Set the active leader prefix. Empty leader is rejected by the
    /// parser at use time. Tests pin it to `,` so `<leader>gh` parses
    /// to a deterministic chord.
    pub fn set_keymap_leader(&mut self, leader: impl Into<String>) {
        self.keymap_registry.borrow_mut().set_leader(leader);
    }

    /// Phase 9 user-keymap entry point. Tests use this directly; the
    /// Phase 10 user-config loader will too. Built-in / plugin
    /// bindings are NOT routed through here — they have their own
    /// install paths.
    pub fn set_user_keymap(
        &mut self,
        context: impl Into<String>,
        key: &str,
        command: impl Into<String>,
    ) {
        self.keymap_registry.borrow_mut().set_user_keymap(
            context,
            key,
            command,
            serde_json::Value::Null,
            String::new(),
            &self.diagnostics,
        );
    }

    /// Insert a built-in (host-owned) binding. Same tier as
    /// `Ctrl+Tab` and friends — beats every plugin / user binding for
    /// the same `(context, chord)`.
    pub fn set_builtin_keymap(
        &mut self,
        context: impl Into<String>,
        key: &str,
        command: impl Into<String>,
        description: impl Into<String>,
    ) {
        self.keymap_registry.borrow_mut().set_builtin(
            context,
            key,
            command,
            serde_json::Value::Null,
            description,
            &self.diagnostics,
        );
    }

    /// Phase 9 dispatch entry. Pass the active context and the
    /// in-flight keystroke buffer; the host walks the resolver and,
    /// on a match, routes through the Phase 8 command dispatcher.
    /// Returns the structured outcome the live `src/app/input.rs`
    /// (or a test driver) consults to decide whether to keep
    /// buffering, dispatch, or fall back to built-in app handling.
    pub fn dispatch_key(&mut self, context: &str, buffer: &[Keystroke]) -> KeymapDispatchOutcome {
        // Phase 16: when the live registry has no entry but a lazy
        // plugin's `[activation.keymaps]` declared this chord, route
        // through the activation entry. After activation, re-dispatch
        // through the live registry. `match_chord` ignores
        // conflict-lost rows, so probing it here is safe. Test-only
        // because the binary's input layer doesn't yet flow chords
        // through `dispatch_key`.
        #[cfg(test)]
        {
            let live_match = self.keymap_registry.borrow().match_chord(context, buffer);
            if matches!(live_match, keymap_mod::MatchOutcome::None) && !buffer.is_empty() {
                // Phase 16: parse each lazy entry's declared key with
                // the active leader and compare against the buffer.
                // The manifest carries the lhs verbatim (`<leader>l`),
                // so we can't shortcut via `render_chord` (which spells
                // out the leader explicitly).
                let leader = self.keymap_registry.borrow().leader().to_string();
                let lazy_match: Option<(String, String)> = self
                    .lazy_registry
                    .entries()
                    .iter()
                    .filter(|e| e.status == crate::plugin::activation::LazyStatus::Lazy)
                    .find_map(|entry| {
                        entry
                            .keymaps
                            .iter()
                            .filter(|k| k.context == context)
                            .find_map(|k| {
                                let parsed =
                                    keymap_mod::parse_key_sequence(&k.key, &leader).ok()?;
                                if parsed == buffer {
                                    Some((entry.plugin_id.clone(), k.key.clone()))
                                } else {
                                    None
                                }
                            })
                    });
                if let Some((_plugin_id, key)) = lazy_match {
                    if let Some(outcome) = self.activate_plugin_for_keymap(context, &key) {
                        if let KeymapDispatchOutcome::Dispatched {
                            context: ref ctx,
                            ref key,
                            ref command,
                            ref plugin_id,
                            ref outcome,
                        } = outcome
                        {
                            let payload = keymap_mod::keymap_triggered_payload(
                                ctx,
                                key,
                                command,
                                plugin_id,
                                matches!(outcome, InvokeOutcome::Ok),
                            );
                            self.fire_event_typed("KeymapTriggered", payload);
                        }
                        return outcome;
                    }
                }
            }
        }
        let env = self.command_dispatch_env();
        let outcome =
            self.keymap_registry
                .borrow()
                .dispatch(context, buffer, &env, &self.diagnostics);
        // Drain queued `CommandExecuted` events so subscribers see
        // them synchronously, same as `invoke_command` does.
        self.flush_pending_command_events();
        // Phase 11: keymap-triggered git commands queue typed events
        // through the shared pending-git-events handle; flush so the
        // `HeadChanged` / `RefsChanged` etc. fire before this returns.
        self.flush_pending_git_events();
        if let KeymapDispatchOutcome::Dispatched {
            context: ref ctx,
            ref key,
            ref command,
            ref plugin_id,
            ref outcome,
        } = outcome
        {
            let payload = keymap_mod::keymap_triggered_payload(
                ctx,
                key,
                command,
                plugin_id,
                matches!(outcome, InvokeOutcome::Ok),
            );
            self.fire_event_typed("KeymapTriggered", payload);
        }
        outcome
    }

    fn record_reload_event(&mut self, event: ReloadEventSummary) {
        let bucket = self
            .reload_history
            .entry(event.plugin_id.clone())
            .or_default();
        if bucket.len() == RELOAD_HISTORY_CAP {
            bucket.pop_front();
        }
        bucket.push_back(event);
    }

    /// Replace the diagnostic store. Tests pass a store wired to
    /// `NullSink` so test output stays quiet while every emission path
    /// still records a real diagnostic. The Phase 18 tracker is
    /// rebuilt against the new store so its breach / trip diagnostics
    /// land in the same buffer the rest of the host uses.
    pub fn set_diagnostic_store(&mut self, store: DiagnosticStore) {
        self.diagnostics = store.clone();
        self.budget_tracker = BudgetTracker::new(store);
    }

    /// Phase 18: cheap-clone handle to the budget tracker. Tests use
    /// this to drive the breaker (`reset_breaker`) and project
    /// summaries directly. Production code goes through
    /// [`Self::reset_breaker`].
    pub fn budget_tracker(&self) -> BudgetTracker {
        self.budget_tracker.clone()
    }

    /// Replace the budget tracker. Phase 18 tests use this to inject
    /// a `MockClock`-backed tracker so timing assertions stay
    /// deterministic.
    pub fn set_budget_tracker(&mut self, tracker: BudgetTracker) {
        self.budget_tracker = tracker;
    }

    /// Phase 18 user-facing breaker reset. Walks every generation
    /// under `plugin_id` and emits a `performance.reset` info
    /// diagnostic.
    pub fn reset_breaker(&self, plugin_id: &str, callback_id: &str) {
        self.budget_tracker
            .reset_breaker(&PluginId::from(plugin_id), callback_id);
    }

    /// Phase 18: query whether a callback's circuit breaker is
    /// tripped. The app-level startup pass calls this with sentinel
    /// values to keep the lookup path warm; tests use it through
    /// `MockHost::budget_tracker()` for assertions.
    pub fn is_breaker_tripped(
        &self,
        plugin_id: &str,
        generation_id: u64,
        callback_id: &str,
    ) -> bool {
        self.budget_tracker.is_tripped(
            &PluginId::from(plugin_id),
            GenerationId::new(generation_id),
            callback_id,
        )
    }

    /// Phase 18: drop every breaker / trace row for `plugin_id`.
    /// Called from `unload_plugin`; the startup pass uses a sentinel
    /// id so production builds always have a live entry into this
    /// path.
    pub fn drop_breaker_state_for_plugin(&self, plugin_id: &str) {
        self.budget_tracker.drop_for_plugin(plugin_id);
    }

    /// Phase 18: drop breaker / trace rows tied to one specific
    /// generation. Called from the reload-cleanup walk so the new
    /// generation starts with a clean slate.
    pub fn drop_breaker_state_for_generation(&self, plugin_id: &str, generation_id: u64) {
        self.budget_tracker
            .drop_for_generation(plugin_id, GenerationId::new(generation_id));
    }

    /// Cheap-clone handle to the host diagnostic store. Read by the
    /// devtools panel and tests.
    pub fn diagnostics(&self) -> DiagnosticStore {
        self.diagnostics.clone()
    }

    pub fn set_plugin_storage_base(&mut self, base: impl AsRef<Path>) {
        self.storage_roots = PluginStorageRoots::under_base(base);
    }

    /// Test-only accessor that exposes the per-plugin storage paths
    /// so harness code can seed / inspect a plugin's surfaces. Same
    /// path layout the production code uses; cheap to call.
    pub fn storage_paths_for_test(
        &self,
        plugin_id: &str,
        plugin_root: &Path,
    ) -> PluginStoragePaths {
        self.storage_paths(plugin_id, plugin_root.to_path_buf())
    }

    fn storage_paths(
        &self,
        plugin_id: &str,
        plugin_root: impl Into<PathBuf>,
    ) -> PluginStoragePaths {
        self.storage_roots.for_plugin(plugin_id, plugin_root)
    }

    pub fn reset_plugin_storage(&mut self, plugin_id: &str, surface: &str) -> Result<(), String> {
        let plugin = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| format!("plugin `{plugin_id}` not loaded"))?;
        let surface = StorageSurface::parse(surface)?;
        let storage = self.storage_paths(plugin_id, plugin.root.clone());
        let path = match surface {
            StorageSurface::Settings => storage.settings_path(),
            _ => storage.surface_dir(surface),
        };
        if !path.exists() {
            return Ok(());
        }
        if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|e| e.to_string())
        } else {
            std::fs::remove_file(&path).map_err(|e| e.to_string())
        }
    }

    fn allocate_generation_id(&mut self, plugin_id: &str) -> GenerationId {
        let next = self
            .next_generation_ids
            .entry(plugin_id.to_string())
            .or_insert(1);
        let id = *next;
        *next += 1;
        GenerationId::new(id)
    }

    fn validate_service_dependencies(
        &self,
        manifest: &PluginManifest,
        generation_id: GenerationId,
    ) -> Result<(), PluginLoadError> {
        let statuses = {
            let registry = self.service_registry.borrow();
            dependency_statuses(
                &manifest.id,
                &manifest.provides_services,
                &manifest.consumes_services,
                &registry,
            )
        };
        for status in statuses {
            if status.satisfied {
                continue;
            }
            if status.required {
                let message = format!(
                    "required service `{}` is not registered before `{}` loads",
                    status.service_key, manifest.id
                );
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(manifest.id.clone()),
                        DiagnosticSeverity::Error,
                        "services.required_missing",
                        message.clone(),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::Manifest {
                        path: format!("plugins/{}/plugin.toml", manifest.id),
                        key: Some("consumes_services".to_string()),
                    })
                    .with_context(serde_json::json!({
                        "service": status.service_key,
                        "required": true,
                    })),
                );
                return Err(PluginLoadError::BadManifest(message));
            }
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Warning,
                    "services.optional_missing",
                    format!(
                        "optional service `{}` is not registered; consumer will receive nil",
                        status.service_key
                    ),
                )
                .with_generation(generation_id)
                .with_source(PluginSourceSpan::Manifest {
                    path: format!("plugins/{}/plugin.toml", manifest.id),
                    key: Some("consumes_services".to_string()),
                })
                .with_context(serde_json::json!({
                    "service": status.service_key,
                    "required": false,
                })),
            );
        }
        Ok(())
    }

    fn cleanup_ledger(&mut self, ledger: &ResourceLedger) {
        let mut cleaner = HostResourceCleaner {
            slot_ops: &mut self.slot_ops,
            event_bus: &mut self.event_bus,
            service_registry: Rc::clone(&self.service_registry),
        };
        let report = ledger.cleanup_all(&mut cleaner);
        if report.has_errors() {
            for error in report.errors {
                let diag = PluginDiagnostic::new(
                    error.resource.plugin_id.clone(),
                    DiagnosticSeverity::Error,
                    "cleanup.resource_failed",
                    format!(
                        "cleanup failed for {} {}: {}",
                        error.resource.kind, error.resource.handle, error.error
                    ),
                )
                .with_generation(error.resource.generation_id)
                .with_context(serde_json::json!({
                    "kind": error.resource.kind.as_str(),
                    "handle": error.resource.handle,
                    "resource_id": error.resource.resource_id.get(),
                }));
                self.diagnostics.record(diag);
            }
        }
    }

    /// Cheap-clone handle to the per-host capability audit log. Will be
    /// consumed by the devtools panel (Phase 6).
    pub fn audit_log(&self) -> AuditLog {
        self.audit_log.clone()
    }

    pub fn load_from_default_dirs(&mut self) {
        let cwd_plugins = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("plugins");
        if cwd_plugins.is_dir() {
            self.load_from_dir(&cwd_plugins);
        }
    }

    /// Discover every plugin under `dir`, resolve their dependency
    /// graph, then load successful plugins in deterministic order.
    /// Reads `plugins.lock` (and `plugins.lock.local`) from `dir` to
    /// validate checksums; rewrites `plugins.lock` afterwards so the
    /// next run sees the resolved set. Failures emit structured
    /// diagnostics; this entry never returns a `Result` because the
    /// startup path needs to keep going past per-plugin failures.
    pub fn load_from_dir(&mut self, dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut candidate_dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.join("plugin.toml").exists() && p.join("init.lua").exists())
            .collect();
        candidate_dirs.sort();
        self.resolve_and_load(dir, &candidate_dirs);
    }

    /// Phase 15 resolver entry point. `lockfile_dir` is the root used
    /// for `plugins.lock` / `plugins.lock.local`; `candidate_dirs`
    /// is the set of plugin directories to consider for loading.
    pub fn resolve_and_load(&mut self, lockfile_dir: &Path, candidate_dirs: &[PathBuf]) {
        use crate::plugin::dependency::{
            resolve, BlockReason, OptionalMissReason, ResolutionReport,
        };
        use crate::plugin::lockfile::{
            compute_plugin_checksum, LockedPlugin, Lockfile, LOCAL_OVERRIDE_NAME, LOCKFILE_NAME,
        };

        // 1) Read manifests, drop dirs whose manifest fails to parse
        //    (load_plugin's own diagnostic path runs at load time, but
        //    here we surface a separate code so callers can tell the
        //    difference between "discovery failed" and "load failed").
        let mut manifests: Vec<PluginManifest> = Vec::new();
        let mut dir_by_id: HashMap<String, PathBuf> = HashMap::new();
        for plugin_dir in candidate_dirs {
            let manifest_path = plugin_dir.join("plugin.toml");
            let manifest_str = match fs::read_to_string(&manifest_path) {
                Ok(s) => s,
                Err(e) => {
                    let id = plugin_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("<unknown>")
                        .to_string();
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(id),
                            DiagnosticSeverity::Error,
                            "manifest.read_failed",
                            format!("could not read plugin.toml: {e}"),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: manifest_path.display().to_string(),
                            key: None,
                        }),
                    );
                    continue;
                }
            };
            let manifest: PluginManifest = match toml::from_str(&manifest_str) {
                Ok(m) => m,
                Err(e) => {
                    let id = plugin_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("<unknown>")
                        .to_string();
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(id),
                            DiagnosticSeverity::Error,
                            "manifest.parse_failed",
                            format!("invalid plugin.toml: {e}"),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: manifest_path.display().to_string(),
                            key: None,
                        }),
                    );
                    continue;
                }
            };
            dir_by_id.insert(manifest.id.clone(), plugin_dir.clone());
            manifests.push(manifest);
        }

        // 2) Resolve.
        let report: ResolutionReport = resolve(&manifests);
        self.dependency_graph = report.graph.clone();

        // 3) Diagnose blocked plugins. Each (plugin, reason) becomes
        //    its own diagnostic so devtools can group and dedupe.
        for (plugin_id, reasons) in &report.blocked {
            let path = dir_by_id
                .get(plugin_id)
                .map(|p| p.join("plugin.toml").display().to_string())
                .unwrap_or_else(|| format!("plugins/{plugin_id}/plugin.toml"));
            for reason in reasons {
                let (code, message, ctx) = match reason {
                    BlockReason::MissingRequired {
                        dependency_id,
                        requirement,
                    } => (
                        "dependency.missing_required",
                        format!(
                            "plugin `{plugin_id}` requires `{dependency_id} {requirement}` but it is not present"
                        ),
                        serde_json::json!({
                            "dependency": dependency_id,
                            "requirement": requirement,
                        }),
                    ),
                    BlockReason::Conflict {
                        dependency_id,
                        requirement,
                        actual_version,
                    } => (
                        "dependency.conflict",
                        format!(
                            "plugin `{plugin_id}` requires `{dependency_id} {requirement}` but found `{actual_version}`"
                        ),
                        serde_json::json!({
                            "dependency": dependency_id,
                            "requirement": requirement,
                            "actual_version": actual_version,
                        }),
                    ),
                    BlockReason::Cycle { cycle } => (
                        "dependency.cycle",
                        format!(
                            "plugin `{plugin_id}` participates in dependency cycle [{}]",
                            cycle.join(" -> ")
                        ),
                        serde_json::json!({ "cycle": cycle }),
                    ),
                    BlockReason::BlockedTransitive { dependency_id } => (
                        "dependency.blocked_transitive",
                        format!(
                            "plugin `{plugin_id}` blocked because dependency `{dependency_id}` is blocked"
                        ),
                        serde_json::json!({ "dependency": dependency_id }),
                    ),
                };
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id.clone()),
                        DiagnosticSeverity::Error,
                        code,
                        message,
                    )
                    .with_source(PluginSourceSpan::Manifest {
                        path: path.clone(),
                        key: Some("dependencies".to_string()),
                    })
                    .with_context(ctx),
                );
            }
        }
        for miss in &report.optional_misses {
            let path = dir_by_id
                .get(&miss.consumer_plugin_id)
                .map(|p| p.join("plugin.toml").display().to_string())
                .unwrap_or_else(|| format!("plugins/{}/plugin.toml", miss.consumer_plugin_id));
            let (msg, extra) = match &miss.reason {
                OptionalMissReason::NotPresent => (
                    format!(
                        "optional dependency `{} {}` not present; `{}` will load without it",
                        miss.dependency_id, miss.requirement, miss.consumer_plugin_id
                    ),
                    serde_json::json!({ "reason": "not_present" }),
                ),
                OptionalMissReason::VersionMismatch { actual_version } => (
                    format!(
                        "optional dependency `{} {}` present at `{}`, requirement not met; `{}` loads without it",
                        miss.dependency_id, miss.requirement, actual_version, miss.consumer_plugin_id
                    ),
                    serde_json::json!({
                        "reason": "version_mismatch",
                        "actual_version": actual_version,
                    }),
                ),
            };
            let mut ctx = serde_json::json!({
                "dependency": miss.dependency_id,
                "requirement": miss.requirement,
            });
            if let (Some(obj), Some(extra_obj)) = (ctx.as_object_mut(), extra.as_object()) {
                for (k, v) in extra_obj {
                    obj.insert(k.clone(), v.clone());
                }
            }
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(miss.consumer_plugin_id.clone()),
                    DiagnosticSeverity::Warning,
                    "dependency.optional_missing",
                    msg,
                )
                .with_source(PluginSourceSpan::Manifest {
                    path,
                    key: Some("optional_dependencies".to_string()),
                })
                .with_context(ctx),
            );
        }

        // 4) Read lockfile + local override (if either exists). Apply
        //    overrides on top, then validate checksums against the
        //    plugins about to load. Mismatches and unknown ids emit
        //    diagnostics; they don't block the load — the next write
        //    will refresh the lockfile to match disk.
        let lock_path = lockfile_dir.join(LOCKFILE_NAME);
        let local_path = lockfile_dir.join(LOCAL_OVERRIDE_NAME);
        let mut effective_lock = if lock_path.is_file() {
            match Lockfile::read(&lock_path) {
                Ok(l) => l,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from("<lockfile>"),
                            DiagnosticSeverity::Warning,
                            "lockfile.read_failed",
                            format!("could not read plugins.lock: {e}"),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: lock_path.display().to_string(),
                            key: None,
                        }),
                    );
                    Lockfile::new()
                }
            }
        } else {
            Lockfile::new()
        };
        if local_path.is_file() {
            match Lockfile::read(&local_path) {
                Ok(overlay) => effective_lock.apply_overlay(&overlay),
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from("<lockfile>"),
                            DiagnosticSeverity::Warning,
                            "lockfile.read_failed",
                            format!("could not read plugins.lock.local: {e}"),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: local_path.display().to_string(),
                            key: None,
                        }),
                    );
                }
            }
        }

        let live_ids: HashSet<String> = report.load_order.iter().cloned().collect();
        for entry in &effective_lock.plugins {
            if !live_ids.contains(&entry.id) {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(entry.id.clone()),
                        DiagnosticSeverity::Warning,
                        "lockfile.unknown_plugin",
                        format!(
                            "plugins.lock pins `{}` but no plugin with that id is present; entry will be dropped on next write",
                            entry.id
                        ),
                    )
                    .with_source(PluginSourceSpan::Manifest {
                        path: lock_path.display().to_string(),
                        key: Some("plugin".to_string()),
                    })
                    .with_context(serde_json::json!({ "version": entry.version })),
                );
            }
        }

        // 5) Validate checksums for plugins we're about to load.
        let mut new_lock_entries: Vec<LockedPlugin> = Vec::new();
        for plugin_id in &report.load_order {
            let Some(plugin_dir) = dir_by_id.get(plugin_id) else {
                continue;
            };
            let manifest = manifests
                .iter()
                .find(|m| &m.id == plugin_id)
                .expect("manifest present for load-order id");
            let checksum = match compute_plugin_checksum(plugin_dir) {
                Ok(c) => c,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Warning,
                            "lockfile.checksum_failed",
                            format!("could not compute plugin checksum: {e}"),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: plugin_dir.display().to_string(),
                            key: None,
                        }),
                    );
                    String::new()
                }
            };
            if let Some(locked) = effective_lock.lookup(plugin_id) {
                if !checksum.is_empty() && locked.checksum != checksum {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Warning,
                            "lockfile.checksum_mismatch",
                            format!(
                                "plugin `{plugin_id}` content changed since plugins.lock was written"
                            ),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: plugin_dir.display().to_string(),
                            key: None,
                        })
                        .with_context(serde_json::json!({
                            "expected": locked.checksum,
                            "actual": checksum,
                            "locked_version": locked.version,
                        })),
                    );
                }
            }
            new_lock_entries.push(LockedPlugin {
                id: plugin_id.clone(),
                version: manifest.version.to_string(),
                source: "local".to_string(),
                checksum,
            });
        }

        // 6) Split into eager / lazy cohorts. Plugins with a non-empty
        //    `[activation]` section are parked in the lazy registry
        //    via `install_lazy_stubs`; everyone else loads eagerly.
        //    `load_plugin` already records its own diagnostics; we
        //    just keep going on per-plugin failures so a misbehaving
        //    plugin can't shadow the rest.
        for plugin_id in &report.load_order {
            let Some(plugin_dir) = dir_by_id.get(plugin_id) else {
                continue;
            };
            let manifest = manifests
                .iter()
                .find(|m| &m.id == plugin_id)
                .expect("manifest present for load-order id");
            if let Some(activation) = manifest.activation.as_ref() {
                if !activation.is_empty() {
                    self.install_lazy_stubs(plugin_id, plugin_dir, activation);
                    continue;
                }
            }
            if let Err(e) = self.load_plugin(plugin_dir) {
                let _ = e;
            }
        }

        // 7) Re-write the lockfile in sorted order.
        let new_lock = Lockfile {
            plugins: new_lock_entries,
        };
        if !new_lock.plugins.is_empty() {
            if let Err(e) = new_lock.write(&lock_path) {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from("<lockfile>"),
                        DiagnosticSeverity::Warning,
                        "lockfile.write_failed",
                        format!("could not write plugins.lock: {e}"),
                    )
                    .with_source(PluginSourceSpan::Manifest {
                        path: lock_path.display().to_string(),
                        key: None,
                    }),
                );
            }
        }
    }

    /// Phase 16: park a plugin in the lazy registry. Records stubs
    /// against a synthetic ledger keyed under `(plugin_id, gen=0)`
    /// so `unload_plugin` can reap them. Validation diagnostics
    /// fire here for unknown events / empty trigger fields. A
    /// plugin with an activation section that has nothing usable
    /// (after validation) falls back to eager-load so users aren't
    /// surprised by a never-activating plugin.
    fn install_lazy_stubs(
        &mut self,
        plugin_id: &str,
        plugin_dir: &Path,
        activation: &git_leviathan_plugin_api::manifest::PluginActivation,
    ) {
        use crate::plugin::activation::{LazyEntry, LazyStatus};

        let manifest_path = plugin_dir.join("plugin.toml");
        let validated = crate::plugin::activation::validate(
            plugin_id,
            &manifest_path,
            activation,
            &self.diagnostics,
        );

        let any_stub = !validated.commands.is_empty()
            || !validated.keymaps.is_empty()
            || !validated.events.is_empty()
            || !validated.regions.is_empty()
            || !validated.files.is_empty()
            || validated.repository_shape.is_some()
            || validated.manual;
        if !any_stub {
            // Empty activation (after validation) — fall back to eager
            // load. We've already recorded any `manifest.activation_invalid`
            // diagnostics in the validate path.
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(plugin_id),
                    DiagnosticSeverity::Warning,
                    "manifest.activation_invalid",
                    format!(
                        "plugin `{plugin_id}` has an [activation] section with no usable triggers; loading eagerly"
                    ),
                )
                .with_source(PluginSourceSpan::Manifest {
                    path: manifest_path.display().to_string(),
                    key: Some("activation".to_string()),
                }),
            );
            if let Err(e) = self.load_plugin(plugin_dir) {
                let _ = e;
            }
            return;
        }

        // Allocate a synthetic ledger so `unload_plugin` (and the
        // standard `cleanup_ledger` walk) can reap stubs uniformly.
        let plugin_id_typed = PluginId::from(plugin_id);
        let synthetic_gen = GenerationId::new(0);
        let ledger = ResourceLedger::new(plugin_id_typed.clone(), synthetic_gen);
        for cmd in &validated.commands {
            ledger.record(
                PluginResourceKind::ActivationStub,
                format!("command:{cmd}"),
                None,
            );
        }
        for km in &validated.keymaps {
            ledger.record(
                PluginResourceKind::ActivationStub,
                format!("keymap:{}:{}", km.context, km.key),
                None,
            );
        }
        for ev in &validated.events {
            ledger.record(
                PluginResourceKind::ActivationStub,
                format!("event:{ev}"),
                None,
            );
        }
        for r in &validated.regions {
            ledger.record(
                PluginResourceKind::ActivationStub,
                format!("region:{r}"),
                None,
            );
        }
        for f in &validated.files {
            ledger.record(
                PluginResourceKind::ActivationStub,
                format!("file:{}", f.display()),
                None,
            );
        }
        if validated.repository_shape.is_some() {
            ledger.record(
                PluginResourceKind::ActivationStub,
                "repository_shape".to_string(),
                None,
            );
        }
        if validated.manual {
            ledger.record(
                PluginResourceKind::ActivationStub,
                "manual".to_string(),
                None,
            );
        }

        self.lazy_ledgers
            .insert(plugin_id.to_string(), ledger.clone());
        self.lazy_registry.insert(LazyEntry {
            plugin_id: plugin_id.to_string(),
            plugin_dir: plugin_dir.to_path_buf(),
            manual: validated.manual,
            commands: validated.commands,
            keymaps: validated.keymaps,
            events: validated.events,
            regions: validated.regions,
            files: validated.files,
            repository_shape: validated.repository_shape,
            status: LazyStatus::Lazy,
            activations: 0,
            last_activation_unix_ms: None,
            last_activation_trigger: None,
            last_error: None,
            consecutive_failures: 0,
        });

        self.diagnostics.record(
            PluginDiagnostic::new(
                plugin_id_typed,
                DiagnosticSeverity::Info,
                "activation.parked",
                format!("plugin `{plugin_id}` parked for lazy activation"),
            )
            .with_source(PluginSourceSpan::Manifest {
                path: manifest_path.display().to_string(),
                key: Some("activation".to_string()),
            }),
        );
        // Drop our ref; ledger persists via `lazy_ledgers`.
        let _ = ledger;
    }

    /// Activate the lazy plugin matching `command_name` and
    /// re-dispatch the command into the live registry. Returns the
    /// dispatch outcome when a match was found; `None` when no
    /// lazy plugin owns the command id.
    ///
    /// Test-only entry point: the binary's command dispatch path is
    /// not yet wired into this funnel, so the binary build hides
    /// the lazy branch entirely.
    #[cfg(test)]
    pub fn activate_plugin_for_command(
        &mut self,
        command_name: &str,
        args: serde_json::Value,
    ) -> Option<InvokeOutcome> {
        let plugin_id = self
            .lazy_registry
            .entries()
            .iter()
            .filter(|e| e.status == crate::plugin::activation::LazyStatus::Lazy)
            .find(|e| e.commands.iter().any(|c| c == command_name))?
            .plugin_id
            .clone();
        match self.activate_now(&plugin_id, "command", format!("command:{command_name}")) {
            Ok(()) => {
                // Re-dispatch directly through the funnel; the lazy
                // registry no longer contains this command, so the
                // outer guard in `invoke_command` won't recurse.
                let env = self.command_dispatch_env();
                let outcome = commands::dispatch_command(&env, command_name, args);
                self.flush_pending_command_events();
                self.flush_pending_git_events();
                Some(outcome)
            }
            Err(_) => None,
        }
    }

    /// Activate a lazy plugin in response to a keymap chord. The
    /// caller has already determined the `(context, key)` pair is
    /// declared on a lazy plugin. After load, we look up the
    /// command id the now-live keymap registry resolves the chord
    /// to and dispatch it. When no live keymap matches (e.g. the
    /// plugin failed to register the binding), records an
    /// `activation.unknown_keymap` diagnostic.
    ///
    /// Test-only entry point — see `activate_plugin_for_command`.
    #[cfg(test)]
    pub fn activate_plugin_for_keymap(
        &mut self,
        context: &str,
        key: &str,
    ) -> Option<KeymapDispatchOutcome> {
        let plugin_id = self
            .lazy_registry
            .entries()
            .iter()
            .filter(|e| e.status == crate::plugin::activation::LazyStatus::Lazy)
            .find(|e| {
                e.keymaps
                    .iter()
                    .any(|k| k.context == context && k.key == key)
            })
            .map(|e| e.plugin_id.clone())?;
        match self.activate_now(&plugin_id, "keymap", format!("keymap:{context}:{key}")) {
            Ok(()) => {
                // Re-dispatch through the unified dispatcher; the lazy
                // entry is gone now so probing the registry directly
                // avoids re-entering the activation path.
                let leader = self.keymap_registry.borrow().leader().to_string();
                let parsed = keymap_mod::parse_key_sequence(key, &leader).unwrap_or_default();
                if parsed.is_empty() {
                    self.diagnostics.record(PluginDiagnostic::new(
                        PluginId::from(plugin_id.as_str()),
                        DiagnosticSeverity::Warning,
                        "activation.unknown_keymap",
                        format!(
                            "lazy keymap `{context}:{key}` could not be parsed after activation"
                        ),
                    ));
                    return Some(KeymapDispatchOutcome::Unhandled);
                }
                let env = self.command_dispatch_env();
                let outcome = self.keymap_registry.borrow().dispatch(
                    context,
                    &parsed,
                    &env,
                    &self.diagnostics,
                );
                self.flush_pending_command_events();
                self.flush_pending_git_events();
                if matches!(outcome, KeymapDispatchOutcome::Unhandled) {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.as_str()),
                            DiagnosticSeverity::Warning,
                            "activation.unknown_keymap",
                            format!(
                                "plugin `{plugin_id}` activated for keymap `{context}:{key}` but did not register the binding"
                            ),
                        ),
                    );
                }
                Some(outcome)
            }
            Err(_) => None,
        }
    }

    /// Phase 16 internal activation worker. Removes the plugin's
    /// lazy stubs (ledger entries), runs `load_plugin`, then either
    /// marks the entry `Active` (success) or bumps the failure
    /// counter and records `activation.failed` /
    /// `activation.poisoned`. `trigger_kind` and `trigger_descriptor`
    /// are recorded on the diagnostic context and (on success) on
    /// the lazy registry row so the inspector can see what woke the
    /// plugin up.
    fn activate_now(
        &mut self,
        plugin_id: &str,
        trigger_kind: &str,
        trigger_descriptor: String,
    ) -> Result<(), String> {
        use crate::plugin::activation::LazyStatus;
        let entry = match self.lazy_registry.lookup(plugin_id) {
            Some(e) if e.status == LazyStatus::Lazy => e.clone(),
            Some(e) => {
                return Err(format!(
                    "plugin `{plugin_id}` not in lazy state ({})",
                    e.status.as_str()
                ));
            }
            None => return Err(format!("no lazy entry for `{plugin_id}`")),
        };
        // Drop the stub ledger so its records disappear before the
        // real load creates fresh ones.
        if let Some(ledger) = self.lazy_ledgers.remove(plugin_id) {
            self.cleanup_ledger(&ledger);
        }
        match self.load_plugin(&entry.plugin_dir) {
            Ok(()) => {
                self.lazy_registry.mark_active(
                    plugin_id,
                    now_unix_ms(),
                    trigger_descriptor.clone(),
                );
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Info,
                        "activation.completed",
                        format!("plugin `{plugin_id}` activated by trigger `{trigger_descriptor}`"),
                    )
                    .with_context(serde_json::json!({
                        "trigger_kind": trigger_kind,
                        "trigger": trigger_descriptor,
                    })),
                );
                // Verify the plugin actually registered every command
                // and keymap it declared in `[activation]`. Anything
                // missing emits a warning but does NOT poison the
                // plugin — the user's first invocation already
                // succeeded (the redispatch above) or the command
                // simply was not auto-callable.
                self.verify_activation_promises(plugin_id, &entry);
                Ok(())
            }
            Err(err) => {
                let msg = err.to_string();
                // Re-install lazy stubs *before* recording failure
                // so the registry has a row to mark. The reinsert
                // resets counters; we then carry over the live
                // failure counter via `mark_failure` so the second
                // attempt poisons.
                let activation = git_leviathan_plugin_api::manifest::PluginActivation {
                    events: entry.events.clone(),
                    commands: entry.commands.clone(),
                    keymaps: entry.keymaps.clone(),
                    regions: entry.regions.clone(),
                    files: entry
                        .files
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect(),
                    repository_shape: entry.repository_shape.clone(),
                    manual: entry.manual,
                };
                self.install_lazy_stubs(plugin_id, &entry.plugin_dir, &activation);
                // Carry over the prior consecutive-failure count
                // before bumping; otherwise every failure looks like
                // the first.
                let prior = entry.consecutive_failures;
                if let Some(e) = self
                    .lazy_registry
                    .entries_mut()
                    .iter_mut()
                    .find(|e| e.plugin_id == plugin_id)
                {
                    e.consecutive_failures = prior;
                }
                let poisoned = self.lazy_registry.mark_failure(plugin_id, msg.clone());
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "activation.failed",
                        format!("plugin `{plugin_id}` activation failed: {msg}"),
                    )
                    .with_context(serde_json::json!({
                        "trigger_kind": trigger_kind,
                        "trigger": trigger_descriptor,
                        "error": msg,
                    })),
                );
                if poisoned {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "activation.poisoned",
                            format!(
                                "plugin `{plugin_id}` poisoned after {} consecutive activation failures",
                                crate::plugin::activation::MAX_ACTIVATION_FAILURES
                            ),
                        ),
                    );
                }
                Err(msg)
            }
        }
    }

    fn verify_activation_promises(
        &self,
        plugin_id: &str,
        entry: &crate::plugin::activation::LazyEntry,
    ) {
        let registry = self.command_registry.borrow();
        for cmd in &entry.commands {
            let registered = registry
                .entries()
                .iter()
                .any(|e| e.descriptor.plugin_id == plugin_id && e.descriptor.name == *cmd);
            if !registered {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Warning,
                        "activation.unknown_command",
                        format!(
                            "plugin `{plugin_id}` declared activation command `{cmd}` but did not register it after activation"
                        ),
                    )
                    .with_context(serde_json::json!({ "command": cmd })),
                );
            }
        }
        drop(registry);
        let kreg = self.keymap_registry.borrow();
        for km in &entry.keymaps {
            let registered = kreg
                .entries()
                .iter()
                .any(|e| e.plugin_id == plugin_id && e.context == km.context);
            if !registered {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Warning,
                        "activation.unknown_keymap",
                        format!(
                            "plugin `{plugin_id}` declared activation keymap `{}:{}` but did not register a binding after activation",
                            km.context, km.key
                        ),
                    )
                    .with_context(serde_json::json!({
                        "context": km.context,
                        "key": km.key,
                    })),
                );
            }
        }
    }

    pub fn load_plugin(&mut self, dir: &Path) -> Result<(), PluginLoadError> {
        // Phase 19: respect the disabled-plugins set. If the id (derived
        // from the manifest below) is in `self.disabled_plugins`, we
        // refuse to load. Pre-parse the manifest to know the id; the
        // actual `disabled` check happens after we have a manifest in
        // hand. We surface the refusal as a structured diagnostic +
        // error so callers see the same shape as a manifest-bad load.
        let manifest_path = dir.join("plugin.toml");
        let manifest_str = match fs::read_to_string(&manifest_path) {
            Ok(s) => s,
            Err(e) => {
                let plugin_id = dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("<unknown>")
                    .to_string();
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "manifest.read_failed",
                        format!("could not read plugin.toml: {e}"),
                    )
                    .with_source(PluginSourceSpan::Manifest {
                        path: manifest_path.display().to_string(),
                        key: None,
                    }),
                );
                return Err(PluginLoadError::Io(e));
            }
        };
        let manifest: PluginManifest = match toml::from_str(&manifest_str) {
            Ok(m) => m,
            Err(e) => {
                let plugin_id = dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("<unknown>")
                    .to_string();
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "manifest.parse_failed",
                        format!("invalid plugin.toml: {e}"),
                    )
                    .with_source(PluginSourceSpan::Manifest {
                        path: manifest_path.display().to_string(),
                        key: None,
                    }),
                );
                return Err(PluginLoadError::Toml(e));
            }
        };
        // Phase 19: short-circuit when the plugin is on the
        // disabled-plugins list. The diagnostic carries enough
        // context to debug a "why isn't my plugin loading" question
        // without scraping stderr; the error variant is `Disabled`
        // so callers can surface the reason explicitly.
        if self.disabled_plugins.contains(&manifest.id) {
            let msg = format!("plugin `{}` is disabled", manifest.id);
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Info,
                    "plugin.disabled",
                    msg.clone(),
                )
                .with_source(PluginSourceSpan::Manifest {
                    path: manifest_path.display().to_string(),
                    key: None,
                }),
            );
            // Record the root anyway so a later `enable` knows where
            // to look. This is safe: the path was readable enough to
            // parse a manifest, so it's a real plugin directory.
            self.last_plugin_roots
                .insert(manifest.id.clone(), dir.to_path_buf());
            return Err(PluginLoadError::Disabled(msg));
        }
        if !manifest.api_version.is_compatible_with(HOST_API_VERSION) {
            let msg = format!(
                "api version {}.{} not compatible with host {}.{}",
                manifest.api_version.major,
                manifest.api_version.minor,
                HOST_API_VERSION.major,
                HOST_API_VERSION.minor
            );
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Error,
                    "manifest.incompatible_api_version",
                    msg.clone(),
                )
                .with_source(PluginSourceSpan::Manifest {
                    path: manifest_path.display().to_string(),
                    key: Some("api_version".to_string()),
                }),
            );
            return Err(PluginLoadError::BadManifest(msg));
        }
        let init_path = dir.join("init.lua");
        let init_src = match fs::read_to_string(&init_path) {
            Ok(s) => s,
            Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(manifest.id.clone()),
                        DiagnosticSeverity::Error,
                        "lua.read_failed",
                        format!("could not read init.lua: {e}"),
                    )
                    .with_source(PluginSourceSpan::Lua {
                        file: init_path.display().to_string(),
                        line: None,
                        traceback: None,
                    }),
                );
                return Err(PluginLoadError::Io(e));
            }
        };

        let plugin_root = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let storage_paths = self.storage_paths(&manifest.id, plugin_root.clone());
        let state_dir = storage_paths.state_dir.clone();
        let config_dir = storage_paths.config_dir.clone();
        let workdir: Option<PathBuf> = None;

        let plugin_id = PluginId::from(manifest.id.clone());
        let generation_id = self.allocate_generation_id(&manifest.id);
        let plugin_version = manifest.version.to_string();

        self.validate_service_dependencies(&manifest, generation_id)?;

        // Phase 10: walk the requested capability list. Anything the
        // descriptor table doesn't know becomes a `capability.unknown`
        // warning; anything not yet decided gets either auto-granted
        // (bundled plugin) or queued for the security prompt
        // (non-bundled). Bundled detection uses the canonical plugin
        // root we already computed.
        let requested_strings = canonicalize_requested(&manifest.capabilities);
        for unknown in unknown_requested(&requested_strings) {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    plugin_id.clone(),
                    DiagnosticSeverity::Warning,
                    "capability.unknown",
                    format!(
                        "manifest declares unknown capability `{unknown}`; ignored at use time"
                    ),
                )
                .with_generation(generation_id),
            );
        }
        if self.auto_grant_policy.is_bundled(&plugin_root) {
            let granted = self.grant_store.auto_grant_bundled(
                &manifest.id,
                &plugin_version,
                &requested_strings,
            );
            for row in granted {
                audit_grant_event(
                    &self.audit_log,
                    &row.plugin_id,
                    "grant.allowed",
                    &row.capability,
                    Some(row.decided_by),
                );
            }
        } else if let Some(prompt) = prompt_for_undecided(
            &self.grant_store,
            &manifest.id,
            &plugin_version,
            &requested_strings,
        ) {
            audit_grant_event(
                &self.audit_log,
                &manifest.id,
                "grant.upgrade_prompted",
                &format!("{} new capabilities", prompt.pending().len()),
                None,
            );
            if let Err(e) = self.grant_store.open_prompt(prompt) {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        plugin_id.clone(),
                        DiagnosticSeverity::Warning,
                        "capability.persistence_failed",
                        format!("could not open capability prompt: {e}"),
                    )
                    .with_generation(generation_id),
                );
            }
        }

        let guard = Rc::new(
            CapabilityGuard::new(
                manifest.id.clone(),
                plugin_version.clone(),
                manifest.capabilities.clone(),
                plugin_root.clone(),
                state_dir,
                config_dir,
                workdir,
                self.grant_store.clone(),
            )
            .with_audit(self.audit_log.clone())
            .with_diagnostics(
                self.diagnostics.clone(),
                plugin_id.clone(),
                generation_id,
            ),
        );

        let lua = Rc::new(Lua::new());
        let generation = PluginGeneration::new(plugin_id.clone(), generation_id, Rc::clone(&lua));
        let ledger = generation.ledger.clone();
        let build: Rc<RefCell<BuildState>> = Rc::new(RefCell::new(BuildState::default()));
        let deferred: Rc<RefCell<DeferredQueue>> = Rc::new(RefCell::new(DeferredQueue::default()));
        let user_commands: Rc<RefCell<UserCommands>> =
            Rc::new(RefCell::new(UserCommands::default()));
        let health_checks_sink: Rc<RefCell<Vec<HealthCheckRegistration>>> =
            Rc::new(RefCell::new(Vec::new()));

        let services_ctx = ServicesContext {
            registry: Rc::clone(&self.service_registry),
            lookup_registry: None,
            plugin_id: manifest.id.clone(),
            generation_id,
            provides: manifest.provides_services.clone(),
            consumes: manifest.consumes_services.clone(),
            plugin_lua: Rc::clone(&lua),
            capability_guard: Rc::clone(&guard),
        };
        self.service_registry
            .borrow()
            .set_budget_tracker(self.budget_tracker.clone());

        let persist_ctx = PersistContext {
            storage: storage_paths,
        };

        let job_callbacks: Rc<RefCell<JobCallbacks>> = Rc::new(RefCell::new(JobCallbacks::new()));
        let timer_callbacks: Rc<RefCell<PluginTimerCallbacks>> =
            Rc::new(RefCell::new(PluginTimerCallbacks::new()));
        let watcher_callbacks: Rc<RefCell<PluginWatcherCallbacks>> =
            Rc::new(RefCell::new(PluginWatcherCallbacks::new()));
        let async_ctx = self.build_async_runtime_context(
            Rc::clone(&job_callbacks),
            Rc::clone(&timer_callbacks),
            Rc::clone(&watcher_callbacks),
        );

        if let Err(e) = api::install_all(
            &lua,
            Rc::clone(&build),
            Rc::clone(&self.pending_tab_ops),
            Rc::clone(&guard),
            services_ctx,
            persist_ctx,
            Rc::clone(&deferred),
            Rc::clone(&user_commands),
            Rc::clone(&health_checks_sink),
            ledger.clone(),
            self.command_dispatch_env(),
            Rc::clone(&self.keymap_registry),
            self.git_ops_context(),
            self.pending_git_events.clone(),
            async_ctx,
            plugin_id.clone(),
            generation_id,
            self.diagnostics.clone(),
            self.extension_registry.clone(),
        ) {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Error,
                    "host.install_api_failed",
                    String::new(),
                )
                .with_generation(generation_id)
                .with_mlua_error(&format!("plugins/{}/install_api", manifest.id), &e),
            );
            self.cleanup_ledger(&ledger);
            return Err(e.into());
        }

        // Phase 5: build the runtime path. The plugin's own root is
        // passed in directly; dependency roots are looked up from the
        // host-wide registry. Registration of *this* plugin's root is
        // deferred until init.lua succeeds, so a failed load doesn't
        // briefly expose a plugin that doesn't actually exist.
        let runtime_path = PluginRuntimePath::resolve(
            &manifest.id,
            &plugin_root,
            &manifest.requires_plugins,
            &self.runtime_path_registry,
        );
        let lua_loader = LuaLoader::new(
            plugin_id.clone(),
            generation_id,
            runtime_path,
            self.diagnostics.clone(),
            manifest.runtime.strict_globals,
        );
        if let Err(e) = lua_loader.install(&lua) {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Error,
                    "host.install_loader_failed",
                    String::new(),
                )
                .with_generation(generation_id)
                .with_mlua_error(&format!("plugins/{}/install_loader", manifest.id), &e),
            );
            self.cleanup_ledger(&ledger);
            return Err(e.into());
        }
        if let Err(e) = lua
            .globals()
            .get::<Table>("leviathan")
            .and_then(|t| install_runtime_module(&lua, &t, lua_loader.clone()))
        {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Error,
                    "host.install_runtime_module_failed",
                    String::new(),
                )
                .with_generation(generation_id)
                .with_mlua_error(
                    &format!("plugins/{}/install_runtime_module", manifest.id),
                    &e,
                ),
            );
            self.cleanup_ledger(&ledger);
            return Err(e.into());
        }

        let chunk_name = format!("plugins/{}/init.lua", manifest.id);
        // Phase 18: time init.lua against the `Init` budget. We pass
        // a fresh `PluginId` rather than borrowing from the manifest
        // because the closure has to be `'static` over the Lua state.
        let init_pid = PluginId::from(manifest.id.clone());
        let init_outcome = self.budget_tracker.track_call::<(), mlua::Error>(
            CallbackKind::Init,
            &init_pid,
            generation_id,
            "init.lua",
            || lua.load(&init_src).set_name(chunk_name.clone()).exec(),
        );
        if let PerfOutcome::Err(e) = init_outcome {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Error,
                    "lua.init_failed",
                    String::new(),
                )
                .with_generation(generation_id)
                .with_mlua_error(&chunk_name, &e),
            );
            self.cleanup_ledger(&ledger);
            return Err(PluginLoadError::Plugin(
                git_leviathan_plugin_api::error::PluginError::from_mlua(
                    &manifest.id,
                    "init.lua exec",
                    &e,
                ),
            ));
        }
        // After init.lua succeeds, register the plugin's root and walk
        // after/plugin/*.lua in lexical order. Failures in the
        // after-walk are recorded as diagnostics but don't unload the
        // plugin — after-files are bootstrap helpers, not load gates.
        self.runtime_path_registry
            .register(manifest.id.clone(), plugin_root.clone());
        lua_loader.run_after_plugin(&lua, &plugin_root);

        let (screens, slot_ops, autocmds) = {
            let mut b = build.borrow_mut();
            (
                std::mem::take(&mut b.screens),
                std::mem::take(&mut b.slot_ops),
                std::mem::take(&mut b.autocmds),
            )
        };

        let mut slot_handlers = HashMap::new();
        let mut dynamic_widgets = HashMap::new();
        for op in slot_ops {
            match prepare_op(
                &manifest.id,
                &plugin_root,
                op,
                &ledger,
                &mut slot_handlers,
                &mut dynamic_widgets,
            ) {
                Ok(prepared) => self.slot_ops.push(prepared),
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(manifest.id.clone()),
                            DiagnosticSeverity::Warning,
                            "schema.slot_invalid",
                            format!("slot op ignored: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.ui.regions.add_slot".into(),
                        }),
                    );
                }
            }
        }

        // Resolve plugin-local autocmd-group ids to host-wide
        // GroupIds, applying any `clear = true` requests captured at
        // declaration time. The same translation map lets autocmds
        // reference their group through the host's stable handle.
        //
        // Operations replay in plugin-local declaration sequence so
        // a `clear` issued *after* an `autocmd.create` correctly
        // removes that registration.
        let (raw_groups, raw_clears) = {
            let mut b = build.borrow_mut();
            (
                std::mem::take(&mut b.autocmd_groups),
                std::mem::take(&mut b.autocmd_clears),
            )
        };
        let mut local_to_host: HashMap<u64, GroupId> = HashMap::new();
        let mut autocmd_ops = MergedAutocmdOps::new(autocmds, raw_groups, raw_clears);
        while let Some(op) = autocmd_ops.next() {
            match op {
                AutocmdOp::Group(grp) => {
                    let host_id = self
                        .event_bus
                        .group_handle(&manifest.id, &grp.name, grp.clear);
                    local_to_host.insert(grp.local_id, host_id);
                }
                AutocmdOp::Clear(clear) => {
                    if let Some(host_id) = local_to_host.get(&clear.local_id).copied() {
                        self.event_bus.clear_group(&manifest.id, host_id);
                    }
                }
                AutocmdOp::Create(raw) => {
                    self.install_one_autocmd(&manifest.id, generation_id, raw, &local_to_host);
                }
            }
        }

        self.diagnostics.record(
            PluginDiagnostic::new(
                PluginId::from(manifest.id.clone()),
                DiagnosticSeverity::Info,
                "host.plugin_loaded",
                format!("loaded plugin {} ({})", manifest.id, manifest.name),
            )
            .with_generation(generation_id),
        );
        let health_checks: Vec<HealthCheckRegistration> =
            std::mem::take(&mut *health_checks_sink.borrow_mut());

        // Phase 8: register the plugin's Lua state + capability guard
        // in the dispatcher's plugin map *before* installing any
        // commands so the dispatcher can resolve them. Then install
        // any `RawCommand`s captured during init into the unified
        // registry. Reload uses the same install path inside
        // `commit_staging`.
        self.command_plugin_registry.insert(
            manifest.id.clone(),
            CommandPluginContext {
                lua: Rc::clone(&lua),
                capability_guard: Rc::clone(&guard),
                generation_id,
            },
        );
        let raw_commands = std::mem::take(&mut build.borrow_mut().commands);
        self.install_raw_commands(&manifest.id, generation_id, raw_commands);

        // Phase 9: install captured keymap rows. Apply staged `del`s
        // after the `set`s so a plugin can rebind its own keymaps in
        // one init.lua run.
        let (raw_keymaps, raw_keymap_dels) = {
            let mut b = build.borrow_mut();
            (
                std::mem::take(&mut b.keymaps),
                std::mem::take(&mut b.keymap_dels),
            )
        };
        self.keymap_registry.borrow_mut().install_plugin_raws(
            &manifest.id,
            generation_id,
            raw_keymaps,
            &self.diagnostics,
        );
        self.keymap_registry.borrow_mut().apply_plugin_dels(
            &manifest.id,
            generation_id,
            raw_keymap_dels,
        );

        let plugin = LoadedPlugin {
            generation,
            root: plugin_root,
            manifest: manifest.clone(),
            slot_handlers,
            screens,
            screen_state: HashMap::new(),
            dynamic_widgets,
            deferred,
            user_commands,
            health_checks,
            lua_loader,
            job_callbacks,
            timer_callbacks,
            watcher_callbacks,
        };
        self.plugins.insert(manifest.id.clone(), plugin);
        // Phase 19: remember where this plugin loaded from so a later
        // `Plugin: Enable` (after `Plugin: Disable` unloaded it) can
        // re-load it from the same on-disk path without the user
        // re-supplying the root.
        self.last_plugin_roots
            .insert(manifest.id.clone(), dir.to_path_buf());

        // Populate dynamic widget caches so the first render has a real
        // tree instead of a placeholder null.
        self.refresh_dynamic_widgets_for_plugin(&manifest.id);
        Ok(())
    }

    /// Convert `RawCommand` rows into `CommandDescriptor`s and
    /// register them. Bad descriptors emit `command.invalid_args`
    /// diagnostics and the row is dropped (registration failure
    /// must not block plugin load).
    fn install_raw_commands(
        &mut self,
        plugin_id: &str,
        generation_id: GenerationId,
        raws: Vec<RawCommand>,
    ) {
        for raw in raws {
            let name_for_diag = raw.name.clone();
            let source = raw.source_location.clone();
            match commands::build_descriptor(raw, plugin_id.to_string(), Some(generation_id)) {
                Ok(descriptor) => {
                    self.command_registry
                        .borrow_mut()
                        .register(descriptor, &self.diagnostics);
                }
                Err(errors) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Warning,
                            "command.invalid_args",
                            format!(
                                "command `{name_for_diag}` rejected at registration: {}",
                                errors.join("; ")
                            ),
                        )
                        .with_generation(generation_id)
                        .with_source(make_lua_span(plugin_id, source.as_deref()))
                        .with_context(serde_json::json!({
                            "name": name_for_diag,
                            "errors": errors,
                        })),
                    );
                }
            }
        }
    }

    /// Apply every collected `main_bar` op to a `MainBarRegistry`.
    pub fn apply_main_bar_slots(&self, registry: &mut MainBarRegistry) {
        self.apply_region_slots(registry, "main_bar", PreparedSlot::into_main_bar);
    }

    /// Apply every collected `repository` op to a `RepoRegionRegistry`.
    pub fn apply_repo_region_slots(&self, registry: &mut RepoRegionRegistry) {
        self.apply_region_slots(registry, "repository", PreparedSlot::into_repo_pane);
    }

    /// Apply every collected `tab_bar` op to a `TabBarRegistry`.
    pub fn apply_tab_bar_slots(&self, registry: &mut TabBarRegistry) {
        self.apply_region_slots(registry, "tab_bar", PreparedSlot::into_tab_bar);
    }

    /// Phase 17 host-level lookup: every context-menu item registered
    /// for `region` (e.g. `"repository.diff.context_menu"`), already
    /// sorted ascending by priority then `(plugin_id, id)`.
    /// Renderers reach the live extension registry through this entry
    /// rather than touching the registry directly.
    pub fn extension_context_menu_items(
        &self,
        region: &str,
    ) -> Vec<crate::plugin::extensions::ContextMenuItemRecord> {
        self.extension_registry.context_menu_items(region)
    }

    /// Phase 17 host-level lookup: every graph decoration attached to
    /// `commit_hash`, sorted by `(plugin_id, id)`. Lets the future
    /// graph renderer paint per-commit decorations without iterating
    /// the full registry on every row.
    pub fn extension_graph_decorations_for_commit(
        &self,
        commit_hash: &str,
    ) -> Vec<crate::plugin::extensions::GraphDecorationRecord> {
        self.extension_registry.graph_decorations_for(commit_hash)
    }

    /// Phase 17 host-level helper used by the load/unload pipeline:
    /// drop every overlay / context-menu item / decoration owned by
    /// `plugin_id`. Callers outside the host (e.g. an admin command
    /// that uninstalls a plugin) reach the registry through this
    /// shim so the registry stays a private detail of the host.
    pub fn discard_extensions_for_plugin(&self, plugin_id: &str) {
        self.extension_registry.clear_for_plugin(plugin_id);
    }

    fn apply_region_slots<T: IsSlot>(
        &self,
        registry: &mut SlotRegistry<T>,
        region_name: &str,
        convert: impl Fn(PreparedSlot) -> T,
    ) {
        for op in &self.slot_ops {
            match op {
                PreparedSlotOp::Add(p) if p.region == region_name => {
                    registry.add(convert(p.clone()));
                }
                PreparedSlotOp::Replace {
                    region, id, spec, ..
                } if region == region_name => {
                    if !registry.replace(id, convert(spec.clone())) {
                        self.diagnostics.record(
                            PluginDiagnostic::new(
                                PluginId::from(spec.plugin_id.clone()),
                                DiagnosticSeverity::Warning,
                                "schema.slot_replace_missing",
                                format!(
                                    "regions.replace_slot({region_name}, \"{id}\") \
                                     — no such slot"
                                ),
                            )
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: "leviathan.ui.regions.replace_slot".into(),
                                },
                            ),
                        );
                    }
                }
                PreparedSlotOp::Remove { region, id, .. } if region == region_name => {
                    if !registry.remove(id) {
                        self.diagnostics.record(
                            PluginDiagnostic::new(
                                PluginId::from("<unknown>"),
                                DiagnosticSeverity::Warning,
                                "schema.slot_remove_missing",
                                format!(
                                    "regions.remove_slot({region_name}, \"{id}\") \
                                     — no such slot"
                                ),
                            )
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: "leviathan.ui.regions.remove_slot".into(),
                                },
                            ),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// Invoke a plugin's slot-click handler. Handler is called with
    /// `(slot_id, event, value)` so a single slot can receive multiple
    /// distinct widget events (e.g. a `tablist` fires `select` / `close`
    /// / `reorder`). Silently no-ops if the plugin is gone, the slot has
    /// no handler, or the Lua call errors.
    pub fn dispatch_slot_click(
        &mut self,
        plugin_id: &str,
        region: &str,
        container: &str,
        slot_id: &str,
        event: &str,
        value: serde_json::Value,
    ) {
        let handler_key = format!("{region}:{container}:{slot_id}");
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        let nav: Option<String> = {
            let Some(plugin) = self.plugins.get(plugin_id) else {
                return;
            };
            let Some(key) = plugin.slot_handlers.get(&handler_key) else {
                return;
            };
            let generation_id = plugin.generation.generation_id;
            let func: Function = match plugin.lua().registry_value(key) {
                Ok(f) => f,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("slot handler lookup failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("slot:{handler_key}"),
                        }),
                    );
                    return;
                }
            };
            let value_lua = match plugin.lua().to_value(&value) {
                Ok(v) => v,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.value_conversion_failed",
                            format!("slot value conv failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("slot:{handler_key}"),
                        }),
                    );
                    return;
                }
            };
            match func.call::<Option<Table>>((slot_id.to_string(), event.to_string(), value_lua)) {
                Ok(Some(t)) => t.get::<String>("navigate").ok(),
                Ok(None) => None,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            format!("slot handler error in {handler_key}"),
                        )
                        .with_generation(generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                    None
                }
            }
        };
        if let Some(target) = nav {
            self.open_screen(plugin_id.to_string(), target);
        }
    }
}

/// Wall-clock timestamp (UNIX millis) used by reload-history rows. The
/// history view sorts by this, so a non-monotonic clock just shuffles
/// neighbouring entries — never important enough to switch to
/// `Instant`.
fn build_watch_event_table(
    lua: &Lua,
    event: &crate::plugin::watchers::WatchEvent,
) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("kind", event.kind.as_str())?;
    let paths = lua.create_table()?;
    for (i, p) in event.paths.iter().enumerate() {
        paths.set(i + 1, p.as_str())?;
    }
    t.set("paths", paths)?;
    Ok(t)
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Hash the fields of each ref that plugins can observe via
/// `leviathan.repository`. Ref ordering matters — libgit2 is stable within
/// a run, and a shuffled list with otherwise-identical content would
/// produce the same table anyway. Cheap on typical repos (a few dozen
/// refs); kept self-contained so the host doesn't depend on the
/// projection-layer `ProjectionSignature`.
fn compute_repo_hash(
    repo_name: &str,
    workdir_path: &str,
    current_branch_name: &str,
    head_hash: &str,
    default_remote_name: &str,
    refs: &[RepoRef],
) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut h = DefaultHasher::new();
    (
        repo_name,
        workdir_path,
        current_branch_name,
        head_hash,
        default_remote_name,
        refs,
    )
        .hash(&mut h);
    h.finish()
}

/// One step of the host's autocmd-install replay. Built by
/// [`MergedAutocmdOps`] from the three streams the plugin's init
/// produced (autocmd creates, group creates, group clears) and
/// emitted in plugin-local declaration order.
enum AutocmdOp {
    Group(api::RawAutocmdGroup),
    Clear(api::RawAutocmdClear),
    Create(api::RawAutocmd),
}

/// Iterator that merges autocmd / group / clear ops into a single
/// stream sorted by their plugin-local sequence id.
struct MergedAutocmdOps {
    autocmds: std::vec::IntoIter<api::RawAutocmd>,
    groups: std::vec::IntoIter<api::RawAutocmdGroup>,
    clears: std::vec::IntoIter<api::RawAutocmdClear>,
    next_autocmd: Option<api::RawAutocmd>,
    next_group: Option<api::RawAutocmdGroup>,
    next_clear: Option<api::RawAutocmdClear>,
}

impl MergedAutocmdOps {
    fn new(
        autocmds: Vec<api::RawAutocmd>,
        groups: Vec<api::RawAutocmdGroup>,
        clears: Vec<api::RawAutocmdClear>,
    ) -> Self {
        let mut autocmds = autocmds.into_iter();
        let mut groups = groups.into_iter();
        let mut clears = clears.into_iter();
        let next_autocmd = autocmds.next();
        let next_group = groups.next();
        let next_clear = clears.next();
        Self {
            autocmds,
            groups,
            clears,
            next_autocmd,
            next_group,
            next_clear,
        }
    }
}

impl Iterator for MergedAutocmdOps {
    type Item = AutocmdOp;
    fn next(&mut self) -> Option<Self::Item> {
        // Pick the head with the smallest sequence; backfill from the
        // matching iterator. Streams produced by the Lua API are
        // already in ascending sequence order, so a 3-way head
        // comparison is enough.
        let a = self.next_autocmd.as_ref().map(|x| x.sequence);
        let g = self.next_group.as_ref().map(|x| x.sequence);
        let c = self.next_clear.as_ref().map(|x| x.sequence);
        let mut min: Option<(u64, u8)> = None; // (sequence, lane: 0=a, 1=g, 2=c)
        for (lane, value) in [(0u8, a), (1, g), (2, c)] {
            if let Some(seq) = value {
                if min.map(|(s, _)| seq < s).unwrap_or(true) {
                    min = Some((seq, lane));
                }
            }
        }
        let (_, lane) = min?;
        match lane {
            0 => {
                let item = self.next_autocmd.take()?;
                self.next_autocmd = self.autocmds.next();
                Some(AutocmdOp::Create(item))
            }
            1 => {
                let item = self.next_group.take()?;
                self.next_group = self.groups.next();
                Some(AutocmdOp::Group(item))
            }
            _ => {
                let item = self.next_clear.take()?;
                self.next_clear = self.clears.next();
                Some(AutocmdOp::Clear(item))
            }
        }
    }
}

/// Staged variant of [`AutocmdOp`] used by `commit_staging`. Mirrors
/// the cold-load merger but operates on `StagedAutocmd` instead of
/// `RawAutocmd` (the staged record carries the resolved canonical
/// event name pre-computed during staging).
enum StagedAutocmdOp {
    Group(api::RawAutocmdGroup),
    Clear(api::RawAutocmdClear),
    Create(staged_reload::StagedAutocmd),
}

struct MergedStagedAutocmdOps {
    autocmds: std::vec::IntoIter<staged_reload::StagedAutocmd>,
    groups: std::vec::IntoIter<api::RawAutocmdGroup>,
    clears: std::vec::IntoIter<api::RawAutocmdClear>,
    next_autocmd: Option<staged_reload::StagedAutocmd>,
    next_group: Option<api::RawAutocmdGroup>,
    next_clear: Option<api::RawAutocmdClear>,
}

impl MergedStagedAutocmdOps {
    fn new(
        autocmds: Vec<staged_reload::StagedAutocmd>,
        groups: Vec<api::RawAutocmdGroup>,
        clears: Vec<api::RawAutocmdClear>,
    ) -> Self {
        let mut autocmds = autocmds.into_iter();
        let mut groups = groups.into_iter();
        let mut clears = clears.into_iter();
        let next_autocmd = autocmds.next();
        let next_group = groups.next();
        let next_clear = clears.next();
        Self {
            autocmds,
            groups,
            clears,
            next_autocmd,
            next_group,
            next_clear,
        }
    }
}

impl Iterator for MergedStagedAutocmdOps {
    type Item = StagedAutocmdOp;
    fn next(&mut self) -> Option<Self::Item> {
        let a = self.next_autocmd.as_ref().map(|x| x.sequence);
        let g = self.next_group.as_ref().map(|x| x.sequence);
        let c = self.next_clear.as_ref().map(|x| x.sequence);
        let mut min: Option<(u64, u8)> = None;
        for (lane, value) in [(0u8, a), (1, g), (2, c)] {
            if let Some(seq) = value {
                if min.map(|(s, _)| seq < s).unwrap_or(true) {
                    min = Some((seq, lane));
                }
            }
        }
        let (_, lane) = min?;
        match lane {
            0 => {
                let item = self.next_autocmd.take()?;
                self.next_autocmd = self.autocmds.next();
                Some(StagedAutocmdOp::Create(item))
            }
            1 => {
                let item = self.next_group.take()?;
                self.next_group = self.groups.next();
                Some(StagedAutocmdOp::Group(item))
            }
            _ => {
                let item = self.next_clear.take()?;
                self.next_clear = self.clears.next();
                Some(StagedAutocmdOp::Clear(item))
            }
        }
    }
}

/// Lightweight snapshot of an [`AutocmdEntry`] used when iterating
/// during dispatch. Cloning the few primitive fields up front lets
/// the funnel hand control off to plugin Lua callbacks without
/// holding a borrow on the EventBus across re-entrant dispatch.
struct EntrySnapshot {
    id: u64,
    plugin_id: String,
    generation_id: GenerationId,
    once: bool,
    debounce_ms: u64,
    pattern_present: bool,
    matches_pattern: bool,
    last_fire_clock_ms: Option<u64>,
    disabled: bool,
}

/// Build the typed payload table the Lua callback receives. Mirrors
/// the descriptor's `LeviathanAutocmdEvent` shape. Empty payload
/// tables still surface as `{}` so the callback signature stays
/// uniform across every Phase 7 event.
fn build_payload_table(
    lua: &Lua,
    canonical: &'static event_descriptor::ApiEvent,
    alias_used: Option<&'static str>,
    payload: &EventPayload,
) -> mlua::Result<Table> {
    let outer = lua.create_table()?;
    outer.set("event", canonical.name)?;
    if let Some(alias) = alias_used {
        outer.set("alias", alias)?;
    }
    let payload_value: serde_json::Value = serde_json::Value::Object(payload.clone());
    let payload_lua: LuaValue = lua.to_value(&payload_value)?;
    outer.set("payload", payload_lua)?;
    Ok(outer)
}

/// Build a [`PluginSourceSpan::Lua`] from the optional registration
/// site recorded by the resource ledger. The plugin's chunk name
/// drives the `file` field; `line` is parsed out of the `:N:` infix
/// when present so devtools shows the autocmd registration call site.
fn make_lua_span(plugin_id: &str, source_location: Option<&str>) -> PluginSourceSpan {
    let file = format!("plugins/{plugin_id}/init.lua");
    let line = source_location.and_then(|raw| {
        let bytes = raw.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b':' {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                if end > start {
                    if let Ok(n) = raw[start..end].parse::<u32>() {
                        return Some(n);
                    }
                }
            }
            i += 1;
        }
        None
    });
    PluginSourceSpan::Lua {
        file,
        line,
        traceback: source_location.map(str::to_string),
    }
}

fn record_screen_state_resource(
    ledger: &ResourceLedger,
    screen_id: &str,
    source_location: Option<String>,
) {
    let handle = format!("screen_state:{screen_id}");
    ledger.remove_by_kind_handle(PluginResourceKind::PersistedScreenState, &handle);
    ledger.remove_by_kind_handle(PluginResourceKind::LuaRegistryKey, &handle);
    ledger.record(
        PluginResourceKind::PersistedScreenState,
        handle.clone(),
        source_location.clone(),
    );
    ledger.record(PluginResourceKind::LuaRegistryKey, handle, source_location);
}

/// True when this slot op was registered by `plugin_id`. Used by
/// `reload_plugin` to park / restore plugin-owned host state.
///
/// `Remove` carries no plugin id; it's considered "host-owned" and
/// never parked. Practical consequence: a plugin's own removals against
/// its slots aren't moved on reload, but since the corresponding `Add`
/// is parked alongside, the resulting state is consistent.
fn op_belongs_to(op: &PreparedSlotOp, plugin_id: &str) -> bool {
    match op {
        PreparedSlotOp::Add(p) => p.plugin_id == plugin_id,
        PreparedSlotOp::Replace { spec, .. } => spec.plugin_id == plugin_id,
        PreparedSlotOp::Remove { .. } => false,
    }
}

fn prepared_slot_handle(region: &str, container: &str, id: &str) -> String {
    format!("{region}:{container}:{id}")
}

fn op_matches_slot_resource(op: &PreparedSlotOp, plugin_id: &str, handle: &str) -> bool {
    match op {
        PreparedSlotOp::Add(p) => {
            p.plugin_id == plugin_id
                && prepared_slot_handle(&p.region, &p.container.key(), &p.id) == handle
        }
        PreparedSlotOp::Replace {
            region,
            container,
            id,
            spec,
        } => {
            spec.plugin_id == plugin_id
                && prepared_slot_handle(region, &container.key(), id) == handle
        }
        PreparedSlotOp::Remove { .. } => false,
    }
}

pub(crate) fn prepare_op(
    plugin_id: &str,
    plugin_root: &Path,
    op: RawSlotOp,
    ledger: &ResourceLedger,
    handlers: &mut HashMap<String, RegistryKey>,
    dynamic_widgets: &mut HashMap<String, (RegistryKey, DynamicAstCache)>,
) -> Result<PreparedSlotOp, String> {
    match op {
        RawSlotOp::Add(raw) => {
            let prepared = prepare_slot(
                plugin_id,
                plugin_root,
                raw,
                ledger,
                handlers,
                dynamic_widgets,
            )?;
            Ok(PreparedSlotOp::Add(prepared))
        }
        RawSlotOp::Remove {
            region,
            container,
            id,
        } => Ok(PreparedSlotOp::Remove {
            region,
            container: parse_container(&container),
            id,
        }),
        RawSlotOp::Replace {
            region,
            container,
            id,
            spec,
        } => {
            let prepared = prepare_slot(
                plugin_id,
                plugin_root,
                spec,
                ledger,
                handlers,
                dynamic_widgets,
            )?;
            Ok(PreparedSlotOp::Replace {
                region,
                container: parse_container(&container),
                id,
                spec: prepared,
            })
        }
    }
}

fn prepare_slot(
    plugin_id: &str,
    plugin_root: &Path,
    raw: RawSlotSpec,
    ledger: &ResourceLedger,
    handlers: &mut HashMap<String, RegistryKey>,
    dynamic_widgets: &mut HashMap<String, (RegistryKey, DynamicAstCache)>,
) -> Result<PreparedSlot, String> {
    let RawSlotSpec {
        id,
        region,
        container,
        priority,
        widget,
        on_click,
        source_location,
    } = raw;
    let container_parsed = parse_container(&container);
    if let Some(key) = on_click {
        let handler_key = format!("{region}:{}:{id}", container_parsed.key());
        handlers.insert(handler_key, key);
    }
    let widget = match widget {
        WidgetSource::Static(ast) => SlotWidget::Static(ast),
        WidgetSource::Dynamic(key) => {
            let cache: DynamicAstCache = Rc::new(RefCell::new(None));
            dynamic_widgets.insert(id.clone(), (key, Rc::clone(&cache)));
            ledger.record(
                PluginResourceKind::DynamicWidgetCache,
                format!("{region}:{}:{id}", container_parsed.key()),
                source_location.clone(),
            );
            SlotWidget::Dynamic(cache)
        }
    };
    Ok(PreparedSlot {
        plugin_id: plugin_id.to_string(),
        id,
        region,
        container: container_parsed,
        priority,
        widget,
        plugin_root: plugin_root.to_path_buf(),
    })
}

// Extra accessor/dispatch methods. Kept in a second `impl` block to
// isolate the new hook-system additions above from the original API.
impl PluginHost {
    /// Sandbox root for a plugin's bundled assets. Returns `None` if the
    /// plugin id is unknown.
    pub fn plugin_root(&self, plugin_id: &str) -> Option<&Path> {
        self.plugins.get(plugin_id).map(|p| p.root.as_path())
    }

    pub fn active_screen(&self) -> Option<(&str, &str)> {
        self.active_screen
            .as_ref()
            .map(|(a, b)| (a.as_str(), b.as_str()))
    }

    pub fn widget_tree(&self) -> Option<&WidgetAst> {
        self.widget_tree.as_ref()
    }

    pub fn split_sizes(&self) -> &HashMap<String, Vec<f32>> {
        &self.split_sizes
    }

    pub fn dispatch_event(
        &mut self,
        plugin_id: &str,
        screen_id: &str,
        event: &str,
        value: serde_json::Value,
    ) {
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        let mut pending_diagnostics: Vec<PluginDiagnostic> = Vec::new();
        let nav: Option<String>;
        {
            let Some(plugin) = self.plugins.get_mut(plugin_id) else {
                return;
            };
            let Some(screen_def) = plugin.screens.get(screen_id) else {
                return;
            };
            let generation_id = plugin.generation.generation_id;
            let update_fn: Function = match plugin.lua().registry_value(&screen_def.update) {
                Ok(f) => f,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("screen update lookup failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("screen:{screen_id}.update"),
                        }),
                    );
                    return;
                }
            };
            let state_val: LuaValue = match plugin.screen_state.get(screen_id) {
                Some(k) => plugin.lua().registry_value(k).unwrap_or(LuaValue::Nil),
                None => LuaValue::Nil,
            };
            let value_lua = match plugin.lua().to_value(&value) {
                Ok(v) => v,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.value_conversion_failed",
                            format!("event value conv failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("screen:{screen_id}.update"),
                        }),
                    );
                    return;
                }
            };
            let result: mlua::Result<Table> =
                update_fn.call((state_val, event.to_string(), value_lua));
            match result {
                Ok(action) => {
                    if let Ok(new_state) = action.get::<LuaValue>("state") {
                        if !matches!(new_state, LuaValue::Nil) {
                            if let Ok(key) = plugin.lua().create_registry_value(new_state) {
                                record_screen_state_resource(&plugin.ledger(), screen_id, None);
                                plugin.screen_state.insert(screen_id.to_string(), key);
                            }
                        }
                    }
                    nav = action.get::<String>("navigate").ok();
                }
                Err(e) => {
                    pending_diagnostics.push(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            format!("screen update error in {screen_id}"),
                        )
                        .with_generation(generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                    nav = None;
                }
            }
        }
        for diag in pending_diagnostics {
            self.diagnostics.record(diag);
        }
        if let Some(target) = nav {
            if target == "repository" || target == "back" {
                self.active_screen = None;
                self.widget_tree = None;
            } else {
                self.open_screen(plugin_id.to_string(), target);
                return;
            }
        }
        self.refresh_active_widget_tree();
    }

    /// Snapshot of a screen's current Lua state as JSON. Returns `None`
    /// when the plugin is unloaded, the screen has no state yet, or the
    /// state value can't be serialised.
    pub fn screen_state_json(&self, plugin_id: &str, screen_id: &str) -> Option<serde_json::Value> {
        let plugin = self.plugins.get(plugin_id)?;
        let key = plugin.screen_state.get(screen_id)?;
        let v: LuaValue = plugin.lua().registry_value(key).ok()?;
        plugin.lua().from_value(v).ok()
    }

    /// Read a top-level Lua global from `plugin_id`'s VM as `i64`. Used
    /// by tests to observe side-effects of plugin code (e.g. results of
    /// `leviathan.services.get(...).method(...)`). Returns `None` when
    /// the plugin is unknown, the global doesn't exist, or the value is
    /// neither integer nor number.
    pub fn plugin_global_i64(&self, plugin_id: &str, name: &str) -> Option<i64> {
        let plugin = self.plugins.get(plugin_id)?;
        let v: LuaValue = plugin.lua().globals().get(name).ok()?;
        match v {
            LuaValue::Integer(i) => Some(i),
            LuaValue::Number(n) => Some(n as i64),
            _ => None,
        }
    }

    /// String counterpart of `plugin_global_i64`. Used by Phase 5
    /// tests that read order-tracking strings out of `_G`.
    pub fn plugin_global_string(&self, plugin_id: &str, name: &str) -> Option<String> {
        let plugin = self.plugins.get(plugin_id)?;
        let v: LuaValue = plugin.lua().globals().get(name).ok()?;
        match v {
            LuaValue::String(s) => s.to_str().ok().map(|c| c.to_string()),
            _ => None,
        }
    }

    /// Returns the last error from a failed `reload_plugin` for this
    /// plugin id, if any. Cleared by a subsequent successful reload.
    pub fn last_reload_error(&self, plugin_id: &str) -> Option<&str> {
        self.last_reload_errors.get(plugin_id).map(String::as_str)
    }

    /// True when the plugin currently owns a slot at
    /// `(region, container, slot_id)`. Walks `slot_ops` in reverse so a
    /// later `Remove` shadows an earlier `Add`.
    pub fn has_slot(&self, plugin_id: &str, region: &str, container: &str, slot_id: &str) -> bool {
        for op in self.slot_ops.iter().rev() {
            match op {
                PreparedSlotOp::Add(p)
                    if p.plugin_id == plugin_id
                        && p.region == region
                        && p.container.key() == container
                        && p.id == slot_id =>
                {
                    return true;
                }
                PreparedSlotOp::Replace {
                    region: r,
                    container: c,
                    id,
                    spec,
                } if spec.plugin_id == plugin_id
                    && r == region
                    && c.key() == container
                    && id == slot_id =>
                {
                    return true;
                }
                PreparedSlotOp::Remove {
                    region: r,
                    container: c,
                    id,
                } if r == region && c.key() == container && id == slot_id => {
                    return false;
                }
                _ => {}
            }
        }
        false
    }

    pub fn unload_plugin(
        &mut self,
        plugin_id: &str,
    ) -> Result<(), git_leviathan_plugin_api::error::PluginError> {
        // Phase 16: a plugin may live in the lazy registry without
        // a Lua state. Reap its stub ledger and remove the entry.
        let in_lazy = self
            .lazy_registry
            .entries()
            .iter()
            .any(|e| e.plugin_id == plugin_id);
        if !self.plugins.contains_key(plugin_id) && in_lazy {
            if let Some(ledger) = self.lazy_ledgers.remove(plugin_id) {
                self.cleanup_ledger(&ledger);
            }
            self.lazy_registry
                .entries_mut()
                .retain(|e| e.plugin_id != plugin_id);
            return Ok(());
        }
        let mut plugin = self.plugins.remove(plugin_id).ok_or_else(|| {
            git_leviathan_plugin_api::error::PluginError::new(
                plugin_id,
                "host.unload_plugin",
                "plugin not loaded",
            )
        })?;
        // If the plugin had been activated lazily, drop the active
        // entry too so a later `load_plugin` doesn't see a stale
        // `Active` row.
        self.lazy_registry
            .entries_mut()
            .retain(|e| e.plugin_id != plugin_id);
        plugin.generation.mark_unloading();
        // Phase 12: cancel every async resource owned by this plugin's
        // current generation. Worker threads observe their cancellation
        // tokens; timer / watcher records drop immediately so no further
        // tick fires their callbacks.
        let pid_typed = plugin.generation.plugin_id.clone();
        let gen_id = plugin.generation.generation_id;
        self.async_jobs.cancel_for_generation(&pid_typed, gen_id);
        self.timers.cancel_for_generation(&pid_typed, gen_id);
        self.watchers.cancel_for_generation(&pid_typed, gen_id);
        // Phase 8: drop every command this plugin owned (across all
        // generations — defensive, mirroring `EventBus::drop_for_plugin`)
        // and release the Lua callback registry keys back to the
        // owning state before the state itself is dropped.
        let removed = self
            .command_registry
            .borrow_mut()
            .drop_for_plugin(plugin_id);
        commands::release_lua_keys(removed, plugin.lua());
        self.command_plugin_registry.remove(plugin_id);
        // Phase 9: drop every keymap this plugin owned. The resolver
        // re-runs inside `drop_for_plugin`, so any conflict-loser
        // bindings from peers automatically reactivate.
        self.keymap_registry.borrow_mut().drop_for_plugin(plugin_id);
        self.cleanup_ledger(&plugin.generation.ledger);
        // Phase 17: drop overlays / context-menu items / decorations
        // owned by this plugin. Records came from a non-Slot ledger
        // path so the cleaner above didn't touch the registry.
        self.extension_registry.clear_for_plugin(plugin_id);
        // Phase 18: drop every breaker / trace row tied to this plugin
        // so a future load starts with a clean slate.
        self.budget_tracker.drop_for_plugin(plugin_id);
        if self
            .active_screen
            .as_ref()
            .map(|(pid, _)| pid == plugin_id)
            .unwrap_or(false)
        {
            self.active_screen = None;
            self.widget_tree = None;
        }
        let split_prefix = format!("{plugin_id}:");
        self.split_sizes
            .retain(|key, _| !key.starts_with(&split_prefix));
        if self
            .split_drag
            .as_ref()
            .map(|drag| drag.split_key.starts_with(&split_prefix))
            .unwrap_or(false)
        {
            self.split_drag = None;
        }
        self.last_reload_errors.remove(plugin_id);
        self.runtime_path_registry.unregister(plugin_id);
        drop(plugin);
        Ok(())
    }

    /// Phase 6 staged reload. Builds a new generation in isolation,
    /// validates it, runs the optional `M.reload(old_state)` migration
    /// hook, and only then atomically swaps it for the previous
    /// generation. Any failure between stages aborts cleanly: the
    /// previous generation stays live, a structured `reload.failed`
    /// diagnostic carries the failing stage and cause, and the staging
    /// generation is dropped in full (its ledger, lua state, after-
    /// files, services, and runtime-path entry).
    ///
    /// On success the previous generation's ledger is drained through
    /// the host's standard `cleanup_ledger` path so every resource keyed
    /// to the old `(plugin_id, generation_id)` pair is gone.
    pub fn reload_plugin(
        &mut self,
        plugin_id: &str,
    ) -> Result<(), git_leviathan_plugin_api::error::PluginError> {
        let plugin = self.plugins.get(plugin_id).ok_or_else(|| {
            git_leviathan_plugin_api::error::PluginError::new(
                plugin_id,
                "host.reload_plugin",
                "plugin not loaded",
            )
        })?;
        let plugin_id_owned = plugin_id.to_string();
        let dir = plugin.root.clone();
        let previous_generation_id = plugin.generation.generation_id;
        let previous_capabilities: Vec<String> = plugin
            .manifest
            .capabilities
            .iter()
            .map(|c| String::from(c.clone()))
            .collect();
        let previous_screen_ids: Vec<String> = plugin.screen_state.keys().cloned().collect();
        let bundled = self.auto_grant_policy.is_bundled(&dir);

        let previous_screens = snapshot_previous_screens(
            plugin_id,
            plugin.lua(),
            &plugin.screens,
            &plugin.screen_state,
        );

        let plugin_id_typed = PluginId::from(plugin_id);
        let staging_generation_id = self.allocate_generation_id(plugin_id);

        let stage_inputs = StageInputs {
            plugin_dir: dir,
            plugin_id: plugin_id_typed.clone(),
            generation_id: staging_generation_id,
            diagnostics: self.diagnostics.clone(),
            audit_log: self.audit_log.clone(),
            runtime_path_registry: &self.runtime_path_registry,
            previous_generation_id,
            previous_screens,
            previous_capabilities,
            previous_screen_ids,
            pending_tab_ops: Rc::clone(&self.pending_tab_ops),
            command_dispatch: self.command_dispatch_env(),
            keymap_registry: Rc::clone(&self.keymap_registry),
            grant_store: self.grant_store.clone(),
            bundled,
            git_ctx: self.git_ops_context(),
            pending_git_events: self.pending_git_events.clone(),
            async_jobs: self.async_jobs.clone(),
            timers: self.timers.clone(),
            watchers: self.watchers.clone(),
            storage_roots: self.storage_roots.clone(),
            service_registry: Rc::clone(&self.service_registry),
            extension_registry: self.extension_registry.clone(),
        };

        match stage_reload(stage_inputs) {
            Ok(artifacts) => {
                let duration_ms = artifacts.started_at.elapsed().as_millis();
                self.commit_staging(artifacts);
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        plugin_id_typed.clone(),
                        DiagnosticSeverity::Info,
                        "reload.completed",
                        format!(
                            "reload completed: gen {} -> {}",
                            previous_generation_id, staging_generation_id
                        ),
                    )
                    .with_generation(staging_generation_id)
                    .with_context(serde_json::json!({
                        "duration_ms": duration_ms,
                        "previous_generation_id": previous_generation_id.get(),
                    })),
                );
                self.last_reload_errors.remove(plugin_id);
                self.record_reload_event(ReloadEventSummary {
                    plugin_id: plugin_id_owned,
                    from_generation_id: previous_generation_id.get(),
                    to_generation_id: Some(staging_generation_id.get()),
                    outcome: ReloadOutcome::Succeeded,
                    duration_ms,
                    stage_reached: ReloadStage::Swap.as_str().to_string(),
                    error_code: None,
                    error_message: None,
                    timestamp_unix_ms: now_unix_ms(),
                });
                self.refresh_active_widget_tree();
                Ok(())
            }
            Err(failure) => {
                let StagingFailure {
                    stage,
                    cause,
                    message,
                } = failure;
                let summary_message = message.clone();
                let summary_cause = cause.clone();
                self.last_reload_errors
                    .insert(plugin_id_owned.clone(), summary_message.clone());
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        plugin_id_typed,
                        DiagnosticSeverity::Error,
                        "reload.failed",
                        format!(
                            "reload failed at stage {}; previous generation kept active",
                            stage.as_str()
                        ),
                    )
                    .with_generation(previous_generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: "host.reload_plugin".into(),
                    })
                    .with_context(serde_json::json!({
                        "stage": stage.as_str(),
                        "cause": summary_cause,
                        "message": summary_message,
                        "staging_generation_id": staging_generation_id.get(),
                    })),
                );
                self.record_reload_event(ReloadEventSummary {
                    plugin_id: plugin_id_owned.clone(),
                    from_generation_id: previous_generation_id.get(),
                    to_generation_id: None,
                    outcome: ReloadOutcome::Failed,
                    duration_ms: 0,
                    stage_reached: stage.as_str().to_string(),
                    error_code: Some(summary_cause),
                    error_message: Some(summary_message.clone()),
                    timestamp_unix_ms: now_unix_ms(),
                });
                Err(git_leviathan_plugin_api::error::PluginError::new(
                    plugin_id,
                    "host.reload_plugin",
                    format!("reload failed at {}: {}", stage.as_str(), summary_message),
                ))
            }
        }
    }

    /// Atomic swap. Drains the previous generation's resources (slot
    /// ops, autocmds, services, runtime-path entry, ledger) and
    /// installs the staged generation in their place. Called only by
    /// `reload_plugin` on a successful staging build; never invoked
    /// when staging failed (the artefacts drop and clean up on their
    /// own).
    fn commit_staging(&mut self, artifacts: StagingArtifacts) {
        let StagingArtifacts {
            plugin_id,
            generation_id,
            plugin_root,
            manifest,
            generation,
            slot_handlers,
            screens,
            screen_state,
            dynamic_widgets,
            deferred,
            user_commands,
            health_checks,
            lua_loader,
            slot_ops: staged_slot_ops,
            autocmds: staged_autocmds,
            autocmd_groups: staged_autocmd_groups,
            autocmd_clears: staged_autocmd_clears,
            staged_services,
            started_at: _,
            commands: staged_commands,
            capability_guard: staged_capability_guard,
            keymaps: staged_keymaps,
            keymap_dels: staged_keymap_dels,
            job_callbacks: staged_job_callbacks,
            timer_callbacks: staged_timer_callbacks,
            watcher_callbacks: staged_watcher_callbacks,
        } = artifacts;

        let plugin_id_str = plugin_id.as_str().to_string();

        // Remove the previous generation from every host-level table
        // before splicing the new one in. RegistryKeys do not implement
        // Clone, so we drain entries by index.
        if let Some(mut previous) = self.plugins.remove(&plugin_id_str) {
            previous.generation.mark_unloading();
            // Phase 12: cancel previous generation's async-runtime
            // resources so old-gen jobs/timers/watchers don't keep
            // firing against the soon-to-be-dropped Lua state.
            let prev_pid = previous.generation.plugin_id.clone();
            let prev_gen = previous.generation.generation_id;
            self.async_jobs.cancel_for_generation(&prev_pid, prev_gen);
            self.timers.cancel_for_generation(&prev_pid, prev_gen);
            self.watchers.cancel_for_generation(&prev_pid, prev_gen);
            // Phase 8: drop every command this plugin owned (across
            // all prior generations) and release the Lua callback
            // keys against the *previous* state — the staged
            // generation will install fresh keys on the new state
            // below.
            let removed = self
                .command_registry
                .borrow_mut()
                .drop_for_plugin(&plugin_id_str);
            commands::release_lua_keys(removed, previous.lua());
            self.command_plugin_registry.remove(&plugin_id_str);
            // Phase 9: drop every keymap the previous generation
            // owned. The staged generation's `set` rows install
            // below.
            self.keymap_registry
                .borrow_mut()
                .drop_for_plugin(&plugin_id_str);
            self.cleanup_ledger(&previous.generation.ledger);
            // Phase 17: drop overlays / context-menu items / decorations
            // owned by the previous generation. The staged init.lua
            // re-registers anything the new generation needs.
            self.extension_registry.clear_for_plugin(&plugin_id_str);
            // Phase 18: clear breaker state for the previous generation
            // so a fixed plugin starts clean. Earlier generations'
            // traces stay visible in devtools because we key by
            // generation id; only the previous-gen rows are dropped.
            self.budget_tracker
                .drop_for_generation(&plugin_id_str, prev_gen);
            // After cleanup_ledger ran, the ledger is empty and host
            // tables (slot_ops, event_bus, services) have already had
            // every resource keyed to the previous gen removed (the
            // ledger drives that). Drop the rest of the previous
            // plugin: dynamic widget caches, screen state keys, etc.
            drop(previous);
        }
        // Defensive sweep: remove any straggler slot_ops/autocmds for
        // this plugin id that the ledger walk did not own (e.g. raw
        // `Remove` ops, which are host-owned and not ledger-tracked).
        // Safe even when the cleanup already emptied them.
        self.slot_ops
            .retain(|op| !op_belongs_to(op, &plugin_id_str));
        // Drop *every* prior autocmd row for this plugin id regardless
        // of generation; the ledger walk already removed the previous
        // gen's, this catches any residue.
        let _ = self.event_bus.drop_for_plugin(&plugin_id_str);

        // Splice staged ops into the live host tables.
        for op in staged_slot_ops {
            self.slot_ops.push(op);
        }
        // Splice staged autocmd group handles + clears + entries into
        // the live `EventBus` in plugin-local declaration order.
        // Group handles are minted per-plugin so a fresh generation
        // always sees a fresh handle namespace.
        let mut local_to_host: HashMap<u64, GroupId> = HashMap::new();
        let mut staged_walk = MergedStagedAutocmdOps::new(
            staged_autocmds,
            staged_autocmd_groups,
            staged_autocmd_clears,
        );
        while let Some(op) = staged_walk.next() {
            match op {
                StagedAutocmdOp::Group(grp) => {
                    let host_id = self
                        .event_bus
                        .group_handle(&plugin_id_str, &grp.name, grp.clear);
                    local_to_host.insert(grp.local_id, host_id);
                }
                StagedAutocmdOp::Clear(clear) => {
                    if let Some(host_id) = local_to_host.get(&clear.local_id).copied() {
                        self.event_bus.clear_group(&plugin_id_str, host_id);
                    }
                }
                StagedAutocmdOp::Create(staged) => {
                    let options = AutocmdOptions {
                        group: staged
                            .options_local_group
                            .and_then(|local| local_to_host.get(&local).copied()),
                        once: staged.once,
                        pattern: staged.pattern,
                        debounce_ms: staged.debounce_ms,
                        priority: staged.priority,
                        source_location: staged.source_location,
                    };
                    self.event_bus.register(
                        plugin_id_str.clone(),
                        generation_id,
                        staged.subscribed_event,
                        staged.canonical_event,
                        staged.callback,
                        options,
                    );
                }
            }
        }
        // `register` initialises `AutocmdRuntime::default()`, so the
        // staged autocmds always start with clean failure counters.
        // The previous generation's disabled rows were dropped by
        // `cleanup_ledger` + the defensive `drop_for_plugin` sweep
        // above, satisfying Phase 7's "re-enabled on plugin reload"
        // requirement.

        // Transfer staged services into the live registry. Old-gen
        // service handles were removed by `cleanup_ledger` above.
        let drained = staged_reload::drain_staged_services(&staged_services, &plugin_id_str);
        self.service_registry.borrow_mut().restore_handles(drained);

        // Phase 8: insert the new generation's lua + capabilities into
        // the dispatcher's plugin registry, then install the staged
        // commands. Any command rejected by `build_descriptor` lands
        // as a `command.invalid_args` diagnostic the same way it
        // would on cold load — the swap itself doesn't fail because
        // of a single bad command (mirrors the autocmd path).
        self.command_plugin_registry.insert(
            plugin_id_str.clone(),
            CommandPluginContext {
                lua: Rc::clone(&generation.lua),
                capability_guard: staged_capability_guard,
                generation_id,
            },
        );
        self.install_raw_commands(&plugin_id_str, generation_id, staged_commands);

        // Phase 9: install staged keymaps. Bad rows surface as
        // `keymap.invalid_key` diagnostics — the swap itself doesn't
        // fail because of one bad binding (mirrors commands /
        // autocmds). Apply staged `del`s after the `set`s so a plugin
        // can rebind its own keymaps in one init.lua run.
        self.keymap_registry.borrow_mut().install_plugin_raws(
            &plugin_id_str,
            generation_id,
            staged_keymaps,
            &self.diagnostics,
        );
        self.keymap_registry.borrow_mut().apply_plugin_dels(
            &plugin_id_str,
            generation_id,
            staged_keymap_dels,
        );

        // Now expose the new plugin's root in the runtime-path
        // registry so dependents resolve `require("<id>.module")`
        // against this generation's `lua/` directory.
        self.runtime_path_registry
            .register(plugin_id_str.clone(), plugin_root.clone());

        // If the active screen belonged to this plugin, drop it when
        // the screen no longer exists (the staged generation may have
        // removed or renamed it). Otherwise the existing
        // `(plugin_id, screen_id)` is still valid because the staged
        // gen carries its `screens` map with the same id.
        if let Some((active_pid, active_sid)) = self.active_screen.clone() {
            if active_pid == plugin_id_str && !screens.contains_key(&active_sid) {
                self.active_screen = None;
                self.widget_tree = None;
            }
        }

        let plugin = LoadedPlugin {
            generation,
            root: plugin_root,
            manifest,
            slot_handlers,
            screens,
            screen_state,
            dynamic_widgets,
            deferred,
            user_commands,
            health_checks,
            lua_loader,
            job_callbacks: staged_job_callbacks,
            timer_callbacks: staged_timer_callbacks,
            watcher_callbacks: staged_watcher_callbacks,
        };
        self.plugins.insert(plugin_id_str.clone(), plugin);
        let _ = generation_id; // recorded on the inserted plugin's ledger
    }

    pub fn open_screen(&mut self, plugin_id: String, screen_id: String) {
        let needs_init = self
            .plugins
            .get(&plugin_id)
            .map(|p| p.screens.contains_key(&screen_id) && !p.screen_state.contains_key(&screen_id))
            .unwrap_or(false);
        if needs_init {
            let chunk_name = format!("plugins/{plugin_id}/init.lua");
            let mut pending: Vec<PluginDiagnostic> = Vec::new();
            if let Some(plugin) = self.plugins.get_mut(&plugin_id) {
                if let Some(screen_def) = plugin.screens.get(&screen_id) {
                    let generation_id = plugin.generation.generation_id;
                    match plugin.lua().registry_value::<Function>(&screen_def.init) {
                        Ok(init_fn) => match init_fn.call::<LuaValue>(()) {
                            Ok(state) => {
                                if let Ok(key) = plugin.lua().create_registry_value(state) {
                                    record_screen_state_resource(
                                        &plugin.ledger(),
                                        &screen_id,
                                        None,
                                    );
                                    plugin.screen_state.insert(screen_id.clone(), key);
                                }
                            }
                            Err(e) => pending.push(
                                PluginDiagnostic::new(
                                    PluginId::from(plugin_id.clone()),
                                    DiagnosticSeverity::Error,
                                    "lua.callback_error",
                                    format!("screen init error in {screen_id}"),
                                )
                                .with_generation(generation_id)
                                .with_mlua_error(&chunk_name, &e),
                            ),
                        },
                        Err(e) => pending.push(
                            PluginDiagnostic::new(
                                PluginId::from(plugin_id.clone()),
                                DiagnosticSeverity::Error,
                                "lua.handler_lookup_failed",
                                format!("screen init lookup failed: {e}"),
                            )
                            .with_generation(generation_id)
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: format!("screen:{screen_id}.init"),
                                },
                            ),
                        ),
                    }
                }
            }
            for diag in pending {
                self.diagnostics.record(diag);
            }
        }
        self.active_screen = Some((plugin_id, screen_id));
        self.refresh_active_widget_tree();
    }

    fn refresh_active_widget_tree(&mut self) {
        let Some((plugin_id, screen_id)) = self.active_screen.clone() else {
            self.widget_tree = None;
            return;
        };
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        let mut pending: Vec<PluginDiagnostic> = Vec::new();
        let mut ok_tree: Option<WidgetAst> = None;
        {
            let Some(plugin) = self.plugins.get(&plugin_id) else {
                self.widget_tree = None;
                return;
            };
            let Some(screen_def) = plugin.screens.get(&screen_id) else {
                self.widget_tree = None;
                return;
            };
            let generation_id = plugin.generation.generation_id;
            let widget_path_root = format!("screen.{screen_id}.view");
            match plugin.lua().registry_value::<Function>(&screen_def.view) {
                Ok(view_fn) => {
                    let state_val: LuaValue = match plugin.screen_state.get(&screen_id) {
                        Some(k) => plugin.lua().registry_value(k).unwrap_or(LuaValue::Nil),
                        None => LuaValue::Nil,
                    };
                    let result: mlua::Result<LuaValue> = view_fn.call(state_val);
                    match result {
                        Ok(v) => {
                            let json: Result<serde_json::Value, mlua::Error> =
                                plugin.lua().from_value(v);
                            match json {
                                Ok(tree) => match widget_ast::decode(&tree) {
                                    Ok(ast) => ok_tree = Some(ast),
                                    Err(decode_err) => pending.push(widget_decode_diagnostic(
                                        &plugin_id,
                                        generation_id,
                                        &widget_path_root,
                                        &decode_err,
                                    )),
                                },
                                Err(e) => pending.push(
                                    PluginDiagnostic::new(
                                        PluginId::from(plugin_id.clone()),
                                        DiagnosticSeverity::Error,
                                        "widget.invalid_tree",
                                        format!("view returned non-serialisable tree: {e}"),
                                    )
                                    .with_generation(generation_id)
                                    .with_source(
                                        PluginSourceSpan::Widget {
                                            path: widget_path_root.clone(),
                                        },
                                    ),
                                ),
                            }
                        }
                        Err(e) => pending.push(
                            PluginDiagnostic::new(
                                PluginId::from(plugin_id.clone()),
                                DiagnosticSeverity::Error,
                                "lua.callback_error",
                                format!("view call failed for screen {screen_id}"),
                            )
                            .with_generation(generation_id)
                            .with_mlua_error(&chunk_name, &e),
                        ),
                    }
                }
                Err(e) => pending.push(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id.clone()),
                        DiagnosticSeverity::Error,
                        "lua.handler_lookup_failed",
                        format!("view lookup failed: {e}"),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("screen:{screen_id}.view"),
                    }),
                ),
            }
        }
        for diag in pending {
            self.diagnostics.record(diag);
        }
        match ok_tree {
            Some(tree) => self.widget_tree = Some(tree),
            None => self.widget_tree = None,
        }
    }

    pub fn split_drag_begin(
        &mut self,
        split_key: &str,
        divider_index: usize,
        child_count: usize,
        is_vertical: bool,
        limits: Vec<(f32, f32)>,
    ) {
        // Sizes now tracked one per child (not n-1) so the last panel also
        // clamps via its own limits.
        let default_len = child_count.max(1);
        let sizes = self
            .split_sizes
            .get(split_key)
            .cloned()
            .unwrap_or_else(|| vec![split::DEFAULT_PANEL_SIZE; default_len]);
        self.split_drag = Some(SplitDragInfo {
            split_key: split_key.to_string(),
            divider_index,
            initial_sizes: sizes.clone(),
            initial_pointer: None,
            is_vertical,
            limits,
        });
        self.split_sizes
            .entry(split_key.to_string())
            .or_insert(sizes);
    }

    pub fn split_drag_moved(&mut self, x: f32, y: f32) {
        let (key, divider_index, initial_sizes, delta, limits) = {
            let Some(drag) = self.split_drag.as_mut() else {
                return;
            };
            let pointer_pos = if drag.is_vertical { y } else { x };
            let initial_pointer = *drag.initial_pointer.get_or_insert(pointer_pos);
            let delta = pointer_pos - initial_pointer;
            (
                drag.split_key.clone(),
                drag.divider_index,
                drag.initial_sizes.clone(),
                delta,
                drag.limits.clone(),
            )
        };
        let new_sizes = split::apply_drag_delta(&initial_sizes, divider_index, delta, &limits);
        self.split_sizes.insert(key, new_sizes);
    }

    pub fn split_drag_released(&mut self) {
        self.split_drag = None;
    }

    pub fn is_dragging_split(&self) -> bool {
        self.split_drag.is_some()
    }

    pub fn active_drag(&self) -> Option<(&str, usize)> {
        self.split_drag
            .as_ref()
            .map(|d| (d.split_key.as_str(), d.divider_index))
    }

    /// Fire a host-side event with no payload.
    /// Equivalent to `fire_event_typed(event, {})`.
    pub fn fire_event(&mut self, event: &str) {
        self.fire_event_typed(event, EventPayload::new());
    }

    /// Fire a host-side typed event. The funnel:
    /// 1. resolves the event name against the descriptor table;
    /// 2. validates the payload shape against the descriptor (records
    ///    `autocmd.payload_mismatch` for shape errors but proceeds);
    /// 3. invokes every matching autocmd in (priority desc, id asc)
    ///    order, honouring `pattern`, `debounce_ms`, `once`, and the
    ///    consecutive-failure disable threshold;
    /// 4. refreshes dynamic widgets for every plugin whose callbacks
    ///    actually ran.
    pub fn fire_event_typed(&mut self, event: &str, payload: EventPayload) {
        // Phase 16: probe the lazy registry for a plugin that
        // declared this event as an activation trigger. Activation
        // re-fires the event through this same funnel so the now-live
        // autocmds observe it.
        if let Ok((canonical, _)) = events::resolve_event(event) {
            let canonical_name = canonical.name;
            let activate = self
                .lazy_registry
                .match_event(canonical_name)
                .or_else(|| self.lazy_registry.match_event(event))
                .map(|e| e.plugin_id.clone());
            if let Some(plugin_id) = activate {
                if self
                    .activate_now(&plugin_id, "event", format!("event:{canonical_name}"))
                    .is_ok()
                {
                    // Continue the dispatch — we want the freshly
                    // installed autocmds to observe this same event.
                }
            }
        }
        let (canonical, _alias_used) = match events::resolve_event(event) {
            Ok(pair) => pair,
            Err(_) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from("<host>"),
                        DiagnosticSeverity::Warning,
                        "autocmd.invalid_event",
                        format!("fire_event_typed: unknown event `{event}`"),
                    )
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("event:{event}"),
                    }),
                );
                return;
            }
        };
        let _validated = events::validate_payload(canonical, &payload, &self.diagnostics);

        let mut affected: HashSet<String> = HashSet::new();
        self.dispatch_for_name(canonical, None, &payload, &mut affected);

        for pid in affected {
            self.refresh_dynamic_widgets_for_plugin(&pid);
        }
    }

    /// Test-only event replay hook. Same dispatch funnel as
    /// `fire_event_typed`, exposed so plugin tests can stage event
    /// sequences deterministically without going through the app
    /// layer. The hook deliberately drives `fire_event_typed` (no
    /// shortcut) so any future invariants the funnel adopts apply
    /// to tests too.
    pub fn dispatch_test_event(&mut self, event: &str, payload: serde_json::Value) {
        let map = match payload {
            serde_json::Value::Object(m) => m,
            serde_json::Value::Null => EventPayload::new(),
            other => {
                let mut m = EventPayload::new();
                m.insert("value".into(), other);
                m
            }
        };
        self.fire_event_typed(event, map);
    }

    /// Advance the host's virtual debounce clock. Tests use this to
    /// drive `debounce_ms` deterministically.
    pub fn advance_event_clock(&mut self, delta_ms: u64) {
        self.event_bus.advance_clock(delta_ms);
    }

    fn dispatch_for_name(
        &mut self,
        canonical: &'static event_descriptor::ApiEvent,
        alias_name: Option<&'static str>,
        payload: &EventPayload,
        affected: &mut HashSet<String>,
    ) {
        // Look up the dispatch name (canonical or alias) and snapshot
        // the iteration order so concurrent removals from `once` /
        // disable don't shift the iteration index.
        let dispatch_name = alias_name.unwrap_or(canonical.name);
        let entries = self.event_bus.entries();
        let order: Vec<usize> = events::dispatch_order(entries)
            .into_iter()
            .filter(|i| entries[*i].event == dispatch_name)
            .collect();
        if order.is_empty() {
            return;
        }
        let now_ms = self.event_bus.clock_ms();
        let mut to_remove: Vec<u64> = Vec::new();
        for index in order {
            // Re-fetch the entry by index because earlier callbacks
            // may have mutated `entries` (e.g. on a recursive fire).
            let snapshot = {
                let entries = self.event_bus.entries();
                if index >= entries.len() {
                    continue;
                }
                let entry = &entries[index];
                EntrySnapshot {
                    id: entry.id.get(),
                    plugin_id: entry.plugin_id.clone(),
                    generation_id: entry.generation_id,
                    once: entry.options.once,
                    debounce_ms: entry.options.debounce_ms,
                    pattern_present: entry.options.pattern.is_some(),
                    matches_pattern: events::pattern_matches(entry, payload),
                    last_fire_clock_ms: entry.runtime.last_fire_clock_ms,
                    disabled: entry.runtime.disabled,
                }
            };
            if snapshot.disabled {
                continue;
            }
            if snapshot.pattern_present && !snapshot.matches_pattern {
                continue;
            }
            if snapshot.debounce_ms > 0 {
                if let Some(prev) = snapshot.last_fire_clock_ms {
                    if now_ms.saturating_sub(prev) < snapshot.debounce_ms {
                        continue;
                    }
                }
            }
            let outcome = self.invoke_one_callback(
                &snapshot.plugin_id,
                snapshot.generation_id,
                snapshot.id,
                canonical,
                alias_name,
                payload,
            );
            // Bookkeeping (counters, last_fire, once removal,
            // disable on threshold). Done after the callback returns
            // so the snapshot reflects post-call state.
            self.update_runtime(snapshot.id, &outcome, now_ms);
            if matches!(outcome, DispatchOutcome::Ok | DispatchOutcome::Error) {
                affected.insert(snapshot.plugin_id);
                if snapshot.once {
                    to_remove.push(snapshot.id);
                }
            }
        }
        for id in to_remove {
            self.event_bus
                .entries_mut()
                .retain(|entry| entry.id.get() != id);
        }
    }

    fn invoke_one_callback(
        &self,
        plugin_id: &str,
        generation_id: GenerationId,
        autocmd_id: u64,
        canonical: &'static event_descriptor::ApiEvent,
        alias_used: Option<&'static str>,
        payload: &EventPayload,
    ) -> DispatchOutcome {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return DispatchOutcome::Skipped;
        };
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        // Resolve the callback through the entry's RegistryKey via the
        // entries vector — the snapshot only carries ids, so dive
        // back in here.
        let key_ptr: *const RegistryKey = {
            let entries = self.event_bus.entries();
            match entries.iter().find(|e| e.id.get() == autocmd_id) {
                Some(entry) => &entry.callback as *const RegistryKey,
                None => return DispatchOutcome::Skipped,
            }
        };
        // SAFETY: the EventBus owns the RegistryKey for the lifetime
        // of this call (we never remove during `invoke_one_callback`,
        // only in `dispatch_for_name` after we return). Borrowing as
        // a `*const` lets us avoid holding a borrow on `self.event_bus`
        // while we call into Lua.
        let key: &RegistryKey = unsafe { &*key_ptr };
        let func: Function = match plugin.lua().registry_value(key) {
            Ok(f) => f,
            Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "lua.handler_lookup_failed",
                        format!("autocmd handler lookup failed for {}: {e}", canonical.name),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("autocmd:{}", canonical.name),
                    }),
                );
                return DispatchOutcome::Error;
            }
        };
        let payload_table = match build_payload_table(plugin.lua(), canonical, alias_used, payload)
        {
            Ok(t) => t,
            Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "autocmd.payload_mismatch",
                        format!("could not build typed payload: {e}"),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("autocmd:{}", canonical.name),
                    }),
                );
                return DispatchOutcome::Error;
            }
        };
        let pid = PluginId::from(plugin_id);
        let callback_id = format!("autocmd:{}", canonical.name);
        let perf_outcome = self.budget_tracker.track_call::<(), mlua::Error>(
            CallbackKind::EventCallback,
            &pid,
            generation_id,
            &callback_id,
            || func.call::<()>(payload_table),
        );
        match perf_outcome {
            PerfOutcome::Skipped => DispatchOutcome::Skipped,
            PerfOutcome::Ok(()) => DispatchOutcome::Ok,
            PerfOutcome::Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "autocmd.callback_failed",
                        format!("autocmd handler error for {}", canonical.name),
                    )
                    .with_generation(generation_id)
                    .with_mlua_error(&chunk_name, &e)
                    .with_context(serde_json::json!({
                        "event": canonical.name,
                        "alias_used": alias_used,
                        "autocmd_id": autocmd_id,
                    })),
                );
                DispatchOutcome::Error
            }
        }
    }

    fn update_runtime(&mut self, autocmd_id: u64, outcome: &DispatchOutcome, now_ms: u64) {
        let entries = self.event_bus.entries_mut();
        let Some(entry) = entries.iter_mut().find(|e| e.id.get() == autocmd_id) else {
            return;
        };
        match outcome {
            DispatchOutcome::Ok => {
                entry.runtime.fires += 1;
                entry.runtime.consecutive_failures = 0;
                entry.runtime.last_fire_clock_ms = Some(now_ms);
            }
            DispatchOutcome::Error => {
                entry.runtime.fires += 1;
                entry.runtime.failures += 1;
                entry.runtime.consecutive_failures += 1;
                entry.runtime.last_fire_clock_ms = Some(now_ms);
                if entry.runtime.consecutive_failures >= MAX_CONSECUTIVE_FAILURES
                    && !entry.runtime.disabled
                {
                    entry.runtime.disabled = true;
                    let plugin_id = entry.plugin_id.clone();
                    let generation_id = entry.generation_id;
                    let event_name = entry.event;
                    let consecutive = entry.runtime.consecutive_failures;
                    let source_location = entry.options.source_location.clone();
                    // Drop the &mut entry borrow before recording
                    // the diagnostic so the diagnostic store call
                    // doesn't reborrow self.event_bus.
                    let _ = entry;
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Error,
                            "autocmd.disabled_after_failures",
                            format!(
                                "autocmd for {event_name} disabled after {consecutive} \
                                 consecutive failures"
                            ),
                        )
                        .with_generation(generation_id)
                        .with_source(make_lua_span(&plugin_id, source_location.as_deref()))
                        .with_context(serde_json::json!({
                            "event": event_name,
                            "consecutive_failures": consecutive,
                            "autocmd_id": autocmd_id,
                        })),
                    );
                }
            }
            DispatchOutcome::Skipped => {}
        }
    }

    /// Push the active repository identity and latest branch refs into every
    /// plugin's `leviathan.repository`,
    /// invoke `BranchChanged` autocmd subscribers, and refresh every
    /// plugin's dynamic main-bar widgets.
    ///
    /// Widgets are refreshed unconditionally — not only for subscribers —
    /// because `leviathan.repository` is a host-owned global that any
    /// widget fn might read. Requiring an opt-in autocmd just to trigger
    /// a widget refresh would force plugins to declare an empty callback
    /// as a refresh hint. Refresh is cheap (one Lua call per dynamic
    /// slot), diffing by `last_repository_hash` keeps the common unchanged
    /// path free; the explicit `BranchChanged` event is still useful for
    /// plugins that want to react imperatively (toasts, logs, etc.).
    ///
    /// Cheap no-op when the repository snapshot hash matches the last sync — callers can
    /// invoke liberally from app-level update hooks without tracking
    /// change detection themselves.
    pub fn sync_repository(
        &mut self,
        repo_name: &str,
        workdir_path: &str,
        current_branch_name: &str,
        head_hash: &str,
        default_remote_name: &str,
        refs: &[RepoRef],
    ) {
        let hash = compute_repo_hash(
            repo_name,
            workdir_path,
            current_branch_name,
            head_hash,
            default_remote_name,
            refs,
        );
        if self.last_repository_hash == Some(hash) {
            return;
        }
        self.last_repository_hash = Some(hash);

        for plugin in self.plugins.values() {
            let plugin_id = plugin.id().to_string();
            let generation_id = plugin.generation.generation_id;
            let table = match api::repository::build_table(
                plugin.lua(),
                repo_name,
                workdir_path,
                current_branch_name,
                head_hash,
                default_remote_name,
                refs,
            ) {
                Ok(t) => t,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Error,
                            "host.repository_table_build_failed",
                            format!("build leviathan.repository failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.repository (sync)".into(),
                        }),
                    );
                    continue;
                }
            };
            let leviathan: Table = match plugin.lua().globals().get("leviathan") {
                Ok(t) => t,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Error,
                            "host.leviathan_global_missing",
                            format!("`leviathan` global missing: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan (global)".into(),
                        }),
                    );
                    continue;
                }
            };
            if let Err(e) = leviathan.set("repository", table) {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "host.repository_table_set_failed",
                        format!("set leviathan.repository failed: {e}"),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: "leviathan.repository (sync)".into(),
                    }),
                );
            }
        }

        // Run BranchChanged callbacks first so any Lua-side state they
        // mutate is fresh before widgets re-read the globals. The
        // payload mirrors the new typed `BranchChanged` schema
        // (`name`, `head_hash`).
        let mut payload = EventPayload::new();
        payload.insert(
            "name".into(),
            serde_json::Value::String(current_branch_name.to_string()),
        );
        payload.insert(
            "head_hash".into(),
            serde_json::Value::String(head_hash.to_string()),
        );
        self.fire_event_typed("BranchChanged", payload);

        let plugin_ids: Vec<String> = self.plugins.keys().cloned().collect();
        for pid in plugin_ids {
            self.refresh_dynamic_widgets_for_plugin(&pid);
        }

        // Phase 16: cache the new shape facts and probe the lazy
        // registry for repository-shape and file-presence triggers.
        let has_remote = !default_remote_name.is_empty();
        let workdir_buf = PathBuf::from(workdir_path);
        let facts = RepositoryShapeFacts {
            current_branch: current_branch_name.to_string(),
            has_remote,
            workdir: workdir_buf.clone(),
        };
        self.last_repository_shape = Some(facts);
        self.probe_lazy_repository_triggers();
    }

    /// Phase 16: walk the lazy registry's repository-shape and
    /// file-presence triggers against the cached facts and activate
    /// matching plugins. Called after every `sync_repository`.
    fn probe_lazy_repository_triggers(&mut self) {
        let Some(facts) = self.last_repository_shape.clone() else {
            return;
        };
        // Repository shape predicates.
        loop {
            let next = self
                .lazy_registry
                .match_repo_shape(&facts.current_branch, facts.has_remote)
                .map(|e| e.plugin_id.clone());
            match next {
                Some(plugin_id) => {
                    let _ = self.activate_now(
                        &plugin_id,
                        "repository_shape",
                        "repository_shape".to_string(),
                    );
                }
                None => break,
            }
        }
        // File presence: collect file lists snapshot first, then
        // iterate so we can mutate the registry inside the loop.
        let candidates: Vec<(String, Vec<PathBuf>)> = self
            .lazy_registry
            .entries()
            .iter()
            .filter(|e| e.status == crate::plugin::activation::LazyStatus::Lazy)
            .map(|e| (e.plugin_id.clone(), e.files.clone()))
            .collect();
        for (plugin_id, files) in candidates {
            for rel in files {
                let abs = facts.workdir.join(&rel);
                if abs.exists() {
                    let _ =
                        self.activate_now(&plugin_id, "file", format!("file:{}", rel.display()));
                    break;
                }
            }
        }
    }

    /// Drain the queue of `tab_registry.{add,remove,select}` ops Lua
    /// pushed since the last call. App applies them through `TabManager`.
    pub fn take_pending_tab_ops(&mut self) -> Vec<TabRegistryOp> {
        std::mem::take(&mut *self.pending_tab_ops.borrow_mut())
    }

    /// Push a fresh tabs snapshot into every plugin's
    /// `leviathan.tab_registry.{list, current}`. Cheap no-op when the
    /// snapshot hash matches the last sync, mirroring `sync_repository`.
    /// Does not fire any tab-lifecycle events — those are explicit at the
    /// app's tab-mutation sites.
    ///
    /// Refreshes every plugin's dynamic widgets after the table is set.
    /// Same rationale as `sync_repository`: `leviathan.tab_registry` is a
    /// host-owned global that any `widget = function() ... end` may
    /// read, even from a plugin that didn't subscribe to a tab event.
    pub fn sync_tab_registry(&mut self, snapshot: &TabsSnapshot) -> Option<TabChange> {
        if &self.last_tab_snapshot == snapshot {
            return None;
        }
        let change = TabChange::diff(&self.last_tab_snapshot, snapshot);
        self.last_tab_snapshot = snapshot.clone();

        for plugin in self.plugins.values() {
            let plugin_id = plugin.id().to_string();
            let generation_id = plugin.generation.generation_id;
            let leviathan: Table = match plugin.lua().globals().get("leviathan") {
                Ok(t) => t,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Error,
                            "host.leviathan_global_missing",
                            format!("`leviathan` global missing: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan (global)".into(),
                        }),
                    );
                    continue;
                }
            };
            if let Err(e) = api::tab_registry::refresh(plugin.lua(), &leviathan, snapshot) {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "host.tab_registry_refresh_failed",
                        format!("tab_registry refresh failed: {e}"),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: "leviathan.tab_registry (sync)".into(),
                    }),
                );
            }
        }

        let plugin_ids: Vec<String> = self.plugins.keys().cloned().collect();
        for pid in plugin_ids {
            self.refresh_dynamic_widgets_for_plugin(&pid);
        }

        Some(change)
    }

    pub fn tab_snapshot(&self) -> &TabsSnapshot {
        &self.last_tab_snapshot
    }

    /// Drain every plugin's deferred-call queue. Order per plugin:
    ///
    /// 1. Immediate (`leviathan.api.schedule(fn)`) callbacks, in FIFO order.
    /// 2. Delayed (`defer_fn(ms, fn)`) callbacks whose deadline is now in
    ///    the past.
    /// 3. Resumable coroutines parked from a previous
    ///    `invoke_user_command` or earlier tick. Coroutines that yield
    ///    again are re-parked for the next tick.
    ///
    /// Errors from any callback / resume are logged; other queue entries
    /// keep processing so one buggy callback can't stall a plugin.
    pub fn tick(&mut self) {
        // Phase 8: drain any `CommandExecuted` events queued by Lua-
        // initiated dispatch since the last tick. The Rust entry
        // (`invoke_command`) flushes synchronously; this catches
        // anything the Lua API queued in between.
        self.flush_pending_command_events();
        // Phase 11: drain any typed events the git Lua API queued so
        // `HeadChanged` / `RefsChanged` / etc. fire on the next tick
        // even if the plugin invoked the op via `tick`-deferred Lua.
        self.flush_pending_git_events();
        let now = Instant::now();
        let ids: Vec<String> = self.plugins.keys().cloned().collect();
        let mut pending: Vec<PluginDiagnostic> = Vec::new();
        for id in ids {
            let Some(plugin) = self.plugins.get(&id) else {
                continue;
            };
            let lua = plugin.lua_rc();
            let queue = Rc::clone(&plugin.deferred);
            let ledger = plugin.ledger();
            let generation_id = plugin.generation.generation_id;
            let chunk_name = format!("plugins/{id}/init.lua");

            let immediate = queue.borrow_mut().drain_immediate();
            for callback in immediate {
                match lua.registry_value::<Function>(&callback.key) {
                    Ok(f) => {
                        if let Err(e) = f.call::<()>(()) {
                            pending.push(
                                PluginDiagnostic::new(
                                    PluginId::from(id.clone()),
                                    DiagnosticSeverity::Error,
                                    "lua.callback_error",
                                    "scheduled fn error".to_string(),
                                )
                                .with_generation(generation_id)
                                .with_mlua_error(&chunk_name, &e),
                            );
                        }
                    }
                    Err(e) => pending.push(
                        PluginDiagnostic::new(
                            PluginId::from(id.clone()),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("scheduled fn lookup failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.api.schedule".into(),
                        }),
                    ),
                }
                ledger.remove_resource(callback.resource_id);
            }

            let due = queue.borrow_mut().drain_due(now);
            for callback in due {
                match lua.registry_value::<Function>(&callback.key) {
                    Ok(f) => {
                        if let Err(e) = f.call::<()>(()) {
                            pending.push(
                                PluginDiagnostic::new(
                                    PluginId::from(id.clone()),
                                    DiagnosticSeverity::Error,
                                    "lua.callback_error",
                                    "defer_fn error".to_string(),
                                )
                                .with_generation(generation_id)
                                .with_mlua_error(&chunk_name, &e),
                            );
                        }
                    }
                    Err(e) => pending.push(
                        PluginDiagnostic::new(
                            PluginId::from(id.clone()),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("defer_fn lookup failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.api.defer_fn".into(),
                        }),
                    ),
                }
                ledger.remove_resource(callback.resource_id);
            }

            let drained: Vec<DeferredCallback> = std::mem::take(&mut queue.borrow_mut().coroutines);
            for callback in drained {
                let thread: Thread = match lua.registry_value(&callback.key) {
                    Ok(t) => t,
                    Err(e) => {
                        pending.push(
                            PluginDiagnostic::new(
                                PluginId::from(id.clone()),
                                DiagnosticSeverity::Error,
                                "lua.handler_lookup_failed",
                                format!("coroutine lookup failed: {e}"),
                            )
                            .with_generation(generation_id)
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: "leviathan.api.coroutine".into(),
                                },
                            ),
                        );
                        ledger.remove_resource(callback.resource_id);
                        continue;
                    }
                };
                if let Err(e) = thread.resume::<()>(()) {
                    pending.push(
                        PluginDiagnostic::new(
                            PluginId::from(id.clone()),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            "coroutine resume error".to_string(),
                        )
                        .with_generation(generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                    ledger.remove_resource(callback.resource_id);
                    continue;
                }
                ledger.remove_resource(callback.resource_id);
                if thread.status() == ThreadStatus::Resumable {
                    match lua.create_registry_value(thread) {
                        Ok(new_key) => {
                            let resource_id =
                                ledger.record(PluginResourceKind::AsyncJob, "coroutine", None);
                            ledger.record(
                                PluginResourceKind::LuaRegistryKey,
                                format!("coroutine:{resource_id}"),
                                None,
                            );
                            queue.borrow_mut().coroutines.push(DeferredCallback {
                                key: new_key,
                                resource_id,
                            });
                        }
                        Err(e) => pending.push(
                            PluginDiagnostic::new(
                                PluginId::from(id.clone()),
                                DiagnosticSeverity::Error,
                                "host.coroutine_repark_failed",
                                format!("re-parking coroutine failed: {e}"),
                            )
                            .with_generation(generation_id)
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: "leviathan.api.coroutine".into(),
                                },
                            ),
                        ),
                    }
                }
            }
        }
        for diag in pending {
            self.diagnostics.record(diag);
        }

        // Phase 12: drain finished async jobs, due timers, and queued
        // file-watcher events. Each fires a Lua callback on the
        // matching plugin's main state.
        self.drive_phase12_runtime(now);
    }

    /// Phase 12: invoke plugin Lua callbacks for finished async jobs,
    /// due timers, and buffered file-watcher events. Errors are
    /// recorded as diagnostics; one buggy callback can't stall the
    /// next.
    fn drive_phase12_runtime(&mut self, now: Instant) {
        let mut pending: Vec<PluginDiagnostic> = Vec::new();

        // Async jobs.
        for job in self.async_jobs.drain_finished() {
            let Some(plugin) = self.plugins.get(job.plugin_id.as_str()) else {
                continue;
            };
            if plugin.generation.generation_id != job.generation_id {
                // Stale generation — skip, the new gen owns its own
                // jobs.
                continue;
            }
            plugin.ledger().remove_resource(job.resource_id);
            let key_opt = plugin.job_callbacks.borrow_mut().remove(job.job_id);
            let Some(key) = key_opt else { continue };
            let lua = plugin.lua_rc();
            let func: Function = match lua.registry_value(&key) {
                Ok(f) => f,
                Err(e) => {
                    pending.push(
                        PluginDiagnostic::new(
                            job.plugin_id.clone(),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("async on_complete lookup failed: {e}"),
                        )
                        .with_generation(job.generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.async.spawn".into(),
                        }),
                    );
                    continue;
                }
            };
            let chunk_name = format!("plugins/{}/init.lua", job.plugin_id.as_str());
            // Phase 18: budget the async on-complete callback against
            // the `AsyncJob` budget. The async body itself runs off
            // the main thread, but on_complete is the only Lua-side
            // step the host invokes synchronously, so it's the
            // user-observable cost.
            let callback_id = format!("async:on_complete:{}", job.job_id.get());
            let perf_outcome = self.budget_tracker.track_call::<(), mlua::Error>(
                CallbackKind::AsyncJob,
                &job.plugin_id,
                job.generation_id,
                &callback_id,
                || match job.outcome {
                    JobOutcome::Ok(value) => {
                        let lua_value = lua.to_value(&value).unwrap_or(mlua::Value::Nil);
                        func.call::<()>((true, lua_value))
                    }
                    JobOutcome::Cancelled => func.call::<()>((false, "cancelled")),
                    JobOutcome::Failed(msg) => func.call::<()>((false, msg)),
                },
            );
            if let PerfOutcome::Err(e) = perf_outcome {
                pending.push(
                    PluginDiagnostic::new(
                        job.plugin_id.clone(),
                        DiagnosticSeverity::Error,
                        "lua.callback_error",
                        "async on_complete error".to_string(),
                    )
                    .with_generation(job.generation_id)
                    .with_mlua_error(&chunk_name, &e),
                );
            }
        }

        // Timers.
        let due_timers = self.timers.drain_due(now);
        for due in due_timers {
            let Some(plugin) = self.plugins.get(due.plugin_id.as_str()) else {
                continue;
            };
            if plugin.generation.generation_id != due.generation_id {
                continue;
            }
            let lua = plugin.lua_rc();
            let chunk_name = format!("plugins/{}/init.lua", due.plugin_id.as_str());
            let func_opt: Option<Function> = match due.kind {
                crate::plugin::timers::TimerKind::After => {
                    let key_opt = plugin.timer_callbacks.borrow_mut().remove(due.timer_id);
                    plugin.ledger().remove_resource(due.resource_id);
                    key_opt.and_then(|k| lua.registry_value::<Function>(&k).ok())
                }
                crate::plugin::timers::TimerKind::Every => plugin
                    .timer_callbacks
                    .borrow()
                    .get(due.timer_id)
                    .and_then(|k| lua.registry_value::<Function>(k).ok()),
            };
            let Some(func) = func_opt else { continue };
            // Phase 18: time the timer callback against the `Timer`
            // budget. The callback id is the timer kind + id so each
            // timer's stats roll up independently.
            let callback_id = format!("timer:{}:{}", due.kind.as_str(), due.timer_id.get());
            let perf_outcome = self.budget_tracker.track_call::<(), mlua::Error>(
                CallbackKind::Timer,
                &due.plugin_id,
                due.generation_id,
                &callback_id,
                || func.call::<()>(()),
            );
            if let PerfOutcome::Err(e) = perf_outcome {
                pending.push(
                    PluginDiagnostic::new(
                        due.plugin_id.clone(),
                        DiagnosticSeverity::Error,
                        "lua.callback_error",
                        format!("timer.{} callback error", due.kind.as_str()),
                    )
                    .with_generation(due.generation_id)
                    .with_mlua_error(&chunk_name, &e),
                );
            }
        }

        // File watchers.
        let events = self.watchers.drain_events();
        for ev in events {
            let Some(plugin) = self.plugins.get(ev.plugin_id.as_str()) else {
                continue;
            };
            if plugin.generation.generation_id != ev.generation_id {
                continue;
            }
            let lua = plugin.lua_rc();
            let func: Option<Function> = {
                let callbacks = plugin.watcher_callbacks.borrow();
                callbacks
                    .get(ev.watch_id)
                    .and_then(|k| lua.registry_value::<Function>(k).ok())
            };
            let Some(func) = func else { continue };
            let event_table = match build_watch_event_table(&lua, &ev.event) {
                Ok(t) => t,
                Err(e) => {
                    pending.push(
                        PluginDiagnostic::new(
                            ev.plugin_id.clone(),
                            DiagnosticSeverity::Error,
                            "lua.watch_event_build_failed",
                            format!("watch event table build failed: {e}"),
                        )
                        .with_generation(ev.generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.fs.watch".into(),
                        }),
                    );
                    continue;
                }
            };
            let chunk_name = format!("plugins/{}/init.lua", ev.plugin_id.as_str());
            if let Err(e) = func.call::<()>(event_table) {
                pending.push(
                    PluginDiagnostic::new(
                        ev.plugin_id.clone(),
                        DiagnosticSeverity::Error,
                        "lua.callback_error",
                        "fs.watch callback error".to_string(),
                    )
                    .with_generation(ev.generation_id)
                    .with_mlua_error(&chunk_name, &e),
                );
            }
        }

        for diag in pending {
            self.diagnostics.record(diag);
        }
    }

    /// Invoke a plugin's named user command. The function is wrapped in
    /// a Lua coroutine so cooperative yields are honoured: if the command
    /// yields, it's parked in the plugin's `coroutines` bucket and
    /// resumed on subsequent `tick` calls. Returns once the first resume
    /// finishes (either completed or yielded).
    pub fn invoke_user_command(&mut self, plugin_id: &str, name: &str) -> mlua::Result<()> {
        let plugin = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| mlua::Error::external(format!("plugin '{plugin_id}' not loaded")))?;
        let f: Function = {
            let cmds = plugin.user_commands.borrow();
            let key = cmds.commands.get(name).ok_or_else(|| {
                mlua::Error::external(format!("user command '{name}' not registered"))
            })?;
            plugin.lua().registry_value(key)?
        };
        let thread = plugin.lua().create_thread(f)?;
        thread.resume::<()>(())?;
        if thread.status() == ThreadStatus::Resumable {
            let key = plugin.lua().create_registry_value(thread)?;
            let ledger = plugin.ledger();
            let resource_id = ledger.record(PluginResourceKind::AsyncJob, "user_command", None);
            ledger.record(
                PluginResourceKind::LuaRegistryKey,
                format!("user_command:{resource_id}"),
                None,
            );
            plugin
                .deferred
                .borrow_mut()
                .coroutines
                .push(DeferredCallback { key, resource_id });
        }
        Ok(())
    }

    /// Run every plugin's registered health checks and return an aggregated
    /// report. Plugins that didn't register a check (or whose checks
    /// produced no items) are absent from the report. Errors from
    /// individual callbacks are logged; partial item lists are kept.
    pub fn run_health_checks(&self) -> HealthReport {
        let mut report = HealthReport::default();
        for (plugin_id, plugin) in &self.plugins {
            let generation_id = plugin.generation.generation_id;
            let chunk_name = format!("plugins/{plugin_id}/init.lua");
            let mut items: Vec<HealthItem> = Vec::new();
            for check in &plugin.health_checks {
                let func: Function = match plugin.lua().registry_value(&check.callback) {
                    Ok(f) => f,
                    Err(e) => {
                        self.diagnostics.record(
                            PluginDiagnostic::new(
                                PluginId::from(plugin_id.clone()),
                                DiagnosticSeverity::Error,
                                "lua.handler_lookup_failed",
                                format!("health callback lookup failed: {e}"),
                            )
                            .with_generation(generation_id)
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: "leviathan.health.register".into(),
                                },
                            ),
                        );
                        continue;
                    }
                };
                let bucket: Rc<RefCell<Vec<HealthItem>>> = Rc::new(RefCell::new(Vec::new()));
                let ctx = HealthContext {
                    items: Rc::clone(&bucket),
                };
                if let Err(e) = func.call::<()>(ctx) {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            "health callback error".to_string(),
                        )
                        .with_generation(generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                }
                items.extend(bucket.borrow().iter().cloned());
            }
            if !items.is_empty() {
                report.plugins.push(PluginHealth {
                    plugin_id: plugin_id.clone(),
                    items,
                });
            }
        }
        report
    }

    /// Point-in-time devtools snapshot: loaded plugins, currently-owned
    /// slots, registered services, and the tail of the capability audit
    /// log. Cheap to call (clones strings; ~O(plugins + slot_ops +
    /// services + audit)). Consumed by the in-app inspector and tests.
    pub fn introspect(&self) -> crate::plugin::devtools::InspectorSnapshot {
        use crate::plugin::devtools::{
            AutocmdSummary, CommandSummaryRow, DependencySummaryRow, DiagnosticSummary,
            InspectorSnapshot, KeymapConflictRef, KeymapSummaryRow, LoadedModuleSummary,
            PluginSummary, ResourceSummary, RuntimePathSummary, SecretSummary,
            ServiceCallTraceSummary, ServiceGraphEdge, ServiceSummary, SettingsSummary,
            SlotSummary, StorageSurfaceSummary,
        };
        let mut snap = InspectorSnapshot::default();

        for (id, plugin) in &self.plugins {
            let m = &plugin.manifest;
            snap.plugins.push(PluginSummary {
                id: id.clone(),
                name: m.name.clone(),
                version: m.version.to_string(),
                api_version: format!("{}.{}", m.api_version.major, m.api_version.minor),
                last_reload_error: self.last_reload_errors.get(id).cloned(),
                provides_services: m
                    .provides_services
                    .iter()
                    .map(|d| format!("{}@{}", d.name, d.version))
                    .collect(),
                consumes_services: m
                    .consumes_services
                    .iter()
                    .map(|d| format!("{}@{}", d.name, d.version))
                    .collect(),
                capabilities: m
                    .capabilities
                    .iter()
                    .map(|c| String::from(c.clone()))
                    .collect(),
            });
            for resource in plugin.generation.ledger.records() {
                debug_assert_eq!(resource.plugin_id, plugin.generation.plugin_id);
                debug_assert_eq!(resource.generation_id, plugin.generation.generation_id);
                let created_at_unix_ms = resource.created_at_unix_ms();
                snap.resources.push(ResourceSummary {
                    resource_id: resource.resource_id.get(),
                    plugin_id: resource.plugin_id.to_string(),
                    generation_id: resource.generation_id.get(),
                    kind: resource.kind.as_str().to_string(),
                    handle: resource.handle,
                    source_location: resource.source_location,
                    created_at_unix_ms,
                });
            }
            for entry in plugin.lua_loader.runtime_path().entries() {
                snap.runtime_paths.push(RuntimePathSummary {
                    plugin_id: id.clone(),
                    generation_id: plugin.generation.generation_id.get(),
                    entry_plugin_id: entry.plugin_id.clone(),
                    kind: entry.kind.as_str().to_string(),
                    root: entry.lua_root.display().to_string(),
                });
            }
            for record in plugin.lua_loader.module_records() {
                snap.loaded_modules.push(LoadedModuleSummary {
                    plugin_id: id.clone(),
                    generation_id: plugin.generation.generation_id.get(),
                    module_name: record.module_name.clone(),
                    source_plugin_id: record.plugin_id.clone(),
                    source_path: record.source_path.display().to_string(),
                    kind: record.kind.as_str().to_string(),
                });
            }
            let storage = self.storage_paths(id, plugin.root.clone());
            for surface in StorageSurface::devtools_surfaces() {
                let path = match surface {
                    StorageSurface::Settings => storage.settings_path(),
                    _ => storage.surface_dir(surface),
                };
                let meta = crate::plugin::storage::surface_metadata(id, surface, &path);
                snap.storage.push(StorageSurfaceSummary {
                    plugin_id: meta.plugin_id,
                    surface: meta.surface,
                    path: meta.path,
                    exists: meta.exists,
                    file_count: meta.file_count,
                    byte_count: meta.byte_count,
                    corrupt_files: meta.corrupt_files,
                });
            }
            let settings_meta = crate::plugin::settings::metadata(&storage.settings_path());
            snap.settings.push(SettingsSummary {
                plugin_id: id.clone(),
                path: settings_meta.path,
                schema_keys: settings_meta.schema_keys,
                value_keys: settings_meta.value_keys,
                valid: settings_meta.valid,
                errors: settings_meta.errors,
            });
            let secret_meta = crate::plugin::secrets::metadata(&storage.secrets_dir);
            snap.secrets.push(SecretSummary {
                plugin_id: id.clone(),
                path: secret_meta.path,
                key_count: secret_meta.key_count,
                keys: secret_meta.keys,
            });
        }
        snap.runtime_paths
            .sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        snap.loaded_modules.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.module_name.cmp(&b.module_name))
        });
        snap.plugins.sort_by(|a, b| a.id.cmp(&b.id));
        snap.resources.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.generation_id.cmp(&b.generation_id))
                .then(a.resource_id.cmp(&b.resource_id))
        });
        snap.storage.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.surface.cmp(&b.surface))
        });
        snap.settings.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        snap.secrets.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));

        // Walk slot_ops in order, applying Add/Replace/Remove to a
        // (region, container, id) keyed map so the snapshot reflects the
        // currently-owned slots rather than the raw op log.
        let mut slot_map: std::collections::BTreeMap<(String, String, String), SlotSummary> =
            std::collections::BTreeMap::new();
        for op in &self.slot_ops {
            match op {
                PreparedSlotOp::Add(p) => {
                    let key = (p.region.clone(), p.container.key(), p.id.clone());
                    slot_map.insert(
                        key,
                        SlotSummary {
                            region: p.region.clone(),
                            container: p.container.key(),
                            id: p.id.clone(),
                            priority: p.priority,
                            owner_plugin_id: p.plugin_id.clone(),
                        },
                    );
                }
                PreparedSlotOp::Replace {
                    region,
                    container,
                    id,
                    spec,
                } => {
                    let key = (region.clone(), container.key(), id.clone());
                    slot_map.insert(
                        key,
                        SlotSummary {
                            region: region.clone(),
                            container: container.key(),
                            id: id.clone(),
                            priority: spec.priority,
                            owner_plugin_id: spec.plugin_id.clone(),
                        },
                    );
                }
                PreparedSlotOp::Remove {
                    region,
                    container,
                    id,
                } => {
                    slot_map.remove(&(region.clone(), container.key(), id.clone()));
                }
            }
        }
        snap.slots = slot_map.into_values().collect();

        {
            let registry = self.service_registry.borrow();
            for h in registry.handles_iter() {
                let mut methods: Vec<String> = h.methods.keys().cloned().collect();
                methods.sort();
                snap.services.push(ServiceSummary {
                    key: ServiceRegistry::key(&h.decl),
                    publisher_plugin_id: h.plugin_id.clone(),
                    methods,
                });
            }
            for (id, plugin) in &self.plugins {
                for status in dependency_statuses(
                    id,
                    &plugin.manifest.provides_services,
                    &plugin.manifest.consumes_services,
                    &registry,
                ) {
                    let edge_status = match (status.required, status.satisfied) {
                        (_, true) => "connected",
                        (true, false) => "missing_required",
                        (false, false) => "missing_optional",
                    };
                    snap.service_graph.push(ServiceGraphEdge {
                        consumer_plugin_id: status.consumer_plugin_id,
                        provider_plugin_id: status.provider_plugin_id,
                        service_key: status.service_key,
                        required: status.required,
                        status: edge_status.to_string(),
                    });
                }
            }
            for trace in registry.traces() {
                snap.service_call_traces.push(ServiceCallTraceSummary {
                    caller_plugin_id: trace.caller_plugin_id,
                    provider_plugin_id: trace.provider_plugin_id,
                    service_key: trace.service_key,
                    method: trace.method,
                    success: trace.success,
                    error: trace.error,
                    duration_ms: trace.duration_ms,
                    timestamp_unix_ms: trace.timestamp_unix_ms,
                });
            }
        }
        snap.services.sort_by(|a, b| a.key.cmp(&b.key));
        snap.service_graph.sort_by(|a, b| {
            a.consumer_plugin_id
                .cmp(&b.consumer_plugin_id)
                .then(a.service_key.cmp(&b.service_key))
        });

        // Phase 15 dependency graph projection. The host stores the
        // resolver's last graph as live state so devtools mirror what
        // resolution actually produced (including blocked plugins).
        snap.dependency_graph = self
            .dependency_graph
            .iter()
            .map(|d| DependencySummaryRow {
                consumer_plugin_id: d.consumer_plugin_id.clone(),
                dependency_id: d.dependency_id.clone(),
                requirement: d.requirement.clone(),
                resolved_version: d.resolved_version.clone(),
                kind: d.kind.to_string(),
                status: d.status.to_string(),
            })
            .collect();
        snap.dependency_graph.sort_by(|a, b| {
            a.consumer_plugin_id
                .cmp(&b.consumer_plugin_id)
                .then(a.dependency_id.cmp(&b.dependency_id))
        });

        let entries = self.audit_log.entries();
        let n = entries.len();
        let start = n.saturating_sub(100);
        snap.audit_recent = entries[start..].to_vec();

        snap.diagnostics = self
            .diagnostics
            .tail(100)
            .into_iter()
            .map(|d| DiagnosticSummary {
                plugin_id: d.plugin_id.to_string(),
                generation_id: d.generation_id.map(|g| g.get()),
                severity: d.severity.to_string(),
                code: d.code.clone(),
                message: d.message.clone(),
                source: d.source_string(),
                context: d.context.clone(),
                timestamp_unix_ms: d.timestamp_unix_ms(),
            })
            .collect();

        // Phase 7 autocmd rows. Project every entry into a stable
        // summary; sort by (plugin_id, generation_id, autocmd_id).
        for entry in self.event_bus.entries() {
            snap.autocmds.push(AutocmdSummary {
                id: entry.id.get(),
                plugin_id: entry.plugin_id.clone(),
                generation_id: entry.generation_id.get(),
                group_id: entry.group.map(|g| g.get()),
                event: entry.canonical_event.to_string(),
                subscribed_event: entry.event.to_string(),
                pattern: entry.options.pattern.clone(),
                debounce_ms: entry.options.debounce_ms,
                priority: entry.options.priority,
                once: entry.options.once,
                source_location: entry.options.source_location.clone(),
                fires: entry.runtime.fires,
                failures: entry.runtime.failures,
                consecutive_failures: entry.runtime.consecutive_failures,
                disabled: entry.runtime.disabled,
            });
        }
        snap.autocmds.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.generation_id.cmp(&b.generation_id))
                .then(a.id.cmp(&b.id))
        });

        // Phase 8 command rows: project the unified registry into
        // sorted devtools rows. Host commands sit under `<host>`.
        let registry = self.command_registry.borrow();
        for entry in registry.entries() {
            let desc = &entry.descriptor;
            snap.commands.push(CommandSummaryRow {
                name: desc.name.clone(),
                title: desc.title.clone(),
                description: desc.description.clone(),
                plugin_id: desc.plugin_id.clone(),
                generation_id: desc.generation_id.map(|g| g.get()),
                context: desc.context.clone(),
                destructive: desc.destructive,
                capabilities: desc.capabilities.clone(),
                fires: entry.runtime.fires,
                failures: entry.runtime.failures,
                last_outcome: entry.runtime.last_outcome.clone(),
                last_duration_ms: entry.runtime.last_duration_ms,
            });
        }
        drop(registry);
        snap.commands
            .sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id).then(a.name.cmp(&b.name)));

        // Phase 9 keymap rows: project the registry's already-sorted
        // summaries straight through.
        let keymap_summaries = self.keymap_registry.borrow().summaries();
        for summary in keymap_summaries {
            snap.keymaps.push(KeymapSummaryRow {
                context: summary.context,
                key: summary.key,
                command: summary.command,
                plugin_id: summary.plugin_id,
                generation_id: summary.generation_id,
                source: summary.source,
                status: summary.status,
                description: summary.description,
                conflict_with: summary.conflict_with.map(|c| KeymapConflictRef {
                    plugin_id: c.plugin_id,
                    source: c.source,
                }),
            });
        }

        // Phase 10 grant snapshot: cheap-clone every row + every
        // open prompt. Inspectors / tests use these to drive the
        // grant lifecycle without poking the store directly.
        snap.capability_grants = self
            .grant_store
            .rows()
            .into_iter()
            .map(CapabilityGrantSummary::from)
            .collect();
        let mut prompts: Vec<PendingPromptSummary> = self
            .grant_store
            .pending_prompts()
            .iter()
            .filter_map(PendingPromptSummary::from_prompt)
            .collect();
        prompts.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.plugin_version.cmp(&b.plugin_version))
        });
        snap.pending_capability_prompts = prompts;

        // Phase 11: surface every recent / in-flight git write.
        snap.pending_git_writes = self.pending_git_writes.entries();

        // Phase 12: project async-runtime registries into the snapshot.
        snap.async_jobs = self
            .async_jobs
            .summaries()
            .into_iter()
            .map(|s| crate::plugin::devtools::AsyncJobSummary {
                plugin_id: s.plugin_id,
                generation_id: s.generation_id,
                job_id: s.job_id,
                started_at_unix_ms: s.started_at_unix_ms,
                status: s.status,
            })
            .collect();
        snap.timers = self
            .timers
            .summaries()
            .into_iter()
            .map(|s| crate::plugin::devtools::TimerSummary {
                plugin_id: s.plugin_id,
                generation_id: s.generation_id,
                timer_id: s.timer_id,
                kind: s.kind,
                interval_ms: s.interval_ms,
                fires: s.fires,
            })
            .collect();
        snap.file_watchers = self
            .watchers
            .summaries()
            .into_iter()
            .map(|s| crate::plugin::devtools::WatcherSummary {
                plugin_id: s.plugin_id,
                generation_id: s.generation_id,
                watch_id: s.watch_id,
                path: s.path,
                recursive: s.recursive,
            })
            .collect();

        // Phase 16 lazy-plugin projection. Builds the inspector row
        // directly from the registry (sorted by plugin_id for
        // stable rendering) and renders trigger descriptors inline
        // — the registry does not own a projection helper, since
        // this is the single consumer.
        let mut lazy_rows: Vec<crate::plugin::devtools::LazyPluginSummary> = self
            .lazy_registry
            .entries()
            .iter()
            .map(|e| {
                let mut triggers: Vec<String> = Vec::new();
                for c in &e.commands {
                    triggers.push(format!("command:{c}"));
                }
                for k in &e.keymaps {
                    triggers.push(format!("keymap:{}:{}", k.context, k.key));
                }
                for ev in &e.events {
                    triggers.push(format!("event:{ev}"));
                }
                for r in &e.regions {
                    triggers.push(format!("region:{r}"));
                }
                for f in &e.files {
                    triggers.push(format!("file:{}", f.display()));
                }
                if e.repository_shape.is_some() {
                    triggers.push("repository_shape".to_string());
                }
                if e.manual {
                    triggers.push("manual".to_string());
                }
                triggers.sort();
                crate::plugin::devtools::LazyPluginSummary {
                    plugin_id: e.plugin_id.clone(),
                    triggers,
                    status: e.status.as_str().to_string(),
                    activations: e.activations,
                    last_activation_unix_ms: e.last_activation_unix_ms,
                    last_activation_trigger: e.last_activation_trigger.clone(),
                    last_error: e.last_error.clone(),
                }
            })
            .collect();
        lazy_rows.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        snap.lazy_plugins = lazy_rows;

        // Phase 17 extension-point projections. The registry already
        // sorts each surface for us (see `ExtensionRegistry`), so the
        // snapshots flow through verbatim.
        snap.overlays = self
            .extension_registry
            .overlays()
            .into_iter()
            .map(|o| crate::plugin::devtools::OverlaySummary {
                plugin_id: o.plugin_id,
                id: o.id,
                priority: o.priority,
                dismissible: o.dismissible,
                widget: o.widget,
                source_location: o.source_location,
            })
            .collect();
        snap.context_menu_items = self
            .extension_registry
            .all_context_menu_items()
            .into_iter()
            .map(|i| crate::plugin::devtools::ContextMenuItemSummary {
                plugin_id: i.plugin_id,
                region: i.region,
                id: i.id,
                label: i.label,
                command: i.command,
                priority: i.priority,
                condition_capability: i.condition_capability,
                source_location: i.source_location,
            })
            .collect();
        snap.graph_decorations = self
            .extension_registry
            .all_graph_decorations()
            .into_iter()
            .map(|d| {
                let kind = d.decoration.kind().to_string();
                let decoration =
                    serde_json::to_value(&d.decoration).unwrap_or(serde_json::Value::Null);
                crate::plugin::devtools::GraphDecorationSummary {
                    plugin_id: d.plugin_id,
                    id: d.id,
                    commit_hash: d.commit_hash,
                    kind,
                    decoration,
                    source_location: d.source_location,
                }
            })
            .collect();
        snap.diff_decorations = self
            .extension_registry
            .all_diff_decorations()
            .into_iter()
            .map(|d| {
                let kind = d.decoration.kind().to_string();
                let decoration =
                    serde_json::to_value(&d.decoration).unwrap_or(serde_json::Value::Null);
                crate::plugin::devtools::DiffDecorationSummary {
                    plugin_id: d.plugin_id,
                    id: d.id,
                    kind,
                    decoration,
                    source_location: d.source_location,
                }
            })
            .collect();

        // Phase 18 performance traces + circuit-breaker rows. Both
        // are cheap clones from the tracker; sort here so the
        // snapshot is deterministic.
        let mut traces: Vec<crate::plugin::devtools::PerformanceTraceSummary> = self
            .budget_tracker
            .traces()
            .into_iter()
            .map(|t| crate::plugin::devtools::PerformanceTraceSummary {
                plugin_id: t.plugin_id,
                generation_id: t.generation_id,
                callback_id: t.callback_id,
                kind: t.kind.as_str().to_string(),
                duration_ms: t.duration_ms,
                ok: t.ok,
                timestamp_unix_ms: t.timestamp_unix_ms,
            })
            .collect();
        traces.sort_by(|a, b| {
            a.timestamp_unix_ms
                .cmp(&b.timestamp_unix_ms)
                .then(a.plugin_id.cmp(&b.plugin_id))
                .then(a.callback_id.cmp(&b.callback_id))
        });
        snap.performance_traces = traces;

        let mut breakers: Vec<crate::plugin::devtools::CircuitBreakerSummary> = self
            .budget_tracker
            .breaker_summaries()
            .into_iter()
            .map(|s| crate::plugin::devtools::CircuitBreakerSummary {
                plugin_id: s.plugin_id,
                generation_id: s.generation_id,
                callback_id: s.callback_id,
                kind: s.kind,
                state: s.state,
                consecutive_failures: s.consecutive_failures,
                count: s.count,
                ok_count: s.ok_count,
                err_count: s.err_count,
                p50_ms: s.p50_ms,
                p95_ms: s.p95_ms,
                last_failure: s.last_failure,
            })
            .collect();
        breakers.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.generation_id.cmp(&b.generation_id))
                .then(a.callback_id.cmp(&b.callback_id))
        });
        snap.circuit_breakers = breakers;

        // Reload history is naturally per-plugin in the store; flatten
        // here in (plugin_id, timestamp) order so external inspectors
        // can group as they please.
        let mut history_keys: Vec<&String> = self.reload_history.keys().collect();
        history_keys.sort();
        for key in history_keys {
            if let Some(bucket) = self.reload_history.get(key) {
                for entry in bucket.iter() {
                    snap.reload_history.push(entry.clone());
                }
            }
        }

        snap
    }

    /// Cheap-cloned reload history for `plugin_id`, oldest first.
    /// Empty when the plugin has never been reloaded since load.
    pub fn reload_history(&self, plugin_id: &str) -> Vec<ReloadEventSummary> {
        self.reload_history
            .get(plugin_id)
            .map(|bucket| bucket.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Number of suspended coroutines parked in this plugin's queue.
    /// Used by tests to drive a coroutine to completion via repeated
    /// `tick()` calls.
    pub fn coroutine_count(&self, plugin_id: &str) -> usize {
        self.plugins
            .get(plugin_id)
            .map(|p| p.deferred.borrow().coroutines.len())
            .unwrap_or(0)
    }

    /// Re-invoke every dynamic widget fn the plugin registered and push
    /// the resulting tree into its shared cache cell. Read by the slot
    /// builder on the next render. Errors are logged; the previous
    /// cached value is left in place so a transient Lua error doesn't
    /// blank out the bar.
    /// Resolve and install one autocmd registration into the live
    /// `EventBus`. Records `autocmd.invalid_event` /
    /// `autocmd.invalid_pattern` diagnostics for shape errors and
    /// drops the row when the event name is unknown.
    fn install_one_autocmd(
        &mut self,
        plugin_id: &str,
        generation_id: GenerationId,
        raw: api::RawAutocmd,
        local_to_host: &HashMap<u64, GroupId>,
    ) {
        let (canonical_name, subscribed_name) = match events::resolve_event(&raw.event) {
            Ok((descriptor, alias)) => {
                let subscribed = alias.unwrap_or(descriptor.name);
                (descriptor.name, subscribed)
            }
            Err(name) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Warning,
                        "autocmd.invalid_event",
                        format!("unknown event `{name}` ignored"),
                    )
                    .with_generation(generation_id)
                    .with_source(make_lua_span(plugin_id, raw.source_location.as_deref())),
                );
                return;
            }
        };
        if let Some(pat) = raw.pattern.as_deref() {
            if pat.is_empty() {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Warning,
                        "autocmd.invalid_pattern",
                        "empty pattern ignored",
                    )
                    .with_generation(generation_id)
                    .with_source(make_lua_span(plugin_id, raw.source_location.as_deref())),
                );
            }
        }
        let options = api::event::build_options(&raw, local_to_host);
        self.event_bus.register(
            plugin_id.to_string(),
            generation_id,
            subscribed_name,
            canonical_name,
            raw.callback,
            options,
        );
    }

    fn refresh_dynamic_widgets_for_plugin(&self, plugin_id: &str) {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return;
        };
        let generation_id = plugin.generation.generation_id;
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        for (slot_id, (key, cache)) in &plugin.dynamic_widgets {
            let func: Function = match plugin.lua().registry_value(key) {
                Ok(f) => f,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("dynamic widget fn lookup failed for {slot_id}: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("dynamic_widget:{slot_id}"),
                        }),
                    );
                    continue;
                }
            };
            // Phase 18: budget the dynamic-widget render against the
            // `UiCallback` budget. UI callbacks have the tightest
            // budgets in the plan because they run on every frame.
            let pid = PluginId::from(plugin_id);
            let cb_id = format!("dynamic_widget:{slot_id}");
            let perf_outcome = self.budget_tracker.track_call::<LuaValue, mlua::Error>(
                CallbackKind::UiCallback,
                &pid,
                generation_id,
                &cb_id,
                || func.call(()),
            );
            let lua_val: LuaValue = match perf_outcome {
                PerfOutcome::Ok(v) => v,
                PerfOutcome::Skipped => continue,
                PerfOutcome::Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            format!("dynamic widget fn error for {slot_id}"),
                        )
                        .with_generation(generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                    continue;
                }
            };
            let json: serde_json::Value = match plugin.lua().from_value(lua_val) {
                Ok(v) => v,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "widget.invalid_tree",
                            format!("dynamic widget returned non-serialisable value: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::Widget {
                            path: format!("slot:{slot_id}"),
                        }),
                    );
                    continue;
                }
            };
            match widget_ast::decode(&json) {
                Ok(ast) => {
                    *cache.borrow_mut() = Some(ast);
                }
                Err(decode_err) => {
                    self.diagnostics.record(widget_decode_diagnostic(
                        plugin_id,
                        generation_id,
                        &format!("slot:{slot_id}"),
                        &decode_err,
                    ));
                    // Leave the cache as-is (previous good AST or `None`).
                }
            }
        }
    }
}

/// Convert a `widget_ast::WidgetDecodeError` into a structured
/// diagnostic. The error path is rooted at `path_root` so screen errors
/// read `screen.<id>.view.children[2].child.label` and slot errors read
/// `slot:<slot_id>.children[…]`.
fn widget_decode_diagnostic(
    plugin_id: &str,
    generation_id: GenerationId,
    path_root: &str,
    err: &widget_ast::WidgetDecodeError,
) -> PluginDiagnostic {
    // The decoder paths are rooted at "root"; strip that and re-root
    // under the host-side path so the diagnostic carries an absolute
    // location.
    let suffix = err.path.strip_prefix("root").unwrap_or(err.path.as_str());
    let full_path = if suffix.is_empty() {
        path_root.to_string()
    } else {
        format!("{path_root}{suffix}")
    };
    PluginDiagnostic::new(
        PluginId::from(plugin_id),
        DiagnosticSeverity::Error,
        err.code,
        err.message.clone(),
    )
    .with_generation(generation_id)
    .with_source(PluginSourceSpan::Widget { path: full_path })
}

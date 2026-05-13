//! Plugin loading, callback dispatch, and resource ownership.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use git_leviathan_plugin_api::descriptor::api as event_descriptor;
use git_leviathan_plugin_api::manifest::PluginManifest;
use mlua::{
    Function, Lua, LuaSerdeExt, RegistryKey, Table, Thread, ThreadStatus, Value as LuaValue,
};

use crate::plugin::api::health::{HealthContext, HealthItem, HealthReport, PluginHealth};
use crate::plugin::api::{
    self, async_api::JobCallbacks, AsyncRuntimeContext, BuildState, DeferredCallback,
    DeferredQueue, HealthCheckRegistration, PersistContext, ServicesContext, UserCommands,
};
use crate::plugin::async_jobs::{AsyncJobRegistry, JobOutcome};
use crate::plugin::audit::AuditLog;
use crate::plugin::bridge::widget_tree::plugin_text_input_id;
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
use crate::plugin::dock::DockManager;
use crate::plugin::events::{
    self, AutocmdOptions, DispatchOutcome, EventBus, EventPayload, GroupId,
    MAX_CONSECUTIVE_FAILURES,
};
use crate::plugin::extensions::{DecorationProviderCallbacks, OverlayCallbacks};
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
use crate::plugin::staged_reload::{
    self, snapshot_previous_screens, stage_reload, ReloadStage, StageInputs, StagingArtifacts,
    StagingFailure,
};
use crate::plugin::storage::{PluginStoragePaths, PluginStorageRoots, StorageSurface};
use crate::plugin::tab_snapshot::{TabChange, TabRegistryOp, TabsSnapshot};
use crate::plugin::timers::{PluginTimerCallbacks, TimerRegistry};
use crate::plugin::ui::invalidation::UiInvalidationCause;
use crate::plugin::ui::main_bar_slots::PreparedSlotOp;
use crate::plugin::ui::split;
use crate::plugin::ui::widget_ast::{self, WidgetAst};
use crate::plugin::watchers::{FileWatcherRegistry, PluginWatcherCallbacks};
use crate::widgets::chrome::main_bar::MainBarRegistry;
use crate::widgets::chrome::repo_region::RepoRegionRegistry;
use crate::widgets::chrome::tab_bar_slots::TabBarRegistry;

use super::slots::{op_belongs_to, op_matches_slot_resource, prepare_op, validate_raw_slot_op};
use super::types::{
    LoadedPlugin, PluginHost, PluginLoadError, RepositoryShapeFacts, SplitDragInfo,
};

struct HostResourceCleaner<'a> {
    slot_ops: &'a mut Vec<PreparedSlotOp>,
    event_bus: &'a mut EventBus,
    service_registry: Rc<RefCell<ServiceRegistry>>,
    dock_manager: DockManager,
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
            PluginResourceKind::DockPanel => {
                self.dock_manager
                    .drop_registration_key(&crate::plugin::dock::panel_key(
                        plugin_id,
                        resource
                            .handle
                            .strip_prefix("dock:")
                            .unwrap_or(resource.handle.as_str()),
                    ));
            }
            _ => {}
        }
        Ok(())
    }
}

const RELOAD_HISTORY_CAP: usize = 64;

/// helper used by `build_diagnostic_bundle` to inline a
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
        let diagnostics = DiagnosticStore::default();
        let mut host = Self {
            plugins: HashMap::new(),
            slot_ops: Vec::new(),
            slot_ops_revision: 0,
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
            diagnostics: diagnostics.clone(),
            runtime_path_registry: RuntimePathRegistry::new(),
            reload_history: HashMap::new(),
            command_registry: Rc::new(RefCell::new(CommandRegistry::new())),
            command_plugin_registry: CommandPluginRegistry::new(),
            pending_command_events: PendingCommandEvents::new(),
            command_active_context: Rc::new(RefCell::new(
                crate::plugin::ui::context::context_for_surface(
                    HOST_COMMAND_PLUGIN_ID,
                    GenerationId::new(0),
                    crate::plugin::ui::context::UiContextSurface::Screen,
                    None,
                    &TabsSnapshot::default(),
                    None,
                ),
            )),
            keymap_registry: Rc::new(RefCell::new(KeymapRegistry::new())),
            grant_store: GrantStore::new_in_memory(),
            auto_grant_policy: AutoGrantPolicy::new(),
            active_gateway: ActiveRepositoryGateway::new(),
            destructive_policy: DestructiveConfirmPolicy::new(),
            pending_git_writes: PendingGitWrites::new(),
            pending_git_events: crate::plugin::api::git::PendingGitEvents::new(),
            pending_ui_effects: crate::plugin::ui::effects::PendingUiEffects::new(),
            async_jobs: AsyncJobRegistry::new(),
            timers: TimerRegistry::new(),
            watchers: FileWatcherRegistry::new(),
            storage_roots: PluginStorageRoots::os_default(),
            dependency_graph: Vec::new(),
            lazy_registry: crate::plugin::activation::LazyRegistry::new(),
            lazy_ledgers: HashMap::new(),
            last_repository_shape: None,
            last_selection_snapshot: crate::plugin::ui::context::SelectionContextSnapshot::none(),
            last_toolbar_dialog_snapshot:
                crate::plugin::ui::context::ToolbarDialogContextSnapshot::none(),
            extension_registry: crate::plugin::extensions::ExtensionRegistry::new(),
            dock_manager: DockManager::new(),
            contribution_overrides:
                crate::plugin::ui::contribution_overrides::ContributionOverrides::new(),
            budget_tracker: BudgetTracker::new(diagnostics),
            disabled_plugins: std::collections::HashSet::new(),
            last_plugin_roots: HashMap::new(),
            devtools_action_queue: Rc::new(RefCell::new(Vec::new())),
            last_devtools_result: None,
            core_command_actions: crate::plugin::core_commands::CoreCommandActions::new(),
            pending_navigation_effects: Vec::new(),
            last_focus_snapshot: crate::plugin::ui::focus::FocusSnapshot::default(),
        };
        host.dock_manager.set_persist_path(
            host.storage_roots
                .state_root
                .join("host")
                .join("dock_layout.json"),
        );
        host.contribution_overrides.set_persist_path(
            host.storage_roots
                .state_root
                .join("host")
                .join("contribution_overrides.json"),
        );
        let local_plugins = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins");
        if local_plugins.is_dir() {
            host.trust_local_plugin_root(local_plugins);
        }
        host.register_builtin_host_commands();
        host.register_builtin_devtools_commands();
        host
    }

    /// async runtime cheap-clone snapshot of the async-job registry.
    /// Devtools renders these directly.
    pub fn async_jobs(&self) -> AsyncJobRegistry {
        self.async_jobs.clone()
    }

    /// async runtime cheap-clone handle to the timer registry.
    pub fn timers(&self) -> TimerRegistry {
        self.timers.clone()
    }

    /// async runtime cheap-clone handle to the file-watcher registry.
    pub fn watchers(&self) -> FileWatcherRegistry {
        self.watchers.clone()
    }

    /// True when at least one plugin has loaded into a Lua state.
    pub fn has_loaded_plugins(&self) -> bool {
        !self.plugins.is_empty()
    }

    pub fn slot_ops_revision(&self) -> u64 {
        self.slot_ops_revision
    }

    fn mark_slot_ops_changed(&mut self) {
        self.slot_ops_revision = self.slot_ops_revision.wrapping_add(1);
    }

    /// The app only needs to poll the Lua runtime while there is work that can
    /// complete independently of normal UI/input events.
    pub fn needs_runtime_tick(&self) -> bool {
        !self.pending_command_events.is_empty()
            || !self.pending_git_events.is_empty()
            || self.async_jobs.has_jobs()
            || self.timers.has_timers()
            || self.watchers.has_watchers()
            || crate::plugin::terminal::registry().has_sessions()
            || self
                .plugins
                .values()
                .any(|plugin| plugin.deferred.borrow().has_pending())
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

    /// repository gateway setter the app calls to keep the host's view of the
    /// active repository in sync. Pass `None` when no repository is
    /// open. The same gateway powers built-in UI ops, so plugin git
    /// reads/writes route through the existing pipeline.
    pub fn set_repository_gateway(
        &mut self,
        gateway: Option<crate::services::SharedRepositoryGateway>,
    ) {
        self.active_gateway.set(gateway);
    }

    /// destructive-policy handle. Devtools / tests use this
    /// to pre-approve a destructive op before invoking it. Cheap-clone.
    pub fn destructive_policy(&self) -> DestructiveConfirmPolicy {
        self.destructive_policy.clone()
    }

    /// cheap-clone snapshot of the recent + in-flight git
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

    /// Drain queued git typed events and fire them through
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

    /// Mark `root` as a local plugin directory. Plugins whose `dir`
    /// canonicalises under this root get their requested capabilities
    /// auto-allowed at load (`decided_by = "default"`).
    pub fn trust_local_plugin_root(&mut self, root: impl Into<PathBuf>) {
        self.auto_grant_policy.trust_root(root);
    }

    /// Hands pending capability decisions to the caller. The caller calls
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

    /// Revoke entry point used by devtools / settings UI.
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
        if capability.starts_with("ui:")
            && self
                .plugins
                .get(plugin_id)
                .is_some_and(|plugin| plugin.manifest.version.to_string() == plugin_version)
        {
            let _ = self.unload_plugin(plugin_id);
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(plugin_id),
                    DiagnosticSeverity::Info,
                    "capability.revoked_ui_unmounted",
                    format!("unmounted UI for {plugin_id} after `{capability}` revocation"),
                )
                .with_context(serde_json::json!({
                    "capability": capability,
                    "plugin_version": plugin_version,
                })),
            );
        }
        Ok(())
    }
}

mod event_helpers;
use event_helpers::*;
use runtime_introspection::widget_decode_diagnostic;

mod commands_devtools;
mod diagnostics_storage;
mod discovery;
mod dock;
mod event_dispatch;
mod loading_and_slots;
mod runtime_introspection;
mod ui_lifecycle;

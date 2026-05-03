//! Transactional plugin reload.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use git_leviathan_plugin_api::api_version::HOST_API_VERSION;
use git_leviathan_plugin_api::manifest::PluginManifest;
use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, Table, Value as LuaValue};

use crate::plugin::api::health::HealthContext;
use crate::plugin::api::{
    self, async_api::JobCallbacks, AsyncRuntimeContext, BuildState, DeferredQueue,
    HealthCheckRegistration, PersistContext, RawAutocmdClear, RawAutocmdGroup, ServicesContext,
    UserCommands,
};
use crate::plugin::audit::AuditLog;
use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::capability_grants::{canonicalize_requested, GrantStore};
use crate::plugin::commands::{CommandDispatchEnv, RawCommand};
use crate::plugin::diagnostic::{DiagnosticSeverity, DiagnosticStore, PluginDiagnostic};
use crate::plugin::generation::PluginGeneration;
use crate::plugin::keymap::{KeymapRegistry, RawKeymap, RawKeymapDel};
use crate::plugin::lua_loader::{install_runtime_module, LuaLoader};
use crate::plugin::resources::{GenerationId, PluginId, ResourceLedger};
use crate::plugin::runtime_path::{PluginRuntimePath, RuntimePathRegistry};
use crate::plugin::services::{dependency_statuses, ServiceHandle, ServiceRegistry};
use crate::plugin::storage::PluginStorageRoots;
use crate::plugin::timers::PluginTimerCallbacks;
use crate::plugin::watchers::PluginWatcherCallbacks;

/// Staging gates. The stage names are exact strings carried in
/// `reload.failed` diagnostics under `context.stage`, and surfaced in
/// devtools `ReloadEventSummary::stage_reached`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadStage {
    ManifestRead,
    ManifestParse,
    Compatibility,
    Capability,
    InstallApi,
    InstallLoader,
    InitLua,
    PrepareSlots,
    Validation,
    HealthCheck,
    Migration,
    Swap,
}

impl ReloadStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManifestRead => "manifest_read",
            Self::ManifestParse => "manifest_parse",
            Self::Compatibility => "compatibility",
            Self::Capability => "capability",
            Self::InstallApi => "install_api",
            Self::InstallLoader => "install_loader",
            Self::InitLua => "init_lua",
            Self::PrepareSlots => "prepare_slots",
            Self::Validation => "validation",
            Self::HealthCheck => "health_check",
            Self::Migration => "migration",
            Self::Swap => "swap",
        }
    }
}

/// Snapshot of one previous-generation screen, captured before any
/// staging work begins. The Lua serialisation runs against the *old*
/// generation's state and is therefore safe to do up front; the
/// resulting JSON is what we hand the staged generation's
/// `M.reload(old_state)` hook (or per-screen `deserialize` fallback).
pub struct PreviousScreenSnapshot {
    pub screen_id: String,
    pub serialized: Option<serde_json::Value>,
}

/// Inputs the host hands to `stage_reload`. Carrying these as a
/// distinct struct keeps the staging entry point free of `&mut
/// PluginHost` so it can't accidentally mutate live state.
pub struct StageInputs<'a> {
    pub plugin_dir: PathBuf,
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
    pub diagnostics: DiagnosticStore,
    pub audit_log: AuditLog,
    pub runtime_path_registry: &'a RuntimePathRegistry,
    pub previous_generation_id: GenerationId,
    pub previous_screens: Vec<PreviousScreenSnapshot>,
    pub previous_capabilities: Vec<String>,
    pub previous_screen_ids: Vec<String>,
    pub pending_tab_ops: Rc<RefCell<Vec<crate::plugin::tab_snapshot::TabRegistryOp>>>,
    /// Command dispatch env handed to the staged generation's
    /// `leviathan.command` API installer. A failed staging drops the
    /// staged generation cleanly; the live registry is never touched
    /// during staging. On commit, the host moves
    /// [`StagingArtifacts::commands`] into the live registry.
    pub command_dispatch: CommandDispatchEnv,
    /// capability grant store consulted at the `Capability` stage. A
    /// staged reload widening the requested set without a matching
    /// grant halts here (`capability.upgrade_required`).
    pub grant_store: GrantStore,
    /// Whether bundled-plugin auto-grant applies to this staging
    /// (i.e. the plugin's root is under a trusted bundled root).
    pub bundled: bool,
    /// keymaps: live keymap registry handle. The staged generation's
    /// `leviathan.keymap.list` shim borrows from this so plugins see
    /// the same resolved table the host sees. Staged `set` / `del`
    /// rows are NOT installed here — they ride on
    /// [`StagingArtifacts::keymaps`] / [`StagingArtifacts::keymap_dels`]
    /// and the host's `commit_staging` splices them in atomically.
    pub keymap_registry: Rc<RefCell<KeymapRegistry>>,
    /// Cheap-clone GitOpsContext shared with the staged
    /// generation's git/repo Lua APIs.
    pub git_ctx: crate::plugin::git_ops::GitOpsContext,
    /// Cheap-clone outbound queue of typed git events the
    /// staged generation's API will push to.
    pub pending_git_events: crate::plugin::api::git::PendingGitEvents,
    /// Cheap-clone async runtime registries shared across
    /// generations. The staged generation's `leviathan.async`,
    /// `leviathan.timer`, and `leviathan.fs.watch` shims push into
    /// these via fresh per-generation callback maps.
    pub async_jobs: crate::plugin::async_jobs::AsyncJobRegistry,
    pub timers: crate::plugin::timers::TimerRegistry,
    pub watchers: crate::plugin::watchers::FileWatcherRegistry,
    pub storage_roots: PluginStorageRoots,
    pub service_registry: Rc<RefCell<ServiceRegistry>>,
    /// Shared extension registry. The staged generation's
    /// new init runs against the live registry — the host clears the
    /// previous generation's records on commit (see
    /// `commit_staging`).
    pub extension_registry: crate::plugin::extensions::ExtensionRegistry,
}

/// Fully-constructed staging generation, ready for atomic swap.
///
/// Held briefly between `stage_reload` returning and
/// `PluginHost::commit_staging` consuming it. If commit is skipped the
/// staging gen drops cleanly: the lua state, ledger, services
/// registry, after-files, and runtime-path entry all go with it.
pub struct StagingArtifacts {
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
    pub plugin_root: PathBuf,
    pub manifest: PluginManifest,
    pub generation: PluginGeneration,
    pub slot_handlers: HashMap<String, RegistryKey>,
    pub screens: HashMap<String, api::ScreenDef>,
    pub screen_state: HashMap<String, RegistryKey>,
    pub dynamic_widgets: HashMap<
        String,
        (
            RegistryKey,
            crate::plugin::ui::main_bar_slots::DynamicAstCache,
        ),
    >,
    pub deferred: Rc<RefCell<DeferredQueue>>,
    pub user_commands: Rc<RefCell<UserCommands>>,
    pub health_checks: Vec<HealthCheckRegistration>,
    pub lua_loader: LuaLoader,
    pub slot_ops: Vec<crate::plugin::ui::main_bar_slots::PreparedSlotOp>,
    pub autocmds: Vec<StagedAutocmd>,
    pub autocmd_groups: Vec<RawAutocmdGroup>,
    pub autocmd_clears: Vec<RawAutocmdClear>,
    pub staged_services: Rc<RefCell<ServiceRegistry>>,
    pub started_at: Instant,
    /// Command rows the staged generation registered. The
    /// host swaps these into the live `CommandRegistry` during
    /// `commit_staging`. Dropped wholesale on rollback.
    pub commands: Vec<RawCommand>,
    /// Capability guard built for the staged generation. The
    /// host installs it into the dispatcher's plugin registry on
    /// commit so the new generation's commands route capability
    /// checks through the right grant set.
    pub capability_guard: Rc<CapabilityGuard>,
    /// staged keymap rows. Installed into the live
    /// `KeymapRegistry` during `commit_staging` after the previous
    /// generation's bindings have been dropped — a failed staging
    /// drops these on the floor.
    pub keymaps: Vec<RawKeymap>,
    /// staged keymap deletions. Applied after the staged
    /// `set` rows so a plugin can rebind its own keymaps in one
    /// init.lua run.
    pub keymap_dels: Vec<RawKeymapDel>,
    /// async runtime per-generation async-runtime callback maps. The staged
    /// generation's `leviathan.async.spawn`, `leviathan.timer.*`, and
    /// `leviathan.fs.watch` shims insert into these; commit moves
    /// them into the live `LoadedPlugin`.
    pub job_callbacks: Rc<RefCell<JobCallbacks>>,
    pub timer_callbacks: Rc<RefCell<PluginTimerCallbacks>>,
    pub watcher_callbacks: Rc<RefCell<PluginWatcherCallbacks>>,
}

/// Staged autocmd registration ready for `commit_staging` to install
/// into the live `EventBus`. The plugin-local `options_local_group`
/// is resolved into a host-wide
/// [`crate::plugin::events::GroupId`] at commit time.
pub struct StagedAutocmd {
    /// Name the plugin actually subscribed under — alias or
    /// canonical. Drives dispatch matching.
    pub subscribed_event: &'static str,
    /// Canonical event name resolved from the descriptor table.
    pub canonical_event: &'static str,
    pub callback: RegistryKey,
    pub options_local_group: Option<u64>,
    pub once: bool,
    pub pattern: Option<String>,
    pub debounce_ms: u64,
    pub priority: i32,
    /// Plugin-local declaration sequence number; the host replays
    /// staged autocmds in this order so a `clear` issued *after* an
    /// `autocmd.create` (in source order) correctly removes that
    /// registration.
    pub sequence: u64,
    pub source_location: Option<String>,
}

/// Reason a stage refused to advance. Carries the failed stage and a
/// stable `cause` string the host emits as `context.cause` on the
/// `reload.failed` diagnostic.
#[derive(Debug)]
pub struct StagingFailure {
    pub stage: ReloadStage,
    pub cause: String,
    pub message: String,
}

impl StagingFailure {
    pub fn new(stage: ReloadStage, cause: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            stage,
            cause: cause.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for StagingFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "reload failed at stage {} ({}): {}",
            self.stage.as_str(),
            self.cause,
            self.message
        )
    }
}

impl std::error::Error for StagingFailure {}

/// Build the staging generation. Returns the artefacts for
/// `commit_staging` to swap in, or a `StagingFailure` describing
/// exactly which stage refused. The previous generation is never
/// touched in this function.
#[allow(clippy::too_many_arguments)]
pub fn stage_reload(inputs: StageInputs<'_>) -> Result<StagingArtifacts, StagingFailure> {
    let started_at = Instant::now();

    inputs.diagnostics.record(
        PluginDiagnostic::new(
            inputs.plugin_id.clone(),
            DiagnosticSeverity::Info,
            "reload.staging_started",
            format!(
                "staging generation {} (previous {})",
                inputs.generation_id, inputs.previous_generation_id
            ),
        )
        .with_generation(inputs.generation_id)
        .with_context(serde_json::json!({
            "previous_generation_id": inputs.previous_generation_id.get(),
        })),
    );

    let manifest_path = inputs.plugin_dir.join("plugin.toml");
    let manifest_str = std::fs::read_to_string(&manifest_path).map_err(|e| {
        StagingFailure::new(
            ReloadStage::ManifestRead,
            "manifest.read_failed",
            format!("could not read {}: {e}", manifest_path.display()),
        )
    })?;
    let manifest: PluginManifest = toml::from_str(&manifest_str).map_err(|e| {
        StagingFailure::new(
            ReloadStage::ManifestParse,
            "manifest.parse_failed",
            format!("invalid {}: {e}", manifest_path.display()),
        )
    })?;
    if manifest.id != inputs.plugin_id.as_str() {
        return Err(StagingFailure::new(
            ReloadStage::ManifestParse,
            "manifest.id_changed",
            format!(
                "manifest id `{}` does not match loaded plugin id `{}`",
                manifest.id, inputs.plugin_id
            ),
        ));
    }
    if !manifest.api_version.is_compatible_with(HOST_API_VERSION) {
        return Err(StagingFailure::new(
            ReloadStage::Compatibility,
            "manifest.incompatible_api_version",
            format!(
                "api version {}.{} not compatible with host {}.{}",
                manifest.api_version.major,
                manifest.api_version.minor,
                HOST_API_VERSION.major,
                HOST_API_VERSION.minor
            ),
        ));
    }

    {
        let registry = inputs.service_registry.borrow();
        for status in dependency_statuses(
            &manifest.id,
            &manifest.provides_services,
            &manifest.consumes_services,
            &registry,
        ) {
            if status.satisfied {
                continue;
            }
            if status.required {
                return Err(StagingFailure::new(
                    ReloadStage::Compatibility,
                    "services.required_missing",
                    format!(
                        "required service `{}` is not registered before `{}` reloads",
                        status.service_key, manifest.id
                    ),
                ));
            }
            inputs.diagnostics.record(
                PluginDiagnostic::new(
                    inputs.plugin_id.clone(),
                    DiagnosticSeverity::Warning,
                    "services.optional_missing",
                    format!(
                        "optional service `{}` is not registered; consumer will receive nil",
                        status.service_key
                    ),
                )
                .with_generation(inputs.generation_id)
                .with_context(serde_json::json!({
                    "service": status.service_key,
                    "required": false,
                })),
            );
        }
    }

    // Detect whether any newly-declared capability was previously absent.
    // capability grants: widening detection halts here when the new set
    // contains capabilities the grant store hasn't decided about yet
    // (`capability.upgrade_required`). Bundled plugins auto-grant new
    // capabilities; non-bundled plugins force the user through a
    // prompt before the staged generation can commit.
    let previous_caps: std::collections::HashSet<&str> = inputs
        .previous_capabilities
        .iter()
        .map(String::as_str)
        .collect();
    let requested_strings = canonicalize_requested(&manifest.capabilities);
    let widened: Vec<String> = requested_strings
        .iter()
        .filter(|c| !previous_caps.contains(c.as_str()))
        .cloned()
        .collect();
    if !widened.is_empty() {
        inputs.diagnostics.record(
            PluginDiagnostic::new(
                inputs.plugin_id.clone(),
                DiagnosticSeverity::Info,
                "reload.capability_widened",
                format!(
                    "staging generation requested {} new capabilities: {}",
                    widened.len(),
                    widened.join(", ")
                ),
            )
            .with_generation(inputs.generation_id)
            .with_context(serde_json::json!({ "added": widened.clone() })),
        );
    }
    let plugin_version_str = manifest.version.to_string();
    if inputs.bundled {
        // Bundled plugins auto-grant whatever they request. Existing
        // `Allow` rows are reused; only the truly new ones are added.
        let granted = inputs.grant_store.auto_grant_bundled(
            inputs.plugin_id.as_str(),
            &plugin_version_str,
            &requested_strings,
        );
        for row in granted {
            inputs.audit_log.record(
                &row.plugin_id,
                "grant.allowed",
                format!("{}|by={}", row.capability, row.decided_by.as_str()),
                crate::plugin::audit::AuditOutcome::Allowed,
            );
        }
    } else {
        let undecided = inputs.grant_store.undecided_for(
            inputs.plugin_id.as_str(),
            &plugin_version_str,
            &requested_strings,
        );
        if !undecided.is_empty() {
            inputs.diagnostics.record(
                PluginDiagnostic::new(
                    inputs.plugin_id.clone(),
                    DiagnosticSeverity::Error,
                    "capability.upgrade_required",
                    format!(
                        "staging halted: {} new capabilities require user approval",
                        undecided.len()
                    ),
                )
                .with_generation(inputs.generation_id)
                .with_context(serde_json::json!({
                    "added": undecided.clone(),
                    "plugin_version": plugin_version_str,
                })),
            );
            inputs.audit_log.record(
                inputs.plugin_id.as_str(),
                "grant.upgrade_prompted",
                format!("{} pending", undecided.len()),
                crate::plugin::audit::AuditOutcome::Denied,
            );
            return Err(StagingFailure::new(
                ReloadStage::Capability,
                "capability.upgrade_required",
                format!(
                    "user has not granted {} capability/ies for {}@{}",
                    undecided.len(),
                    manifest.id,
                    plugin_version_str
                ),
            ));
        }
    }

    let init_path = inputs.plugin_dir.join("init.lua");
    let init_src = std::fs::read_to_string(&init_path).map_err(|e| {
        StagingFailure::new(
            ReloadStage::InitLua,
            "lua.read_failed",
            format!("could not read init.lua: {e}"),
        )
    })?;

    let plugin_root =
        std::fs::canonicalize(&inputs.plugin_dir).unwrap_or_else(|_| inputs.plugin_dir.clone());
    let storage_paths = inputs
        .storage_roots
        .for_plugin(&manifest.id, plugin_root.clone());
    let state_dir = storage_paths.state_dir.clone();
    let config_dir = storage_paths.config_dir.clone();

    let lua = Rc::new(Lua::new());
    let generation = PluginGeneration::new(
        inputs.plugin_id.clone(),
        inputs.generation_id,
        Rc::clone(&lua),
    );
    let ledger = generation.ledger.clone();

    let guard = Rc::new(
        CapabilityGuard::new(
            manifest.id.clone(),
            plugin_version_str.clone(),
            manifest.capabilities.clone(),
            crate::plugin::capabilities::CapabilityPaths {
                plugin_root: plugin_root.clone(),
                state_dir,
                config_dir,
                workdir: None,
            },
            inputs.grant_store.clone(),
        )
        .with_audit(inputs.audit_log.clone())
        .with_diagnostics(
            inputs.diagnostics.clone(),
            inputs.plugin_id.clone(),
            inputs.generation_id,
        ),
    );

    let build: Rc<RefCell<BuildState>> = Rc::new(RefCell::new(BuildState::default()));
    let deferred: Rc<RefCell<DeferredQueue>> = Rc::new(RefCell::new(DeferredQueue::default()));
    let user_commands: Rc<RefCell<UserCommands>> = Rc::new(RefCell::new(UserCommands::default()));
    let health_checks_sink: Rc<RefCell<Vec<HealthCheckRegistration>>> =
        Rc::new(RefCell::new(Vec::new()));

    // Keep staged services invisible until commit.
    let staged_services: Rc<RefCell<ServiceRegistry>> =
        Rc::new(RefCell::new(ServiceRegistry::new()));
    let services_ctx = ServicesContext {
        registry: Rc::clone(&staged_services),
        lookup_registry: Some(Rc::clone(&inputs.service_registry)),
        plugin_id: manifest.id.clone(),
        generation_id: inputs.generation_id,
        provides: manifest.provides_services.clone(),
        consumes: manifest.consumes_services.clone(),
        plugin_lua: Rc::clone(&lua),
        capability_guard: Rc::clone(&guard),
    };

    let persist_ctx = PersistContext {
        storage: storage_paths,
    };

    let job_callbacks: Rc<RefCell<JobCallbacks>> = Rc::new(RefCell::new(JobCallbacks::new()));
    let timer_callbacks: Rc<RefCell<PluginTimerCallbacks>> =
        Rc::new(RefCell::new(PluginTimerCallbacks::new()));
    let watcher_callbacks: Rc<RefCell<PluginWatcherCallbacks>> =
        Rc::new(RefCell::new(PluginWatcherCallbacks::new()));
    let async_ctx = AsyncRuntimeContext {
        jobs: inputs.async_jobs.clone(),
        timers: inputs.timers.clone(),
        watchers: inputs.watchers.clone(),
        job_callbacks: Rc::clone(&job_callbacks),
        timer_callbacks: Rc::clone(&timer_callbacks),
        watcher_callbacks: Rc::clone(&watcher_callbacks),
    };

    api::install_all(
        &lua,
        Rc::clone(&build),
        Rc::clone(&inputs.pending_tab_ops),
        Rc::clone(&guard),
        services_ctx,
        persist_ctx,
        Rc::clone(&deferred),
        Rc::clone(&user_commands),
        Rc::clone(&health_checks_sink),
        ledger.clone(),
        inputs.command_dispatch.clone(),
        Rc::clone(&inputs.keymap_registry),
        inputs.git_ctx.clone(),
        inputs.pending_git_events.clone(),
        async_ctx,
        inputs.plugin_id.clone(),
        inputs.generation_id,
        inputs.diagnostics.clone(),
        inputs.extension_registry.clone(),
    )
    .map_err(|e| {
        StagingFailure::new(
            ReloadStage::InstallApi,
            "host.install_api_failed",
            format!("install_all: {e}"),
        )
    })?;

    let runtime_path = PluginRuntimePath::resolve(
        &manifest.id,
        &plugin_root,
        &manifest.requires_plugins,
        inputs.runtime_path_registry,
    );
    let lua_loader = LuaLoader::new(
        inputs.plugin_id.clone(),
        inputs.generation_id,
        runtime_path,
        inputs.diagnostics.clone(),
        manifest.runtime.strict_globals,
    );
    lua_loader.install(&lua).map_err(|e| {
        StagingFailure::new(
            ReloadStage::InstallLoader,
            "host.install_loader_failed",
            format!("loader.install: {e}"),
        )
    })?;
    {
        let leviathan: Table = lua.globals().get("leviathan").map_err(|e| {
            StagingFailure::new(
                ReloadStage::InstallLoader,
                "host.leviathan_global_missing",
                format!("`leviathan` global missing: {e}"),
            )
        })?;
        install_runtime_module(&lua, &leviathan, lua_loader.clone()).map_err(|e| {
            StagingFailure::new(
                ReloadStage::InstallLoader,
                "host.install_runtime_module_failed",
                format!("install_runtime_module: {e}"),
            )
        })?;
    }

    let chunk_name = format!("plugins/{}/init.lua", manifest.id);
    // Eval captures the returned module table for `M.reload(old_state)`.
    let init_return: LuaValue = lua
        .load(&init_src)
        .set_name(chunk_name.clone())
        .eval()
        .map_err(|e| {
            StagingFailure::new(
                ReloadStage::InitLua,
                "lua.init_failed",
                format!("init.lua exec: {e}"),
            )
        })?;

    lua_loader.run_after_plugin(&lua, &plugin_root);

    let (
        screens,
        raw_slot_ops,
        raw_autocmds,
        raw_autocmd_groups,
        raw_autocmd_clears,
        raw_commands,
        raw_keymaps,
        raw_keymap_dels,
    ) = {
        let mut b = build.borrow_mut();
        (
            std::mem::take(&mut b.screens),
            std::mem::take(&mut b.slot_ops),
            std::mem::take(&mut b.autocmds),
            std::mem::take(&mut b.autocmd_groups),
            std::mem::take(&mut b.autocmd_clears),
            std::mem::take(&mut b.commands),
            std::mem::take(&mut b.keymaps),
            std::mem::take(&mut b.keymap_dels),
        )
    };

    let mut slot_handlers: HashMap<String, RegistryKey> = HashMap::new();
    let mut dynamic_widgets: HashMap<
        String,
        (
            RegistryKey,
            crate::plugin::ui::main_bar_slots::DynamicAstCache,
        ),
    > = HashMap::new();
    let mut slot_ops = Vec::new();
    for op in raw_slot_ops {
        let prepared = crate::plugin::host::prepare_op(
            &manifest.id,
            &plugin_root,
            op,
            &ledger,
            &mut slot_handlers,
            &mut dynamic_widgets,
        )
        .map_err(|e| {
            StagingFailure::new(
                ReloadStage::PrepareSlots,
                "schema.slot_invalid",
                format!("slot op rejected: {e}"),
            )
        })?;
        slot_ops.push(prepared);
    }

    // Dynamic widget call/type failures abort staging. AST decode failures
    // stay diagnostics so unrelated widget regressions do not block reload.
    for (slot_id, (key, cache)) in &dynamic_widgets {
        let func: Function = lua.registry_value(key).map_err(|e| {
            StagingFailure::new(
                ReloadStage::Validation,
                "widget.dynamic_lookup_failed",
                format!("dynamic widget `{slot_id}` lookup failed: {e}"),
            )
        })?;
        let value: LuaValue = func.call(()).map_err(|e| {
            StagingFailure::new(
                ReloadStage::Validation,
                "widget.dynamic_call_failed",
                format!("dynamic widget `{slot_id}` raised: {e}"),
            )
        })?;
        let json: serde_json::Value = lua.from_value(value).map_err(|e| {
            StagingFailure::new(
                ReloadStage::Validation,
                "widget.dynamic_serialise_failed",
                format!("dynamic widget `{slot_id}` returned non-JSON: {e}"),
            )
        })?;
        match crate::plugin::ui::widget_ast::decode(&json) {
            Ok(ast) => {
                *cache.borrow_mut() = Some(ast);
            }
            Err(e) => {
                inputs.diagnostics.record(
                    PluginDiagnostic::new(
                        inputs.plugin_id.clone(),
                        DiagnosticSeverity::Warning,
                        "widget.invalid_tree",
                        format!(
                            "staged dynamic widget `{slot_id}` first decode: {} ({})",
                            e.message, e.path
                        ),
                    )
                    .with_generation(inputs.generation_id),
                );
            }
        }
    }

    // `provides_services` entries must register during init.
    {
        let staged = staged_services.borrow();
        for decl in &manifest.provides_services {
            let key = ServiceRegistry::key(decl);
            if !staged
                .handles_iter()
                .any(|h| h.decl.name == decl.name && h.decl.version == decl.version)
            {
                return Err(StagingFailure::new(
                    ReloadStage::Validation,
                    "services.missing_registration",
                    format!("manifest declares `{key}` but init.lua never registered it"),
                ));
            }
        }
    }

    // Error health items abort the swap; warnings/info pass.
    let staged_health: Vec<HealthCheckRegistration> =
        std::mem::take(&mut *health_checks_sink.borrow_mut());
    for check in &staged_health {
        let func: Function = lua.registry_value(&check.callback).map_err(|e| {
            StagingFailure::new(
                ReloadStage::HealthCheck,
                "health.lookup_failed",
                format!("health callback lookup: {e}"),
            )
        })?;
        let bucket: Rc<RefCell<Vec<crate::plugin::api::health::HealthItem>>> =
            Rc::new(RefCell::new(Vec::new()));
        let ctx = HealthContext {
            items: Rc::clone(&bucket),
        };
        func.call::<()>(ctx).map_err(|e| {
            StagingFailure::new(
                ReloadStage::HealthCheck,
                "health.callback_failed",
                format!("health callback raised: {e}"),
            )
        })?;
        let items = bucket.borrow();
        if let Some(err_item) = items
            .iter()
            .find(|i| i.severity == crate::plugin::api::health::Severity::Error)
        {
            return Err(StagingFailure::new(
                ReloadStage::HealthCheck,
                "health.reported_error",
                format!("health check reported error: {}", err_item.message),
            ));
        }
    }

    let mut migrated_screens: HashMap<String, serde_json::Value> = HashMap::new();
    for snap in &inputs.previous_screens {
        if let Some(json) = snap.serialized.clone() {
            migrated_screens.insert(snap.screen_id.clone(), json);
        }
    }

    if let LuaValue::Table(module) = &init_return {
        if let Ok(reload_fn) = module.get::<Function>("reload") {
            inputs.diagnostics.record(
                PluginDiagnostic::new(
                    inputs.plugin_id.clone(),
                    DiagnosticSeverity::Info,
                    "reload.migration_invoked",
                    "M.reload(old_state) hook invoked".to_string(),
                )
                .with_generation(inputs.generation_id),
            );
            let old_state_json = serde_json::json!({
                "screens": migrated_screens.clone(),
            });
            let old_state_lua: LuaValue = lua.to_value(&old_state_json).map_err(|e| {
                StagingFailure::new(
                    ReloadStage::Migration,
                    "reload.migration_failed",
                    format!("could not marshal old_state to lua: {e}"),
                )
            })?;
            let returned: LuaValue = reload_fn.call(old_state_lua).map_err(|e| {
                let diag = PluginDiagnostic::new(
                    inputs.plugin_id.clone(),
                    DiagnosticSeverity::Error,
                    "reload.migration_failed",
                    "M.reload(old_state) raised".to_string(),
                )
                .with_generation(inputs.generation_id)
                .with_mlua_error(&chunk_name, &e);
                inputs.diagnostics.record(diag);
                StagingFailure::new(
                    ReloadStage::Migration,
                    "reload.migration_failed",
                    format!("M.reload raised: {e}"),
                )
            })?;
            // Nil means keep the prior serialized state.
            if !matches!(returned, LuaValue::Nil) {
                let returned_json: serde_json::Value = lua.from_value(returned).map_err(|e| {
                    StagingFailure::new(
                        ReloadStage::Migration,
                        "reload.migration_failed",
                        format!("M.reload return value not JSON-serialisable: {e}"),
                    )
                })?;
                if !returned_json.is_object() {
                    return Err(StagingFailure::new(
                        ReloadStage::Migration,
                        "reload.migration_failed",
                        format!(
                            "M.reload must return a table; got {:?}",
                            returned_json.as_str().unwrap_or("non-string")
                        ),
                    ));
                }
                migrated_screens.clear();
                if let Some(screens) = returned_json.get("screens").and_then(|v| v.as_object()) {
                    for (sid, sval) in screens {
                        migrated_screens.insert(sid.clone(), sval.clone());
                    }
                }
            }
        }
    }

    // Seed staged screen state from migrated snapshots. Use each
    // screen's `deserialize` hook when present; otherwise re-init via
    // the screen's own `init` so the new generation has a sane
    // starting state without smuggling Lua values across VMs.
    let mut screen_state: HashMap<String, RegistryKey> = HashMap::new();
    for screen_id in &inputs.previous_screen_ids {
        let Some(screen_def) = screens.get(screen_id) else {
            continue;
        };
        let mut seeded = false;
        if let Some(snap_json) = migrated_screens.get(screen_id) {
            if let Some(de_key) = &screen_def.deserialize {
                if let Ok(de_fn) = lua.registry_value::<Function>(de_key) {
                    if let Ok(snap_lua) = lua.to_value(snap_json) {
                        if let Ok(state_lua) = de_fn.call::<LuaValue>(snap_lua) {
                            if let Ok(state_key) = lua.create_registry_value(state_lua) {
                                record_screen_state_resource(&ledger, screen_id);
                                screen_state.insert(screen_id.clone(), state_key);
                                seeded = true;
                            }
                        }
                    }
                }
            }
        }
        if !seeded {
            if let Ok(init_fn) = lua.registry_value::<Function>(&screen_def.init) {
                if let Ok(state) = init_fn.call::<LuaValue>(()) {
                    if let Ok(key) = lua.create_registry_value(state) {
                        record_screen_state_resource(&ledger, screen_id);
                        screen_state.insert(screen_id.clone(), key);
                    }
                }
            }
        }
    }

    let mut autocmds: Vec<StagedAutocmd> = Vec::with_capacity(raw_autocmds.len());
    for raw in raw_autocmds {
        let (canonical_event, subscribed_event) =
            match crate::plugin::events::resolve_event(&raw.event) {
                Ok((descriptor, alias)) => {
                    let subscribed = alias.unwrap_or(descriptor.name);
                    (descriptor.name, subscribed)
                }
                Err(name) => {
                    inputs.diagnostics.record(
                        PluginDiagnostic::new(
                            inputs.plugin_id.clone(),
                            DiagnosticSeverity::Warning,
                            "autocmd.invalid_event",
                            format!("staging dropped autocmd for unknown event `{name}`"),
                        )
                        .with_generation(inputs.generation_id),
                    );
                    continue;
                }
            };
        autocmds.push(StagedAutocmd {
            subscribed_event,
            canonical_event,
            callback: raw.callback,
            options_local_group: raw.local_group,
            once: raw.once,
            pattern: raw.pattern,
            debounce_ms: raw.debounce_ms,
            priority: raw.priority,
            sequence: raw.sequence,
            source_location: raw.source_location,
        });
    }

    let _ = lua;
    Ok(StagingArtifacts {
        plugin_id: inputs.plugin_id,
        generation_id: inputs.generation_id,
        plugin_root,
        manifest,
        generation,
        slot_handlers,
        screens,
        screen_state,
        dynamic_widgets,
        deferred,
        user_commands,
        health_checks: staged_health,
        lua_loader,
        slot_ops,
        autocmds,
        autocmd_groups: raw_autocmd_groups,
        autocmd_clears: raw_autocmd_clears,
        staged_services,
        started_at,
        commands: raw_commands,
        capability_guard: Rc::clone(&guard),
        keymaps: raw_keymaps,
        keymap_dels: raw_keymap_dels,
        job_callbacks,
        timer_callbacks,
        watcher_callbacks,
    })
}

/// Drain every staged `ServiceHandle` out of the staged registry so
/// the host can transfer them into the live registry under one lock.
pub fn drain_staged_services(
    staged: &Rc<RefCell<ServiceRegistry>>,
    plugin_id: &str,
) -> Vec<(String, ServiceHandle)> {
    staged.borrow_mut().drain_for_plugin(plugin_id)
}

fn record_screen_state_resource(ledger: &ResourceLedger, screen_id: &str) {
    use crate::plugin::resources::PluginResourceKind;
    let handle = format!("screen_state:{screen_id}");
    ledger.remove_by_kind_handle(PluginResourceKind::PersistedScreenState, &handle);
    ledger.remove_by_kind_handle(PluginResourceKind::LuaRegistryKey, &handle);
    ledger.record(
        PluginResourceKind::PersistedScreenState,
        handle.clone(),
        None,
    );
    ledger.record(PluginResourceKind::LuaRegistryKey, handle, None);
}

/// Convenience for the host: where staging looks for the active
/// plugin's screen-state Lua key. Lets the host serialise the
/// previous generation's screens through the same code path it uses
/// for the regular runtime serialization helpers.
pub fn snapshot_previous_screens(
    plugin_id: &str,
    plugin_lua: &Lua,
    screens: &HashMap<String, api::ScreenDef>,
    screen_state: &HashMap<String, RegistryKey>,
) -> Vec<PreviousScreenSnapshot> {
    let _ = plugin_id;
    let mut out = Vec::new();
    for (screen_id, state_key) in screen_state.iter() {
        let mut serialized: Option<serde_json::Value> = None;
        if let Some(screen_def) = screens.get(screen_id) {
            if let Some(ser_key) = &screen_def.serialize {
                if let Ok(ser_fn) = plugin_lua.registry_value::<Function>(ser_key) {
                    if let Ok(state_val) = plugin_lua.registry_value::<LuaValue>(state_key) {
                        if let Ok(snap_lua) = ser_fn.call::<LuaValue>(state_val) {
                            if let Ok(json) = plugin_lua.from_value(snap_lua) {
                                serialized = Some(json);
                            }
                        }
                    }
                }
            }
        }
        out.push(PreviousScreenSnapshot {
            screen_id: screen_id.clone(),
            serialized,
        });
    }
    out
}

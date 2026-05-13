//! Devtools introspection. `PluginHost::introspect()` returns a
//! point-in-time snapshot of the host's state for in-app inspectors,
//! tests, and external tools.

use crate::plugin::audit::AuditEntry;
pub use crate::plugin::capability_grants::{CapabilityGrantSummary, PendingPromptSummary};
pub use crate::plugin::git_ops::PendingGitWrite;

#[derive(Debug, Clone)]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub debug: bool,
    pub last_reload_error: Option<String>,
    pub provides_services: Vec<String>,
    pub consumes_services: Vec<String>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SlotSummary {
    pub region: String,
    pub container: String,
    pub id: String,
    pub priority: i32,
    pub owner_plugin_id: String,
    pub hidden: bool,
    pub effective_priority: i32,
    pub replace_capability: Option<String>,
    pub remove_capability: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ContributionOverrideSummary {
    pub plugin_id: String,
    pub region: String,
    pub container: String,
    pub id: String,
    pub hidden: bool,
    pub priority: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct UiRenderDiagnosticSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub region: String,
    pub container: String,
    pub slot_id: String,
    pub dependencies: Vec<String>,
    pub last_refresh_causes: Vec<String>,
    pub refresh_count: u64,
    pub skipped_count: u64,
    pub stale_count: u64,
    pub last_duration_ms: u128,
    pub last_status: String,
    pub last_error: Option<String>,
    pub diagnostic_badge: bool,
}

#[derive(Debug, Clone)]
pub struct ServiceSummary {
    pub key: String,
    pub publisher_plugin_id: String,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ServiceGraphEdge {
    pub consumer_plugin_id: String,
    pub provider_plugin_id: Option<String>,
    pub service_key: String,
    pub required: bool,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ServiceCallTraceSummary {
    pub caller_plugin_id: String,
    pub provider_plugin_id: String,
    pub service_key: String,
    pub method: String,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u128,
    pub timestamp_unix_ms: u128,
}

#[derive(Debug, Clone)]
pub struct ResourceSummary {
    pub resource_id: u64,
    pub plugin_id: String,
    pub generation_id: u64,
    pub kind: String,
    pub handle: String,
    pub source_location: Option<String>,
    pub created_at_unix_ms: u128,
}

/// Diagnostic projected for the inspector. Carries the structured
/// fields verbatim (severity/code/source) plus a serialised JSON copy
/// so external inspectors don't need to depend on the host crate.
#[derive(Debug, Clone)]
pub struct DiagnosticSummary {
    pub plugin_id: String,
    pub generation_id: Option<u64>,
    pub severity: String,
    pub code: String,
    pub message: String,
    pub source: Option<String>,
    pub context: serde_json::Value,
    pub timestamp_unix_ms: u128,
}

/// One row of the plugin package layout runtime-path inspector. Plugins are sorted
/// by `plugin_id`; entries within each plugin are in search order
/// (own first, then `requires_plugins` in manifest order).
#[derive(Debug, Clone)]
pub struct RuntimePathSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub entry_plugin_id: String,
    pub kind: String,
    pub root: String,
}

/// Per-plugin loaded-module summary projected from the live module
/// cache. Records what `require()` has actually fetched in this
/// generation. Reload drops the cache, so stale generations never
/// appear here.
#[derive(Debug, Clone)]
pub struct LoadedModuleSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub module_name: String,
    pub source_plugin_id: String,
    pub source_path: String,
    pub kind: String,
}

/// Outcome of a single `reload_plugin` attempt. `Succeeded` means the
/// staging generation became live and the previous one was cleaned up
/// in full. `Failed` means staging aborted and the previous generation
/// is still serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadOutcome {
    Succeeded,
    Failed,
}

impl ReloadOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

impl std::fmt::Display for ReloadOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One autocmd row projected for the inspector. Mirrors
/// every observable field of an [`crate::plugin::events::AutocmdEntry`]
/// so the in-app devtools and external tooling can answer questions
/// like "which plugin's autocmd disabled itself?" without needing
/// access to the host crate.
#[derive(Debug, Clone)]
pub struct AutocmdSummary {
    pub id: u64,
    pub plugin_id: String,
    pub generation_id: u64,
    pub group_id: Option<u64>,
    /// Canonical event name resolved from the descriptor table.
    pub event: String,
    /// Name the plugin subscribed under.
    pub subscribed_event: String,
    pub pattern: Option<String>,
    pub debounce_ms: u64,
    pub priority: i32,
    pub once: bool,
    pub source_location: Option<String>,
    pub fires: u64,
    pub failures: u32,
    pub consecutive_failures: u32,
    pub disabled: bool,
}

/// One row of the Reload history panel. Capped at the last
/// 64 entries per plugin by `PluginHost::record_reload_event`.
///
/// `stage_reached` names the highest staging step the attempt
/// successfully completed (e.g. `"init_lua"`, `"validation"`,
/// `"swap"`); a failed attempt's `stage_reached` is the stage that
/// failed to complete (i.e. the stage at which rollback fired). A
/// successful attempt always reports `"swap"`.
#[derive(Debug, Clone)]
pub struct ReloadEventSummary {
    pub plugin_id: String,
    pub from_generation_id: u64,
    pub to_generation_id: Option<u64>,
    pub outcome: ReloadOutcome,
    pub duration_ms: u128,
    pub stage_reached: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub timestamp_unix_ms: u128,
}

/// keymap projection. Mirrors the live
/// [`crate::plugin::keymap::KeymapSummary`] shape so external
/// inspectors don't need to depend on the host crate.
///
/// Rows are sorted by `(context, key, source, plugin_id)` so external
/// tooling can group as it pleases. Conflict losers are NOT removed —
/// they appear with `status = "conflict_lost"` plus a `conflict_with`
/// reference to the winning plugin/source.
#[derive(Debug, Clone)]
pub struct KeymapSummaryRow {
    pub context: String,
    pub key: String,
    pub command: String,
    pub plugin_id: String,
    pub generation_id: Option<u64>,
    /// One of `built-in`, `user`, `plugin`.
    pub source: String,
    /// One of `active`, `conflict_lost`.
    pub status: String,
    pub description: String,
    pub conflict_with: Option<KeymapConflictRef>,
}

#[derive(Debug, Clone)]
pub struct KeymapConflictRef {
    pub plugin_id: String,
    pub source: String,
}

/// command projection. Mirrors the live
/// [`crate::plugin::commands::CommandSummary`] shape minus the dispatcher
/// internals so external inspectors don't need to depend on the host
/// crate.
#[derive(Debug, Clone)]
pub struct CommandSummaryRow {
    pub name: String,
    pub title: String,
    pub description: String,
    pub plugin_id: String,
    pub generation_id: Option<u64>,
    pub context: String,
    pub destructive: bool,
    pub capabilities: Vec<String>,
    pub plugin_invocation_capabilities: Vec<String>,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub keymap_eligible: bool,
    pub palette_visible: bool,
    pub hook_before: bool,
    pub hook_after: bool,
    pub hook_veto: bool,
    pub replaceable: bool,
    pub fires: u64,
    pub failures: u64,
    pub last_outcome: Option<String>,
    pub last_duration_ms: u128,
}

#[derive(Debug, Clone)]
pub struct ExtensionPointSummary {
    pub id: String,
    pub kind: String,
    pub context_schema: String,
    pub contribution_schema: String,
    pub ordering_model: String,
    pub ownership_rules: String,
    pub capabilities: Vec<String>,
    pub render_mount_handler: String,
}

#[derive(Debug, Clone)]
pub struct ExtensionContributionSummary {
    pub plugin_id: String,
    pub point_id: String,
    pub kind: String,
    pub id: String,
    pub resource_kind: String,
    pub owner_key: String,
    pub dynamic: bool,
    pub source_location: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginTreeNode {
    pub id: String,
    pub label: String,
    pub version: String,
    pub children: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ContributionTreeNode {
    pub id: String,
    pub plugin_id: String,
    pub kind: String,
    pub location: String,
    pub visible: bool,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CommandTreeNode {
    pub id: String,
    pub plugin_id: String,
    pub title: String,
    pub enabled: bool,
    pub children: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AutocmdTreeNode {
    pub id: String,
    pub plugin_id: String,
    pub event: String,
    pub status: String,
    pub children: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DecorationInvalidationSummary {
    pub revision: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct InspectorSnapshot {
    pub plugins: Vec<PluginSummary>,
    pub slots: Vec<SlotSummary>,
    pub ui_render_diagnostics: Vec<UiRenderDiagnosticSummary>,
    pub services: Vec<ServiceSummary>,
    /// plugin services: declared consumer/provider service edges. Required
    /// missing edges block plugin load, so live snapshots mainly show
    /// connected required edges plus optional missing integrations.
    pub service_graph: Vec<ServiceGraphEdge>,
    /// plugin services: recent cross-plugin service method calls, capped in
    /// the registry. Includes success/failure and duration so devtools
    /// can trace invisible composition.
    pub service_call_traces: Vec<ServiceCallTraceSummary>,
    pub resources: Vec<ResourceSummary>,
    pub audit_recent: Vec<AuditEntry>,
    pub diagnostics: Vec<DiagnosticSummary>,
    /// plugin package layout: ordered runtime-path entries per plugin. Sorted by
    /// `(plugin_id, position-in-search-order)`.
    pub runtime_paths: Vec<RuntimePathSummary>,
    /// plugin package layout: cached modules per plugin/generation.
    pub loaded_modules: Vec<LoadedModuleSummary>,
    /// reload transactions: reload-history rows, oldest first across plugins.
    /// Entries within a plugin stay in chronological order. Capped at
    /// 64 per plugin in the underlying store.
    pub reload_history: Vec<ReloadEventSummary>,
    /// autocmd events: every live autocmd row across all plugins. Sorted by
    /// `(plugin_id, generation_id, autocmd_id)` so external tooling
    /// can group as it pleases.
    pub autocmds: Vec<AutocmdSummary>,
    /// command registry: every live command row across host and plugins.
    /// Sorted by `(plugin_id, name)`. Built-in host commands appear
    /// under the sentinel `<host>` plugin id.
    pub commands: Vec<CommandSummaryRow>,
    /// keymaps: every live keymap row, including conflict losers
    /// (each loser carries a `conflict_with` cross-reference to the
    /// winner). Sorted by `(context, key, source, plugin_id)`.
    pub keymaps: Vec<KeymapSummaryRow>,
    pub extension_points: Vec<ExtensionPointSummary>,
    pub extension_contributions: Vec<ExtensionContributionSummary>,
    pub plugin_tree: Vec<PluginTreeNode>,
    pub contribution_tree: Vec<ContributionTreeNode>,
    pub command_tree: Vec<CommandTreeNode>,
    pub autocmd_tree: Vec<AutocmdTreeNode>,
    pub contribution_overrides: Vec<ContributionOverrideSummary>,
    pub dock_panels: Vec<DockPanelSummary>,
    pub dock_layout: serde_json::Value,
    pub decoration_revision: u64,
    pub decoration_invalidations: Vec<DecorationInvalidationSummary>,
    /// Every recorded capability grant row, sorted by
    /// `(plugin_id, plugin_version, capability)`. Includes both
    /// allow/deny rows and `decided_by` lineage so a security-review
    /// pass can audit the full set without scraping the persisted
    /// JSON.
    pub capability_grants: Vec<CapabilityGrantSummary>,
    /// capability grants: open security prompts the host has surfaced but the
    /// user hasn't yet resolved. Sorted by `(plugin_id, plugin_version)`.
    /// devtools will render each row as a modal overlay; the grant store
    /// ships them headless so tests can drive the prompt flow
    /// without a renderer in the loop.
    pub pending_capability_prompts: Vec<PendingPromptSummary>,
    /// repository Git API: in-flight + recent plugin-triggered Git writes.
    /// Capped at [`crate::plugin::git_ops::PendingGitWrites::CAP`] (32)
    /// in chronological order. Each row records the plugin id, op
    /// name, args summary, start/finish unix-ms, and final status.
    pub pending_git_writes: Vec<PendingGitWrite>,
    /// async runtime: every active plugin async job. Sorted by
    /// `(plugin_id, generation_id, job_id)`.
    pub async_jobs: Vec<AsyncJobSummary>,
    /// async runtime: every active plugin timer.
    pub timers: Vec<TimerSummary>,
    /// async runtime: every active plugin file watcher.
    pub file_watchers: Vec<WatcherSummary>,
    /// persistence and settings: storage browser rows for plugin state, config,
    /// per-repo state, cache, settings, and secrets surfaces.
    pub storage: Vec<StorageSurfaceSummary>,
    /// persistence and settings: settings schema/value metadata. Values are omitted;
    /// devtools renders from schema and key lists.
    pub settings: Vec<SettingsSummary>,
    /// persistence and settings: secret metadata only. Secret values never appear in
    /// devtools snapshots.
    pub secrets: Vec<SecretSummary>,
    /// dependency graph: per-edge dependency rows. Sorted by
    /// `(consumer_plugin_id, dependency_id)`. Required and optional
    /// edges share the same row shape; `kind` and `status`
    /// distinguish them.
    pub dependency_graph: Vec<DependencySummaryRow>,
    /// lazy loading: per-plugin lazy-activation rows. Sorted by
    /// `plugin_id`. Lists every plugin that opted into lazy load
    /// regardless of whether it has activated yet — `status` is
    /// `"lazy"`, `"active"`, or `"poisoned"`.
    pub lazy_plugins: Vec<LazyPluginSummary>,
    /// extension points: every overlay registered via `leviathan.ui.overlay`,
    /// sorted by descending priority then `(plugin_id, id)`.
    pub overlays: Vec<OverlaySummary>,
    /// extension points: every context-menu item, sorted by
    /// `(region, priority, plugin_id, id)`.
    pub context_menu_items: Vec<ContextMenuItemSummary>,
    /// extension points: every commit-row decoration, sorted by
    /// `(commit_hash, plugin_id, id)`.
    pub graph_decorations: Vec<GraphDecorationSummary>,
    /// extension points: every diff decoration, sorted by `(plugin_id, id)`.
    pub diff_decorations: Vec<DiffDecorationSummary>,
    /// performance budgets: recent performance traces (one per `track_call`).
    /// Capped at 256 in the underlying tracker. Sorted by
    /// `(timestamp_unix_ms, plugin_id, callback_id)` ascending so
    /// devtools can render newest-last (oldest-first).
    pub performance_traces: Vec<PerformanceTraceSummary>,
    /// performance budgets: per-callback circuit-breaker rows, one per
    /// `(plugin_id, generation_id, callback_id)` that has at least
    /// one recorded sample. Sorted by
    /// `(plugin_id, generation_id, callback_id)`.
    pub circuit_breakers: Vec<CircuitBreakerSummary>,
    pub current_context: serde_json::Value,
}

/// performance-trace projection. Mirrors
/// [`crate::plugin::performance::PerformanceTrace`] for the inspector.
#[derive(Debug, Clone)]
pub struct PerformanceTraceSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub callback_id: String,
    pub kind: String,
    pub duration_ms: u128,
    pub ok: bool,
    pub timestamp_unix_ms: u128,
}

/// performance budgets circuit-breaker projection. Mirrors
/// [`crate::plugin::performance::CircuitBreakerSummary`] (the host
/// crate keeps one too, but devtools needs it without cross-crate
/// imports).
#[derive(Debug, Clone)]
pub struct CircuitBreakerSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub callback_id: String,
    /// Callback kind (`init`, `event`, `ui`, `command`, `service`,
    /// `timer`, `async_job`).
    pub kind: String,
    /// `"healthy"` | `"tripped"` | `"disabled"`.
    pub state: String,
    pub consecutive_failures: u32,
    pub count: u64,
    pub ok_count: u64,
    pub err_count: u64,
    pub p50_ms: u128,
    pub p95_ms: u128,
    pub last_failure: Option<String>,
}

/// extension points overlay projection. The `widget` payload is carried
/// through verbatim so devtools / acceptance tests can introspect
/// the plugin-supplied content without re-decoding the Lua AST.
#[derive(Debug, Clone)]
pub struct OverlaySummary {
    pub plugin_id: String,
    pub id: String,
    pub priority: i32,
    pub dismissible: bool,
    pub key_events: Vec<String>,
    pub widget: crate::plugin::ui::widget_ast::WidgetAst,
    pub source_location: Option<String>,
}

/// extension points context-menu projection.
#[derive(Debug, Clone)]
pub struct ContextMenuItemSummary {
    pub plugin_id: String,
    pub region: String,
    pub id: String,
    pub label: String,
    pub command: String,
    pub priority: i32,
    pub condition_capability: Option<String>,
    pub source_location: Option<String>,
}

/// extension points graph-decoration projection.
#[derive(Debug, Clone)]
pub struct GraphDecorationSummary {
    pub plugin_id: String,
    pub id: String,
    pub commit_hash: String,
    pub kind: String,
    pub decoration: serde_json::Value,
    pub source_location: Option<String>,
}

/// extension points diff-decoration projection.
#[derive(Debug, Clone)]
pub struct DiffDecorationSummary {
    pub plugin_id: String,
    pub id: String,
    pub kind: String,
    pub decoration: serde_json::Value,
    pub source_location: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DockPanelSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub id: String,
    pub key: String,
    pub title: String,
    pub area: String,
    pub open: bool,
    pub order: usize,
    pub selected: bool,
    pub state: serde_json::Value,
    pub source_location: Option<String>,
}

/// lazy-plugin projection. The host's `introspect()`
/// builds these directly from the lazy registry; external
/// inspectors don't need to depend on the host crate.
#[derive(Debug, Clone)]
pub struct LazyPluginSummary {
    pub plugin_id: String,
    /// Pretty-printed trigger descriptors (`command:foo`,
    /// `keymap:repository:gh`, `event:FetchStarted`, …) sorted for
    /// stable rendering.
    pub triggers: Vec<String>,
    /// `"lazy"` | `"active"` | `"poisoned"`.
    pub status: String,
    pub activations: u64,
    pub last_activation_unix_ms: Option<u128>,
    /// Pretty-printed descriptor of the most recent successful
    /// activation trigger (matches one of the `triggers` entries).
    /// `None` until the plugin has activated at least once.
    pub last_activation_trigger: Option<String>,
    pub last_error: Option<String>,
}

/// Dependency graph projection row. Mirrors
/// [`crate::plugin::dependency::DependencySummary`] for the inspector.
#[derive(Debug, Clone)]
pub struct DependencySummaryRow {
    pub consumer_plugin_id: String,
    pub dependency_id: String,
    pub requirement: String,
    pub resolved_version: Option<String>,
    /// `"required"` or `"optional"`.
    pub kind: String,
    /// `"resolved"`, `"missing"`, `"conflict"`.
    pub status: String,
}

/// async-job projection. Mirrors the live
/// [`crate::plugin::async_jobs::AsyncJobSummary`] shape.
#[derive(Debug, Clone)]
pub struct AsyncJobSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub job_id: u64,
    pub started_at_unix_ms: u128,
    pub status: String,
}

/// async runtime timer projection. Mirrors the live
/// [`crate::plugin::timers::TimerSummary`] shape.
#[derive(Debug, Clone)]
pub struct TimerSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub timer_id: u64,
    pub kind: String,
    pub interval_ms: u64,
    pub fires: u64,
}

/// async runtime watcher projection. Mirrors the live
/// [`crate::plugin::watchers::WatcherSummary`] shape.
#[derive(Debug, Clone)]
pub struct WatcherSummary {
    pub plugin_id: String,
    pub generation_id: u64,
    pub watch_id: u64,
    pub path: String,
    pub recursive: bool,
}

#[derive(Debug, Clone)]
pub struct StorageSurfaceSummary {
    pub plugin_id: String,
    pub surface: String,
    pub path: String,
    pub exists: bool,
    pub file_count: usize,
    pub byte_count: u64,
    pub corrupt_files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SettingsSummary {
    pub plugin_id: String,
    pub path: String,
    pub generated_form: Option<crate::plugin::ui::widget_ast::WidgetAst>,
    pub custom_view: Option<crate::plugin::ui::widget_ast::WidgetAst>,
    pub custom_view_source: Option<String>,
    pub schema_keys: Vec<String>,
    pub value_keys: Vec<String>,
    pub valid: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SecretSummary {
    pub plugin_id: String,
    pub path: String,
    pub key_count: usize,
    pub keys: Vec<String>,
}

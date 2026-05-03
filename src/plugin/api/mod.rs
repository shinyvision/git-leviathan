//! Lua-facing API surface exposed under the `leviathan.*` global.
//!
//! One submodule per namespace (SOLID single-responsibility):
//! - `ui` — descriptor-backed regions, screen registration
//! - `fs` — filesystem operations
//! - `env` — process environment variables
//! - `event` — `leviathan.autocmd.*` event subscription
//!
//! `install_all` mounts them on a fresh `leviathan` table. Callable state
//! that must survive `init.lua` exec (button/screen handlers, autocmd
//! callbacks) is captured in a shared `BuildState`; the host drains it
//! after exec.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use git_leviathan_plugin_api::descriptor::api as api_descriptor;
use mlua::{Lua, LuaSerdeExt, RegistryKey};

use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::diagnostic::DiagnosticStore;
use crate::plugin::git_ops::GitOpsContext;
use crate::plugin::resources::{GenerationId, PluginId, ResourceLedger};

#[path = "async.rs"]
pub mod async_api;
pub mod async_runtime;
pub mod command;
pub mod env;
pub mod event;
pub mod factory;
pub mod fs;
pub mod git;
pub mod health;
pub mod keymap;
pub mod persist;
pub mod repository;
pub mod secrets;
pub mod services_api;
pub mod settings;
pub mod tab_registry;
pub mod timer;
pub mod ui;
pub mod ui_ext;

pub use async_runtime::{DeferredCallback, DeferredQueue};
pub use command::UserCommands;
pub use health::HealthCheckRegistration;
pub use persist::PersistContext;
pub use services_api::ServicesContext;

/// async runtime cross-plugin async-runtime handles. Cheap-clone (Arc-backed).
/// Bundles the registries plus per-plugin callback tables so the
/// install path doesn't grow yet another argument.
pub struct AsyncRuntimeContext {
    pub jobs: crate::plugin::async_jobs::AsyncJobRegistry,
    pub timers: crate::plugin::timers::TimerRegistry,
    pub watchers: crate::plugin::watchers::FileWatcherRegistry,
    pub job_callbacks: Rc<RefCell<async_api::JobCallbacks>>,
    pub timer_callbacks: Rc<RefCell<crate::plugin::timers::PluginTimerCallbacks>>,
    pub watcher_callbacks: Rc<RefCell<crate::plugin::watchers::PluginWatcherCallbacks>>,
}

pub struct ScreenDef {
    pub init: RegistryKey,
    pub view: RegistryKey,
    pub update: RegistryKey,
    pub serialize: Option<RegistryKey>,
    pub deserialize: Option<RegistryKey>,
}

/// Raw slot spec (region-scoped). Carries everything the host needs to
/// place the slot in the right registry: a region name, a container key
/// (chrome section, or "pane.section" for content regions), id,
/// priority, widget, on_click.
///
/// The `widget` is either a plugin-supplied widget tree (same DSL as
/// plugin screens) serialised to `serde_json::Value`, or a Lua function
/// the host re-invokes to produce one. See [`WidgetSource`]. The slot is
/// fully widget-type-agnostic: the host doesn't know whether a slot is an
/// `icon+text` button, a bordered pill, a plain label, or a layout of
/// nested containers — that's all the plugin's decision.
///
/// `on_click` is stashed as a registry key so the slot's handler can be
/// invoked later (fired from clickable widgets inside `widget` that
/// dispatch through the main-bar-slot scope).
pub struct RawSlotSpec {
    pub id: String,
    pub region: String,
    pub container: String,
    pub priority: i32,
    pub widget: WidgetSource,
    pub on_click: Option<RegistryKey>,
    pub source_location: Option<String>,
}

/// Where a slot's widget tree comes from.
///
/// - `Static` — the plugin declared a literal table once at init. The
///   host keeps the decoded `WidgetAst` and renders from it each
///   frame.
/// - `Dynamic` — the plugin declared a function. The host re-invokes it
///   whenever plugin-observable state might have changed (autocmd
///   callbacks) and re-decodes the resulting Lua tree to an AST cached
///   for the slot.
pub enum WidgetSource {
    Static(crate::plugin::ui::widget_ast::WidgetAst),
    Dynamic(RegistryKey),
}

/// One hook operation as issued by a plugin's init.lua. Kept in source
/// order so that within a single plugin, a remove issued after an add
/// targets the already-added slot (not the other way around).
pub enum RawSlotOp {
    Add(RawSlotSpec),
    Remove {
        region: String,
        container: String,
        id: String,
    },
    Replace {
        region: String,
        container: String,
        id: String,
        spec: RawSlotSpec,
    },
}

/// One autocmd subscription captured during init by
/// `leviathan.autocmd.create`. One per (event, callback) pair — a
/// single create call with an array of events emits one `RawAutocmd`
/// per event so dispatch stays a plain index lookup.
///
/// `local_group` is a plugin-local group id minted by
/// `leviathan.autocmd.group`; the host resolves it to a stable
/// [`crate::plugin::events::GroupId`] at install time and writes the
/// resolved value back onto the autocmd's options.
///
/// `sequence` is the plugin-local declaration sequence number,
/// shared with [`RawAutocmdGroup`] and [`RawAutocmdClear`]. The host
/// install path replays operations in `sequence` order so a `clear`
/// issued *after* an `autocmd.create` correctly removes that
/// registration (and not the unrelated one that comes after the
/// `clear` in declaration order).
pub struct RawAutocmd {
    pub event: String,
    pub callback: RegistryKey,
    pub local_group: Option<u64>,
    pub once: bool,
    pub pattern: Option<String>,
    pub debounce_ms: u64,
    pub priority: i32,
    pub sequence: u64,
    pub source_location: Option<String>,
}

/// Per-plugin record of every `leviathan.autocmd.group` call made
/// during init. The host resolves these into stable
/// [`crate::plugin::events::GroupId`] handles when staging commits or
/// when the plugin first loads. Captured during init because group
/// resolution needs the host's group table (which Lua can't reach
/// directly).
pub struct RawAutocmdGroup {
    /// Local handle the plugin saw — a small integer issued by
    /// `event.rs`'s `next_local_group_id` counter at registration
    /// time. Autocmds reference groups through this same local id.
    pub local_id: u64,
    pub name: String,
    pub clear: bool,
    /// Plugin-local declaration sequence number; see [`RawAutocmd`].
    pub sequence: u64,
    pub source_location: Option<String>,
}

/// One `leviathan.autocmd.clear` call. The host applies the clear
/// after staging completes, against the resolved
/// [`crate::plugin::events::GroupId`].
pub struct RawAutocmdClear {
    pub local_id: u64,
    /// Plugin-local declaration sequence number; see [`RawAutocmd`].
    pub sequence: u64,
    pub source_location: Option<String>,
}

#[derive(Default)]
pub struct BuildState {
    pub screens: HashMap<String, ScreenDef>,
    /// Ordered hook operations from `leviathan.ui.regions.*`.
    pub slot_ops: Vec<RawSlotOp>,
    /// Autocmd subscriptions from `leviathan.autocmd.create`. Each
    /// entry's `options.group` holds the plugin-local group id
    /// (resolved to a host-wide
    /// [`crate::plugin::events::GroupId`] at install time).
    pub autocmds: Vec<RawAutocmd>,
    /// `leviathan.autocmd.group(name, opts)` calls in declaration
    /// order.
    pub autocmd_groups: Vec<RawAutocmdGroup>,
    /// `leviathan.autocmd.clear(group)` calls in declaration order.
    pub autocmd_clears: Vec<RawAutocmdClear>,
    /// Counter for plugin-local group ids minted by
    /// `leviathan.autocmd.group`. Starts at 1 because `0` is the
    /// "no group" sentinel for the autocmd `group` option.
    pub next_local_group_id: u64,
    /// Monotonic declaration sequence shared by every autocmd /
    /// group / clear op so the host can replay them in source
    /// order at install time.
    pub next_autocmd_sequence: u64,
    /// typed command registrations from
    /// `leviathan.command.create`. The host drains this list after
    /// init.lua and installs each entry into the unified
    /// `CommandRegistry`.
    pub commands: Vec<crate::plugin::commands::RawCommand>,
    /// keymap `set` rows captured from `leviathan.keymap.set`.
    /// The host drains and installs them into the unified
    /// `KeymapRegistry` keyed by `(plugin_id, generation_id)`.
    pub keymaps: Vec<crate::plugin::keymap::RawKeymap>,
    /// keymap `del` rows captured from `leviathan.keymap.del`.
    /// Applied after the staged generation's `set` rows so a plugin
    /// can rebind its own keymap inside one init.lua run.
    pub keymap_dels: Vec<crate::plugin::keymap::RawKeymapDel>,
    /// Monotonic declaration sequence shared by every keymap `set` /
    /// `del` op so the host can replay them in source order at install
    /// time and so the resolver's per-plugin tie-break is stable.
    pub next_keymap_sequence: u64,
}

#[allow(clippy::too_many_arguments)]
pub fn install_all(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    pending_tab_ops: tab_registry::PendingOps,
    guard: Rc<CapabilityGuard>,
    services_ctx: ServicesContext,
    persist_ctx: PersistContext,
    deferred: Rc<RefCell<DeferredQueue>>,
    user_commands: Rc<RefCell<UserCommands>>,
    health_checks: Rc<RefCell<Vec<HealthCheckRegistration>>>,
    ledger: ResourceLedger,
    command_dispatch: crate::plugin::commands::CommandDispatchEnv,
    keymaps: keymap::SharedKeymapRegistry,
    git_ctx: GitOpsContext,
    pending_git_events: git::PendingGitEvents,
    async_ctx: AsyncRuntimeContext,
    plugin_id: PluginId,
    generation_id: GenerationId,
    _diagnostics: DiagnosticStore,
    extension_registry: crate::plugin::extensions::ExtensionRegistry,
) -> mlua::Result<()> {
    let leviathan = lua.create_table()?;

    let api_tbl = lua.create_table()?;
    api_tbl.set(
        "describe",
        lua.create_function(|lua_inner, ()| {
            let value =
                serde_json::to_value(api_descriptor::describe()).map_err(mlua::Error::external)?;
            lua_inner.to_value(&value)
        })?,
    )?;
    event::install(lua, Rc::clone(&build), ledger.clone(), &leviathan)?;
    async_runtime::install(lua, Rc::clone(&deferred), ledger.clone(), &api_tbl)?;
    command::install(
        lua,
        Rc::clone(&user_commands),
        Rc::clone(&build),
        ledger.clone(),
        &leviathan,
        command_dispatch,
    )?;
    leviathan.set("api", api_tbl)?;

    leviathan.set(
        "has",
        lua.create_function(|_, feature: String| Ok(api_descriptor::has_feature(&feature)))?,
    )?;

    ui::install(lua, Rc::clone(&build), ledger.clone(), &leviathan)?;
    {
        let ui_table: mlua::Table = leviathan.get("ui")?;
        ui_ext::install(
            lua,
            &ui_table,
            ledger.clone(),
            Rc::clone(&guard),
            extension_registry,
        )?;
    }
    keymap::install(lua, Rc::clone(&build), ledger.clone(), &leviathan, keymaps)?;
    fs::install(lua, &leviathan, Rc::clone(&guard))?;
    fs::install_watch(
        lua,
        &leviathan,
        Rc::clone(&guard),
        ledger.clone(),
        async_ctx.watchers.clone(),
        Rc::clone(&async_ctx.watcher_callbacks),
        plugin_id.clone(),
        generation_id,
    )?;
    async_api::install(
        lua,
        &leviathan,
        Rc::clone(&guard),
        ledger.clone(),
        async_ctx.jobs.clone(),
        Rc::clone(&async_ctx.job_callbacks),
        Rc::clone(&deferred),
        plugin_id.clone(),
        generation_id,
    )?;
    timer::install(
        lua,
        &leviathan,
        Rc::clone(&guard),
        ledger.clone(),
        async_ctx.timers.clone(),
        Rc::clone(&async_ctx.timer_callbacks),
        plugin_id.clone(),
        generation_id,
    )?;
    env::install(lua, &leviathan, Rc::clone(&guard))?;
    tab_registry::install(lua, &leviathan, pending_tab_ops)?;
    services_api::install(lua, services_ctx, ledger.clone(), &leviathan)?;
    persist::install(lua, persist_ctx.clone(), &leviathan)?;
    settings::install(lua, persist_ctx.clone(), &leviathan)?;
    secrets::install(lua, persist_ctx, Rc::clone(&guard), &leviathan)?;
    health::install(lua, health_checks, ledger, &leviathan)?;

    // Start with an empty repository snapshot so plugin code that touches
    // `leviathan.repository` at `init.lua` time (before the first sync)
    // never trips on `nil`. The host overwrites this on every sync.
    leviathan.set(
        "repository",
        repository::build_table(lua, "", "", "", "", "", &[])?,
    )?;

    // typed read APIs install onto the existing repository
    // table. Capability checks happen at call time inside each closure.
    repository::install_read_functions(
        lua,
        &leviathan,
        git_ctx.clone(),
        Rc::clone(&guard),
        plugin_id.clone(),
        generation_id,
    )?;

    // `leviathan.git.*` write namespace.
    git::install(
        lua,
        &leviathan,
        git_ctx,
        pending_git_events,
        Rc::clone(&guard),
        plugin_id,
        generation_id,
    )?;

    // `leviathan.log` is plugin-callable; the host treats it as a
    // request to surface a developer-facing message and forwards it
    // verbatim to stderr. Plugins MUST NOT use this to fabricate host
    // diagnostics — diagnostic emission stays host-owned.
    leviathan.set(
        "log",
        lua.create_function(|_, msg: String| {
            eprintln!("git_leviathan plugin: {msg}");
            Ok(())
        })?,
    )?;

    lua.globals().set("leviathan", leviathan)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use git_leviathan_plugin_api::capability::Capability;
    use mlua::{Table, Value as LuaValue};

    use crate::plugin::commands::{
        CommandDispatchEnv, CommandPluginContext, CommandPluginRegistry, CommandRegistry,
        PendingCommandEvents,
    };
    use crate::plugin::diagnostic::{DiagnosticStore, NullSink};
    use crate::plugin::keymap::KeymapRegistry;
    use crate::plugin::lua_loader::{install_runtime_module, LuaLoader};
    use crate::plugin::resources::{GenerationId, PluginId};
    use crate::plugin::runtime_path::{PluginRuntimePath, RuntimePathRegistry};
    use crate::plugin::services::ServiceRegistry;

    fn install_test_api() -> (Rc<Lua>, tempfile::TempDir) {
        let lua = Rc::new(Lua::new());
        let tmp = tempfile::tempdir().unwrap();
        let build = Rc::new(RefCell::new(BuildState::default()));
        let deferred = Rc::new(RefCell::new(DeferredQueue::default()));
        let commands = Rc::new(RefCell::new(UserCommands::default()));
        let health = Rc::new(RefCell::new(Vec::new()));
        let pending_tab_ops = tab_registry::PendingOps::default();
        let grant_store = crate::plugin::capability_grants::GrantStore::new_in_memory();
        let guard = Rc::new(CapabilityGuard::new(
            "coverage",
            "0.0.0",
            Vec::<Capability>::new(),
            tmp.path().to_path_buf(),
            tmp.path().join("state"),
            tmp.path().join("config"),
            None,
            grant_store,
        ));
        let services_ctx = ServicesContext {
            registry: Rc::new(RefCell::new(ServiceRegistry::new())),
            lookup_registry: None,
            plugin_id: "coverage".to_string(),
            generation_id: GenerationId::new(1),
            provides: Vec::new(),
            consumes: Vec::new(),
            plugin_lua: Rc::clone(&lua),
            capability_guard: Rc::clone(&guard),
        };
        let storage =
            crate::plugin::storage::PluginStorageRoots::under_base(tmp.path().join("persist"))
                .for_plugin("coverage", tmp.path());
        let persist_ctx = PersistContext { storage };
        let ledger = ResourceLedger::new("coverage".into(), GenerationId::new(1));
        let plugin_registry = CommandPluginRegistry::new();
        plugin_registry.insert(
            "coverage",
            CommandPluginContext {
                lua: Rc::clone(&lua),
                capability_guard: Rc::clone(&guard),
                generation_id: GenerationId::new(1),
            },
        );
        let diag_store = DiagnosticStore::with_sink(std::sync::Arc::new(NullSink));
        let dispatch = CommandDispatchEnv {
            commands: Rc::new(RefCell::new(CommandRegistry::new())),
            plugin_registry,
            diagnostics: diag_store.clone(),
            pending_events: PendingCommandEvents::new(),
            budget_tracker: crate::plugin::performance::BudgetTracker::new(diag_store),
        };
        let keymaps = Rc::new(RefCell::new(KeymapRegistry::new()));
        let git_ctx = crate::plugin::git_ops::GitOpsContext {
            gateway: crate::plugin::git_ops::ActiveRepositoryGateway::new(),
            destructive: crate::plugin::git_ops::DestructiveConfirmPolicy::new(),
            pending: crate::plugin::git_ops::PendingGitWrites::new(),
            audit: crate::plugin::audit::AuditLog::new(),
            diagnostics: DiagnosticStore::with_sink(std::sync::Arc::new(NullSink)),
        };
        let pending_git_events = git::PendingGitEvents::new();
        let async_ctx = AsyncRuntimeContext {
            jobs: crate::plugin::async_jobs::AsyncJobRegistry::new(),
            timers: crate::plugin::timers::TimerRegistry::new(),
            watchers: crate::plugin::watchers::FileWatcherRegistry::new(),
            job_callbacks: Rc::new(RefCell::new(async_api::JobCallbacks::new())),
            timer_callbacks: Rc::new(RefCell::new(
                crate::plugin::timers::PluginTimerCallbacks::new(),
            )),
            watcher_callbacks: Rc::new(RefCell::new(
                crate::plugin::watchers::PluginWatcherCallbacks::new(),
            )),
        };
        install_all(
            lua.as_ref(),
            build,
            pending_tab_ops,
            guard,
            services_ctx,
            persist_ctx,
            deferred,
            commands,
            health,
            ledger,
            dispatch,
            keymaps,
            git_ctx,
            pending_git_events,
            async_ctx,
            PluginId::from("coverage"),
            GenerationId::new(1),
            DiagnosticStore::with_sink(std::sync::Arc::new(NullSink)),
            crate::plugin::extensions::ExtensionRegistry::new(),
        )
        .unwrap();
        // The plugin package layout `leviathan.runtime` module is installed by the
        // host alongside `leviathan.*`. Mirror that here so the
        // descriptor coverage test sees every host function the real
        // load path exposes.
        let registry = RuntimePathRegistry::new();
        let runtime_path = PluginRuntimePath::resolve("coverage", tmp.path(), &[], &registry);
        let loader = LuaLoader::new(
            "coverage".into(),
            GenerationId::new(1),
            runtime_path,
            DiagnosticStore::with_sink(std::sync::Arc::new(NullSink)),
            false,
        );
        let leviathan: Table = lua.globals().get("leviathan").unwrap();
        install_runtime_module(lua.as_ref(), &leviathan, loader).unwrap();
        (lua, tmp)
    }

    fn collect_functions(table: Table, prefix: &str, out: &mut BTreeSet<String>) {
        for pair in table.pairs::<String, LuaValue>() {
            let (key, value) = pair.unwrap();
            let path = format!("{prefix}.{key}");
            match value {
                LuaValue::Function(_) => {
                    out.insert(path);
                }
                LuaValue::Table(child) => collect_functions(child, &path, out),
                _ => {}
            }
        }
    }

    #[test]
    fn installed_host_functions_match_descriptors() {
        let (lua, _tmp) = install_test_api();
        let leviathan: Table = lua.globals().get("leviathan").unwrap();
        let mut actual = BTreeSet::new();
        collect_functions(leviathan, "leviathan", &mut actual);

        let expected: BTreeSet<String> = api_descriptor::function_paths()
            .into_iter()
            .map(str::to_string)
            .collect();

        assert_eq!(actual, expected);
    }

    #[test]
    fn runtime_has_and_describe_use_descriptors() {
        let (lua, _tmp) = install_test_api();
        let has_read_file: bool = lua
            .load(r#"return leviathan.has("fs.read_file@1")"#)
            .eval()
            .unwrap();
        let has_regions_v1: bool = lua
            .load(r#"return leviathan.has("ui.regions.add_slot@1")"#)
            .eval()
            .unwrap();
        let has_unknown: bool = lua
            .load(r#"return leviathan.has("fs.read_file@3")"#)
            .eval()
            .unwrap();
        let module_count: usize = lua
            .load(r#"return #leviathan.api.describe().modules"#)
            .eval()
            .unwrap();

        assert!(has_read_file);
        assert!(has_regions_v1);
        assert!(!has_unknown);
        assert_eq!(module_count, api_descriptor::API_MODULES.len());
    }
}

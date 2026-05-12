//! Lua API installers and init-time registration state.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use git_leviathan_plugin_api::descriptor::api as api_descriptor;
use mlua::{Lua, LuaSerdeExt, RegistryKey};

use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::diagnostic::DiagnosticStore;
use crate::plugin::extensions::{DecorationProviderCallbacks, OverlayCallbacks};
use crate::plugin::git_ops::GitOpsContext;
use crate::plugin::resources::{GenerationId, PluginId, ResourceLedger};

pub mod assets;
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
pub mod shell;
pub mod tab_registry;
pub mod timer;
pub mod ui;
pub mod ui_core;
pub mod ui_dock;
pub mod ui_ext;

pub use async_runtime::{DeferredCallback, DeferredQueue};
pub use command::UserCommands;
pub use health::HealthCheckRegistration;
pub use persist::PersistContext;
pub use services_api::ServicesContext;

pub struct AsyncRuntimeContext {
    pub jobs: crate::plugin::async_jobs::AsyncJobRegistry,
    pub timers: crate::plugin::timers::TimerRegistry,
    pub watchers: crate::plugin::watchers::FileWatcherRegistry,
    pub job_callbacks: Rc<RefCell<async_api::JobCallbacks>>,
    pub timer_callbacks: Rc<RefCell<crate::plugin::timers::PluginTimerCallbacks>>,
    pub watcher_callbacks: Rc<RefCell<crate::plugin::watchers::PluginWatcherCallbacks>>,
}

pub struct ScreenDef {
    pub id: String,
    pub title: String,
    pub breadcrumbs: Vec<String>,
    pub bind_repository: bool,
    pub init: RegistryKey,
    pub view: RegistryKey,
    pub update: RegistryKey,
    pub serialize: Option<RegistryKey>,
    pub deserialize: Option<RegistryKey>,
    pub can_close: Option<RegistryKey>,
}

pub struct DockPanelDef {
    pub id: String,
    pub title: String,
    pub area: crate::plugin::dock::DockArea,
    pub default_open: bool,
    pub view: RegistryKey,
    pub update: Option<RegistryKey>,
    pub source_location: Option<String>,
}

pub struct SettingsPanelDef {
    pub view: RegistryKey,
    pub source_location: Option<String>,
}

pub struct RawSlotSpec {
    pub id: String,
    pub region: String,
    pub container: String,
    pub priority: i32,
    pub widget: WidgetSource,
    pub depends_on: Vec<crate::plugin::ui::invalidation::UiDependency>,
    pub on_click: Option<RegistryKey>,
    pub source_location: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicWidgetCall {
    Context,
}

pub enum WidgetSource {
    Static(Box<crate::plugin::ui::widget_ast::WidgetAst>),
    Dynamic {
        key: RegistryKey,
        call: DynamicWidgetCall,
    },
}

/// Kept in source order so remove/replace can target earlier adds.
pub enum RawSlotOp {
    Add(RawSlotSpec),
    Remove {
        target_plugin_id: Option<String>,
        region: String,
        container: String,
        id: String,
        source_location: Option<String>,
    },
    Replace {
        target_plugin_id: Option<String>,
        region: String,
        container: String,
        id: String,
        spec: RawSlotSpec,
        source_location: Option<String>,
    },
}

/// Captured in source order so `autocmd.clear` applies to earlier rows only.
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

pub struct RawAutocmdGroup {
    pub local_id: u64,
    pub name: String,
    pub clear: bool,
    pub sequence: u64,
    pub source_location: Option<String>,
}

pub struct RawAutocmdClear {
    pub local_id: u64,
    pub sequence: u64,
    pub source_location: Option<String>,
}

#[derive(Default)]
pub struct BuildState {
    pub screens: HashMap<String, ScreenDef>,
    pub slot_ops: Vec<RawSlotOp>,
    pub autocmds: Vec<RawAutocmd>,
    pub autocmd_groups: Vec<RawAutocmdGroup>,
    pub autocmd_clears: Vec<RawAutocmdClear>,
    pub dock_panels: HashMap<String, DockPanelDef>,
    pub settings_panel: Option<SettingsPanelDef>,
    pub next_local_group_id: u64,
    pub next_autocmd_sequence: u64,
    pub commands: Vec<crate::plugin::commands::RawCommand>,
    pub keymaps: Vec<crate::plugin::keymap::RawKeymap>,
    pub keymap_dels: Vec<crate::plugin::keymap::RawKeymapDel>,
    /// Monotonic declaration sequence shared by every keymap `set` /
    /// `del` op so the host can replay them in source order at install
    /// time and so the resolver's per-plugin tie-break is stable.
    pub next_keymap_sequence: u64,
}

pub struct InstallAllContext {
    pub build: Rc<RefCell<BuildState>>,
    pub pending_tab_ops: tab_registry::PendingOps,
    pub guard: Rc<CapabilityGuard>,
    pub services_ctx: ServicesContext,
    pub persist_ctx: PersistContext,
    pub deferred: Rc<RefCell<DeferredQueue>>,
    pub user_commands: Rc<RefCell<UserCommands>>,
    pub health_checks: Rc<RefCell<Vec<HealthCheckRegistration>>>,
    pub ledger: ResourceLedger,
    pub command_dispatch: crate::plugin::commands::CommandDispatchEnv,
    pub keymaps: keymap::SharedKeymapRegistry,
    pub git_ctx: GitOpsContext,
    pub pending_git_events: git::PendingGitEvents,
    pub pending_ui_effects: crate::plugin::ui::effects::PendingUiEffects,
    pub async_ctx: AsyncRuntimeContext,
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
    pub diagnostics: DiagnosticStore,
    pub extension_registry: crate::plugin::extensions::ExtensionRegistry,
    pub overlay_callbacks: Rc<RefCell<OverlayCallbacks>>,
    pub decoration_provider_callbacks: Rc<RefCell<DecorationProviderCallbacks>>,
    pub ui_context: crate::plugin::ui::context::UiContextStore,
    pub chrome_widgets: ui_ext::ChromeWidgetMap,
}

pub fn install_all(lua: &Lua, ctx: InstallAllContext) -> mlua::Result<()> {
    let InstallAllContext {
        build,
        pending_tab_ops,
        guard,
        services_ctx,
        persist_ctx,
        deferred,
        user_commands,
        health_checks,
        ledger,
        command_dispatch,
        keymaps,
        git_ctx,
        pending_git_events,
        pending_ui_effects,
        async_ctx,
        plugin_id,
        generation_id,
        diagnostics,
        extension_registry,
        overlay_callbacks,
        decoration_provider_callbacks,
        ui_context,
        chrome_widgets,
    } = ctx;

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
        &leviathan,
        command::InstallCtx {
            commands: Rc::clone(&user_commands),
            build: Rc::clone(&build),
            ledger: ledger.clone(),
            dispatch: command_dispatch,
            guard: Rc::clone(&guard),
            plugin_id: plugin_id.clone(),
        },
    )?;
    leviathan.set("api", api_tbl)?;

    leviathan.set(
        "has",
        lua.create_function(|_, feature: String| Ok(api_descriptor::has_feature(&feature)))?,
    )?;

    ui::install(
        lua,
        Rc::clone(&build),
        ledger.clone(),
        Rc::clone(&guard),
        diagnostics.clone(),
        ui_context,
        &leviathan,
    )?;
    assets::install(lua, &leviathan, ledger.clone())?;
    {
        let ui_table: mlua::Table = leviathan.get("ui")?;
        ui_dock::install(
            lua,
            Rc::clone(&build),
            ledger.clone(),
            Rc::clone(&guard),
            &ui_table,
        )?;
        ui_ext::install(
            lua,
            &ui_table,
            ui_ext::UiExtInstall {
                ledger: ledger.clone(),
                guard: Rc::clone(&guard),
                registry: extension_registry,
                ui_effects: pending_ui_effects,
                overlay_callbacks: Rc::clone(&overlay_callbacks),
                decoration_provider_callbacks: Rc::clone(&decoration_provider_callbacks),
                chrome_widgets,
            },
        )?;
    }
    keymap::install(lua, Rc::clone(&build), ledger.clone(), &leviathan, keymaps)?;
    fs::install(lua, &leviathan, Rc::clone(&guard))?;
    fs::install_watch(
        lua,
        &leviathan,
        fs::WatchInstallContext {
            guard: Rc::clone(&guard),
            ledger: ledger.clone(),
            registry: async_ctx.watchers.clone(),
            callbacks: Rc::clone(&async_ctx.watcher_callbacks),
            plugin_id: plugin_id.clone(),
            generation_id,
        },
    )?;
    async_api::install(
        lua,
        &leviathan,
        async_api::AsyncInstallContext {
            guard: Rc::clone(&guard),
            ledger: ledger.clone(),
            registry: async_ctx.jobs.clone(),
            job_callbacks: Rc::clone(&async_ctx.job_callbacks),
            deferred: Rc::clone(&deferred),
            plugin_id: plugin_id.clone(),
            generation_id,
        },
    )?;
    timer::install(
        lua,
        &leviathan,
        timer::TimerInstallContext {
            guard: Rc::clone(&guard),
            ledger: ledger.clone(),
            registry: async_ctx.timers.clone(),
            callbacks: Rc::clone(&async_ctx.timer_callbacks),
            plugin_id: plugin_id.clone(),
            generation_id,
        },
    )?;
    env::install(lua, &leviathan, Rc::clone(&guard))?;
    shell::install(
        lua,
        &leviathan,
        shell::ShellInstallContext {
            guard: Rc::clone(&guard),
            ledger: ledger.clone(),
            registry: async_ctx.jobs.clone(),
            job_callbacks: Rc::clone(&async_ctx.job_callbacks),
            plugin_id: plugin_id.clone(),
            generation_id,
        },
    )?;
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
            crate::plugin::capabilities::CapabilityPaths {
                plugin_root: tmp.path().to_path_buf(),
                state_dir: tmp.path().join("state"),
                config_dir: tmp.path().join("config"),
                workdir: None,
            },
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
        let ui_context =
            crate::plugin::ui::context::UiContextStore::new("coverage", GenerationId::new(1));
        plugin_registry.insert(
            "coverage",
            CommandPluginContext {
                lua: Rc::clone(&lua),
                capability_guard: Rc::clone(&guard),
                ui_context: ui_context.clone(),
                generation_id: GenerationId::new(1),
            },
        );
        let diag_store = DiagnosticStore::with_sink(std::sync::Arc::new(NullSink));
        let dispatch = CommandDispatchEnv {
            commands: Rc::new(RefCell::new(CommandRegistry::new())),
            plugin_registry,
            diagnostics: diag_store.clone(),
            pending_events: PendingCommandEvents::new(),
            active_context: Rc::new(RefCell::new(serde_json::json!({
                "surface": "screen",
                "repository": { "is_open": false },
                "tab": { "count": 0 },
            }))),
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
            InstallAllContext {
                build,
                pending_tab_ops,
                guard,
                services_ctx,
                persist_ctx,
                deferred,
                user_commands: commands,
                health_checks: health,
                ledger,
                command_dispatch: dispatch,
                keymaps,
                git_ctx,
                pending_git_events,
                pending_ui_effects: crate::plugin::ui::effects::PendingUiEffects::new(),
                async_ctx,
                plugin_id: PluginId::from("coverage"),
                generation_id: GenerationId::new(1),
                diagnostics: DiagnosticStore::with_sink(std::sync::Arc::new(NullSink)),
                extension_registry: crate::plugin::extensions::ExtensionRegistry::new(),
                overlay_callbacks: Rc::new(RefCell::new(OverlayCallbacks::new())),
                decoration_provider_callbacks: Rc::new(RefCell::new(
                    DecorationProviderCallbacks::new(),
                )),
                ui_context,
                chrome_widgets: Rc::new(RefCell::new(std::collections::HashMap::new())),
            },
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
        let has_ui_slot: bool = lua
            .load(r#"return leviathan.has("ui.slot@1")"#)
            .eval()
            .unwrap();
        let has_ui_context: bool = lua
            .load(r#"return leviathan.has("ui.context@1")"#)
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
        assert!(has_ui_slot);
        assert!(has_ui_context);
        assert!(!has_unknown);
        assert_eq!(module_count, api_descriptor::API_MODULES.len());
    }
}

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
//! - Widget trees are cached in `widget_tree: Option<serde_json::Value>` and
//!   refreshed after every dispatch (open_screen / dispatch_event).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use git_leviathan_plugin_api::api_version::HOST_API_VERSION;
use git_leviathan_plugin_api::manifest::PluginManifest;
use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, Table, Thread, ThreadStatus, Value as LuaValue};

use crate::plugin::api::{
    self, BuildState, DeferredQueue, RawSlotOp, RawSlotSpec, ScreenDef, ServicesContext,
    UserCommands, WidgetSource,
};
use crate::plugin::audit::AuditLog;
use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::services::ServiceRegistry;
use crate::plugin::slots::{IsSlot, SlotRegistry};
use crate::plugin::tab_snapshot::{TabChange, TabRegistryOp, TabsSnapshot};
use crate::plugin::ui::main_bar_slots::{parse_container, PreparedSlot, PreparedSlotOp, SlotWidget};
use crate::plugin::ui::split;
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
        }
    }
}

impl std::error::Error for PluginLoadError {}

struct LoadedPlugin {
    #[allow(dead_code)]
    id: String,
    /// Absolute path to the plugin's directory. Used as sandbox root when
    /// resolving plugin-bundled assets (icons, etc).
    root: PathBuf,
    lua: Rc<Lua>,
    /// `main_bar.add` / `main_bar.replace` handlers. Keyed by `slot_id`
    /// (the full registry id the plugin declared).
    slot_handlers: HashMap<String, RegistryKey>,
    screens: HashMap<String, ScreenDef>,
    screen_state: HashMap<String, RegistryKey>,
    /// Dynamic (function-backed) main-bar slot widgets. Each entry is the
    /// Lua function registry key plus the shared cache cell the slot's
    /// builder reads from. Populated on load; refreshed after every
    /// autocmd fire affecting this plugin.
    dynamic_widgets: HashMap<String, (RegistryKey, Rc<RefCell<serde_json::Value>>)>,
    /// Per-plugin deferred-call queue. `leviathan.api.schedule(fn)` and
    /// `defer_fn(ms, fn)` push into this; `PluginHost::tick` drains it.
    /// Coroutines that yielded mid-resume also live here so subsequent
    /// ticks can resume them.
    deferred: Rc<RefCell<DeferredQueue>>,
    /// Plugin-registered named commands (`leviathan.api.create_user_command`).
    /// Looked up by `PluginHost::invoke_user_command`.
    user_commands: Rc<RefCell<UserCommands>>,
}

struct SplitDragInfo {
    split_key: String,
    divider_index: usize,
    initial_sizes: Vec<f32>,
    initial_pointer: Option<f32>,
    is_vertical: bool,
    limits: Vec<(f32, f32)>,
}

pub struct PluginHost {
    plugins: HashMap<String, LoadedPlugin>,
    /// Ordered hook operations across every region.
    slot_ops: Vec<PreparedSlotOp>,
    active_screen: Option<(String, String)>,
    widget_tree: Option<serde_json::Value>,
    split_sizes: HashMap<String, Vec<f32>>,
    split_drag: Option<SplitDragInfo>,
    /// Autocmd subscriptions indexed by event name for O(1) dispatch.
    /// Each entry is `(plugin_id, callback registry key)` in declaration
    /// order across all plugins.
    autocmds: HashMap<String, Vec<(String, RegistryKey)>>,
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
}

impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHost {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            slot_ops: Vec::new(),
            active_screen: None,
            widget_tree: None,
            split_sizes: HashMap::new(),
            split_drag: None,
            autocmds: HashMap::new(),
            last_repository_hash: None,
            last_tab_snapshot: TabsSnapshot::default(),
            pending_tab_ops: Rc::new(RefCell::new(Vec::new())),
            audit_log: AuditLog::new(),
            last_reload_errors: HashMap::new(),
            service_registry: Rc::new(RefCell::new(ServiceRegistry::new())),
        }
    }

    /// Cheap-clone handle to the per-host capability audit log. Will be
    /// consumed by the devtools panel (Phase 6).
    #[allow(dead_code)]
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

    pub fn load_from_dir(&mut self, dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() && p.join("plugin.toml").exists() && p.join("init.lua").exists() {
                if let Err(e) = self.load_plugin(&p) {
                    eprintln!(
                        "git_leviathan: plugin load failed {}: {e}",
                        p.display()
                    );
                }
            }
        }
    }

    pub fn load_plugin(&mut self, dir: &Path) -> Result<(), PluginLoadError> {
        let manifest_str = fs::read_to_string(dir.join("plugin.toml"))?;
        let manifest: PluginManifest = toml::from_str(&manifest_str)?;
        if !manifest.api_version.is_compatible_with(HOST_API_VERSION) {
            return Err(PluginLoadError::BadManifest(format!(
                "api version {}.{} not compatible with host {}.{}",
                manifest.api_version.major,
                manifest.api_version.minor,
                HOST_API_VERSION.major,
                HOST_API_VERSION.minor
            )));
        }
        let init_src = fs::read_to_string(dir.join("init.lua"))?;

        let plugin_root = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let state_dir = dirs::state_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("git_leviathan")
            .join(&manifest.id);
        let config_dir = dirs::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("git_leviathan")
            .join(&manifest.id);
        let workdir: Option<PathBuf> = None;
        let guard = Rc::new(
            CapabilityGuard::new(
                manifest.capabilities.clone(),
                plugin_root.clone(),
                state_dir,
                config_dir,
                workdir,
            )
            .with_audit(self.audit_log.clone(), manifest.id.clone()),
        );

        let lua = Rc::new(Lua::new());
        let build: Rc<RefCell<BuildState>> = Rc::new(RefCell::new(BuildState::default()));
        let deferred: Rc<RefCell<DeferredQueue>> =
            Rc::new(RefCell::new(DeferredQueue::default()));
        let user_commands: Rc<RefCell<UserCommands>> =
            Rc::new(RefCell::new(UserCommands::default()));

        let services_ctx = ServicesContext {
            registry: Rc::clone(&self.service_registry),
            plugin_id: manifest.id.clone(),
            provides: manifest.provides_services.clone(),
            consumes: manifest.consumes_services.clone(),
            plugin_lua: Rc::clone(&lua),
        };

        api::install_all(
            &lua,
            Rc::clone(&build),
            Rc::clone(&self.pending_tab_ops),
            Rc::clone(&guard),
            services_ctx,
            Rc::clone(&deferred),
            Rc::clone(&user_commands),
        )?;

        if let Err(e) = lua
            .load(&init_src)
            .set_name(format!("plugins/{}/init.lua", manifest.id))
            .exec()
        {
            return Err(PluginLoadError::Plugin(
                git_leviathan_plugin_api::error::PluginError::from_mlua(
                    &manifest.id,
                    "init.lua exec",
                    &e,
                ),
            ));
        }

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
                &mut slot_handlers,
                &mut dynamic_widgets,
            ) {
                Ok(prepared) => self.slot_ops.push(prepared),
                Err(e) => eprintln!(
                    "git_leviathan: plugin {} slot op ignored: {e}",
                    manifest.id
                ),
            }
        }

        for api::RawAutocmd { event, callback } in autocmds {
            self.autocmds
                .entry(event)
                .or_default()
                .push((manifest.id.clone(), callback));
        }

        eprintln!(
            "git_leviathan: loaded plugin {} ({})",
            manifest.id, manifest.name
        );
        let plugin = LoadedPlugin {
            id: manifest.id.clone(),
            root: plugin_root,
            lua,
            slot_handlers,
            screens,
            screen_state: HashMap::new(),
            dynamic_widgets,
            deferred,
            user_commands,
        };
        self.plugins.insert(manifest.id.clone(), plugin);

        // Populate dynamic widget caches so the first render has a real
        // tree instead of a placeholder null.
        self.refresh_dynamic_widgets_for_plugin(&manifest.id);
        Ok(())
    }

    /// Apply every collected `main_bar` op to a `MainBarRegistry`.
    pub fn apply_main_bar_slots(&self, registry: &mut MainBarRegistry) {
        self.apply_region_slots(registry, "main_bar", true, PreparedSlot::into_main_bar);
    }

    /// Apply every collected `repository` op to a `RepoRegionRegistry`.
    pub fn apply_repo_region_slots(&self, registry: &mut RepoRegionRegistry) {
        self.apply_region_slots(registry, "repository", false, PreparedSlot::into_repo_pane);
    }

    /// Apply every collected `tab_bar` op to a `TabBarRegistry`.
    pub fn apply_tab_bar_slots(&self, registry: &mut TabBarRegistry) {
        self.apply_region_slots(registry, "tab_bar", false, PreparedSlot::into_tab_bar);
    }

    fn apply_region_slots<T: IsSlot>(
        &self,
        registry: &mut SlotRegistry<T>,
        region_name: &str,
        handle_any_container: bool,
        convert: impl Fn(PreparedSlot) -> T,
    ) {
        for op in &self.slot_ops {
            match op {
                PreparedSlotOp::Add(p) if p.region == region_name => {
                    registry.add(convert(p.clone()));
                }
                PreparedSlotOp::Replace { region, id, spec, .. } if region == region_name => {
                    if !registry.replace(id, convert(spec.clone())) {
                        eprintln!("git_leviathan: regions.replace_slot({region_name}, \"{id}\") — no such slot");
                    }
                }
                PreparedSlotOp::Remove { region, id, .. } if region == region_name => {
                    if !registry.remove(id) {
                        eprintln!("git_leviathan: regions.remove_slot({region_name}, \"{id}\") — no such slot");
                    }
                }
                PreparedSlotOp::RemoveAnyContainer { region, id }
                    if handle_any_container && region == region_name =>
                {
                    if !registry.remove(id) {
                        eprintln!("git_leviathan: {region_name}.remove(\"{id}\") — no such slot");
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
        let nav: Option<String> = {
            let Some(plugin) = self.plugins.get(plugin_id) else {
                return;
            };
            let Some(key) = plugin.slot_handlers.get(&handler_key) else {
                return;
            };
            let func: Function = match plugin.lua.registry_value(key) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("git_leviathan: slot handler lookup failed: {e}");
                    return;
                }
            };
            let value_lua = match plugin.lua.to_value(&value) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("git_leviathan: slot value conv failed: {e}");
                    return;
                }
            };
            match func.call::<Option<Table>>((slot_id.to_string(), event.to_string(), value_lua)) {
                Ok(Some(t)) => t.get::<String>("navigate").ok(),
                Ok(None) => None,
                Err(e) => {
                    eprintln!("git_leviathan: slot handler error: {e}");
                    None
                }
            }
        };
        if let Some(target) = nav {
            self.open_screen(plugin_id.to_string(), target);
        }
    }
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

/// True when this slot op was registered by `plugin_id`. Used by
/// `reload_plugin` to park / restore plugin-owned host state.
///
/// `Remove` and `RemoveAnyContainer` carry no plugin id; they're
/// considered "host-owned" and never parked. Practical consequence: a
/// plugin's own removals against its slots aren't moved on reload, but
/// since the corresponding `Add` is parked alongside, the resulting
/// state is consistent.
fn op_belongs_to(op: &PreparedSlotOp, plugin_id: &str) -> bool {
    match op {
        PreparedSlotOp::Add(p) => p.plugin_id == plugin_id,
        PreparedSlotOp::Replace { spec, .. } => spec.plugin_id == plugin_id,
        PreparedSlotOp::Remove { .. } | PreparedSlotOp::RemoveAnyContainer { .. } => false,
    }
}

fn prepare_op(
    plugin_id: &str,
    plugin_root: &Path,
    op: RawSlotOp,
    handlers: &mut HashMap<String, RegistryKey>,
    dynamic_widgets: &mut HashMap<String, (RegistryKey, Rc<RefCell<serde_json::Value>>)>,
) -> Result<PreparedSlotOp, String> {
    match op {
        RawSlotOp::Add(raw) => {
            let prepared = prepare_slot(plugin_id, plugin_root, raw, handlers, dynamic_widgets)?;
            Ok(PreparedSlotOp::Add(prepared))
        }
        RawSlotOp::Remove { region, container, id } => {
            if container.is_empty() {
                Ok(PreparedSlotOp::RemoveAnyContainer { region, id })
            } else {
                Ok(PreparedSlotOp::Remove {
                    region,
                    container: parse_container(&container),
                    id,
                })
            }
        }
        RawSlotOp::Replace { region, container, id, spec } => {
            let prepared = prepare_slot(plugin_id, plugin_root, spec, handlers, dynamic_widgets)?;
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
    handlers: &mut HashMap<String, RegistryKey>,
    dynamic_widgets: &mut HashMap<String, (RegistryKey, Rc<RefCell<serde_json::Value>>)>,
) -> Result<PreparedSlot, String> {
    let RawSlotSpec {
        id,
        region,
        container,
        priority,
        widget,
        on_click,
    } = raw;
    let container_parsed = parse_container(&container);
    if let Some(key) = on_click {
        let handler_key = format!("{region}:{}:{id}", container_parsed.key());
        handlers.insert(handler_key, key);
    }
    let widget = match widget {
        WidgetSource::Static(v) => SlotWidget::Static(v),
        WidgetSource::Dynamic(key) => {
            let cache = Rc::new(RefCell::new(serde_json::Value::Null));
            dynamic_widgets.insert(id.clone(), (key, Rc::clone(&cache)));
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

    pub fn widget_tree(&self) -> Option<&serde_json::Value> {
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
        let nav: Option<String>;
        {
            let Some(plugin) = self.plugins.get_mut(plugin_id) else {
                return;
            };
            let Some(screen_def) = plugin.screens.get(screen_id) else {
                return;
            };
            let update_fn: Function = match plugin.lua.registry_value(&screen_def.update) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("git_leviathan: update lookup failed: {e}");
                    return;
                }
            };
            let state_val: LuaValue = match plugin.screen_state.get(screen_id) {
                Some(k) => plugin.lua.registry_value(k).unwrap_or(LuaValue::Nil),
                None => LuaValue::Nil,
            };
            let value_lua = match plugin.lua.to_value(&value) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("git_leviathan: value conv failed: {e}");
                    return;
                }
            };
            let result: mlua::Result<Table> =
                update_fn.call((state_val, event.to_string(), value_lua));
            match result {
                Ok(action) => {
                    if let Ok(new_state) = action.get::<LuaValue>("state") {
                        if !matches!(new_state, LuaValue::Nil) {
                            if let Ok(key) = plugin.lua.create_registry_value(new_state) {
                                plugin
                                    .screen_state
                                    .insert(screen_id.to_string(), key);
                            }
                        }
                    }
                    nav = action.get::<String>("navigate").ok();
                }
                Err(e) => {
                    eprintln!("git_leviathan: screen update error: {e}");
                    nav = None;
                }
            }
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
    pub fn screen_state_json(
        &self,
        plugin_id: &str,
        screen_id: &str,
    ) -> Option<serde_json::Value> {
        let plugin = self.plugins.get(plugin_id)?;
        let key = plugin.screen_state.get(screen_id)?;
        let v: LuaValue = plugin.lua.registry_value(key).ok()?;
        plugin.lua.from_value(v).ok()
    }

    /// Read a top-level Lua global from `plugin_id`'s VM as `i64`. Used
    /// by tests to observe side-effects of plugin code (e.g. results of
    /// `leviathan.services.get(...).method(...)`). Returns `None` when
    /// the plugin is unknown, the global doesn't exist, or the value is
    /// neither integer nor number.
    pub fn plugin_global_i64(&self, plugin_id: &str, name: &str) -> Option<i64> {
        let plugin = self.plugins.get(plugin_id)?;
        let v: LuaValue = plugin.lua.globals().get(name).ok()?;
        match v {
            LuaValue::Integer(i) => Some(i),
            LuaValue::Number(n) => Some(n as i64),
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
    pub fn has_slot(
        &self,
        plugin_id: &str,
        region: &str,
        container: &str,
        slot_id: &str,
    ) -> bool {
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
                PreparedSlotOp::Replace { region: r, container: c, id, spec }
                    if spec.plugin_id == plugin_id
                        && r == region
                        && c.key() == container
                        && id == slot_id =>
                {
                    return true;
                }
                PreparedSlotOp::Remove { region: r, container: c, id }
                    if r == region && c.key() == container && id == slot_id =>
                {
                    return false;
                }
                PreparedSlotOp::RemoveAnyContainer { region: r, id }
                    if r == region && id == slot_id =>
                {
                    return false;
                }
                _ => {}
            }
        }
        false
    }

    /// Reload a plugin's `init.lua` while preserving any currently open
    /// screens whose `serialize` and `deserialize` hooks are both defined.
    /// Screens without those hooks have their state freshly re-initialised
    /// via `init()`.
    ///
    /// Two-phase: park the existing plugin and the host-level state it
    /// owns (slot_ops, autocmds), then attempt a fresh load. On failure,
    /// restore the parked state and record the error in
    /// `last_reload_errors` — the prior version keeps serving. On
    /// success, clear any prior error.
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
        let dir = plugin.root.clone();

        let mut snapshots: HashMap<String, serde_json::Value> = HashMap::new();
        let mut open_screens: Vec<String> = Vec::new();
        for (screen_id, state_key) in plugin.screen_state.iter() {
            open_screens.push(screen_id.clone());
            let Some(screen_def) = plugin.screens.get(screen_id) else {
                continue;
            };
            let Some(ser_key) = &screen_def.serialize else {
                continue;
            };
            let Ok(ser_fn) = plugin.lua.registry_value::<Function>(ser_key) else {
                continue;
            };
            let Ok(state_val) = plugin.lua.registry_value::<LuaValue>(state_key) else {
                continue;
            };
            let Ok(snap_lua) = ser_fn.call::<LuaValue>(state_val) else {
                continue;
            };
            let Ok(snap_json) = plugin.lua.from_value(snap_lua) else {
                continue;
            };
            snapshots.insert(screen_id.clone(), snap_json);
        }

        // Park host-level state owned by this plugin so a fresh load
        // starts from a clean slate. We hand the parked pieces back if
        // the load fails.
        //
        // Services published by this plugin are dropped from the registry
        // before reload so the new init.lua's `services.register` doesn't
        // collide with the parked-plugin's registration. On reload-failure
        // the parked plugin keeps serving but its services stay
        // unregistered; consumers calling `services.get` will see the
        // service as missing until a successful reload restores it.
        self.service_registry.borrow_mut().unregister_for_plugin(plugin_id);
        let parked_plugin = self.plugins.remove(plugin_id).expect("verified above");
        let parked_slot_ops: Vec<PreparedSlotOp> = {
            let mut owned = Vec::new();
            let mut i = 0;
            while i < self.slot_ops.len() {
                if op_belongs_to(&self.slot_ops[i], plugin_id) {
                    owned.push(self.slot_ops.remove(i));
                } else {
                    i += 1;
                }
            }
            owned
        };
        let parked_autocmds: Vec<(String, Vec<(String, RegistryKey)>)> = {
            let mut snapshot: Vec<(String, Vec<(String, RegistryKey)>)> = Vec::new();
            // Drain entries belonging to this plugin out of the live map.
            // RegistryKey is non-Clone, so we move the vec slot-by-slot.
            let events: Vec<String> = self.autocmds.keys().cloned().collect();
            for event in events {
                let Some(list) = self.autocmds.get_mut(&event) else { continue };
                let mut owned: Vec<(String, RegistryKey)> = Vec::new();
                let mut i = 0;
                while i < list.len() {
                    if list[i].0 == plugin_id {
                        owned.push(list.swap_remove(i));
                    } else {
                        i += 1;
                    }
                }
                if !owned.is_empty() {
                    snapshot.push((event.clone(), owned));
                }
                if list.is_empty() {
                    self.autocmds.remove(&event);
                }
            }
            snapshot
        };

        if let Err(e) = self.load_plugin(&dir) {
            // Restore parked state. The new load may have partially
            // populated host-level state before erroring (slot_ops are
            // pushed during prepare_op which runs after init.lua exec
            // succeeds). Any state pushed under `plugin_id` by the
            // failed load gets purged, then the parked snapshot is
            // re-inserted so the previous good plugin keeps serving.
            self.slot_ops.retain(|op| !op_belongs_to(op, plugin_id));
            let events: Vec<String> = self.autocmds.keys().cloned().collect();
            for event in events {
                if let Some(list) = self.autocmds.get_mut(&event) {
                    list.retain(|(pid, _)| pid != plugin_id);
                    if list.is_empty() {
                        self.autocmds.remove(&event);
                    }
                }
            }
            self.plugins.remove(plugin_id);

            for op in parked_slot_ops {
                self.slot_ops.push(op);
            }
            for (event, list) in parked_autocmds {
                self.autocmds.entry(event).or_default().extend(list);
            }
            self.plugins.insert(plugin_id.to_string(), parked_plugin);

            let msg = e.to_string();
            self.last_reload_errors.insert(plugin_id.to_string(), msg.clone());
            return Err(git_leviathan_plugin_api::error::PluginError::new(
                plugin_id,
                "host.reload_plugin",
                format!("reload failed: {msg}"),
            ));
        }

        // Reload succeeded — discard the parked state by simply not
        // restoring it, and clear any prior error.
        drop(parked_plugin);
        drop(parked_slot_ops);
        drop(parked_autocmds);
        self.last_reload_errors.remove(plugin_id);

        let plugin = self.plugins.get_mut(plugin_id).ok_or_else(|| {
            git_leviathan_plugin_api::error::PluginError::new(
                plugin_id,
                "host.reload_plugin",
                "plugin missing after reload",
            )
        })?;
        for screen_id in open_screens {
            let Some(screen_def) = plugin.screens.get(&screen_id) else {
                continue;
            };
            if let Some(snap_json) = snapshots.get(&screen_id) {
                if let Some(de_key) = &screen_def.deserialize {
                    if let Ok(de_fn) = plugin.lua.registry_value::<Function>(de_key) {
                        if let Ok(snap_lua) = plugin.lua.to_value(snap_json) {
                            if let Ok(state_lua) = de_fn.call::<LuaValue>(snap_lua) {
                                if let Ok(state_key) =
                                    plugin.lua.create_registry_value(state_lua)
                                {
                                    plugin.screen_state.insert(screen_id, state_key);
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
            // No serialize/deserialize available — re-init fresh state.
            if let Ok(init_fn) = plugin.lua.registry_value::<Function>(&screen_def.init) {
                if let Ok(state) = init_fn.call::<LuaValue>(()) {
                    if let Ok(key) = plugin.lua.create_registry_value(state) {
                        plugin.screen_state.insert(screen_id, key);
                    }
                }
            }
        }

        self.refresh_active_widget_tree();
        Ok(())
    }

    pub fn open_screen(&mut self, plugin_id: String, screen_id: String) {
        let needs_init = self
            .plugins
            .get(&plugin_id)
            .map(|p| p.screens.contains_key(&screen_id) && !p.screen_state.contains_key(&screen_id))
            .unwrap_or(false);
        if needs_init {
            if let Some(plugin) = self.plugins.get_mut(&plugin_id) {
                if let Some(screen_def) = plugin.screens.get(&screen_id) {
                    match plugin.lua.registry_value::<Function>(&screen_def.init) {
                        Ok(init_fn) => match init_fn.call::<LuaValue>(()) {
                            Ok(state) => {
                                if let Ok(key) = plugin.lua.create_registry_value(state) {
                                    plugin.screen_state.insert(screen_id.clone(), key);
                                }
                            }
                            Err(e) => eprintln!("git_leviathan: init error: {e}"),
                        },
                        Err(e) => eprintln!("git_leviathan: init lookup failed: {e}"),
                    }
                }
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
        let Some(plugin) = self.plugins.get(&plugin_id) else {
            self.widget_tree = None;
            return;
        };
        let Some(screen_def) = plugin.screens.get(&screen_id) else {
            self.widget_tree = None;
            return;
        };
        let view_fn: Function = match plugin.lua.registry_value(&screen_def.view) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("git_leviathan: view lookup failed: {e}");
                return;
            }
        };
        let state_val: LuaValue = match plugin.screen_state.get(&screen_id) {
            Some(k) => plugin.lua.registry_value(k).unwrap_or(LuaValue::Nil),
            None => LuaValue::Nil,
        };
        let result: mlua::Result<LuaValue> = view_fn.call(state_val);
        match result {
            Ok(v) => {
                let json: Result<serde_json::Value, mlua::Error> = plugin.lua.from_value(v);
                match json {
                    Ok(tree) => self.widget_tree = Some(tree),
                    Err(e) => {
                        eprintln!("git_leviathan: view→json failed: {e}");
                        self.widget_tree = None;
                    }
                }
            }
            Err(e) => {
                eprintln!("git_leviathan: view call failed: {e}");
                self.widget_tree = None;
            }
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
        self.split_sizes.entry(split_key.to_string()).or_insert(sizes);
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

    /// Fire a host-side event. Invokes every plugin's registered
    /// callbacks for this event (see
    /// [`api::event`](crate::plugin::api::event)), then refreshes the
    /// dynamic main-bar widgets for every plugin that had a callback so
    /// UI state reflects any Lua-side state the callbacks just mutated.
    ///
    /// Silently no-ops when no plugin has subscribed — callers can fire
    /// liberally from lifecycle hooks without a subscription check.
    pub fn fire_event(&mut self, event: &str) {
        let affected = self.invoke_subscribers(event);
        for pid in affected {
            self.refresh_dynamic_widgets_for_plugin(&pid);
        }
    }

    /// Invoke every plugin's registered callbacks for `event` and return
    /// the set of plugin ids whose callbacks ran. The caller decides
    /// what (if anything) to do with that set — `fire_event` uses it to
    /// scope widget refreshes; `sync_repository` ignores it because it
    /// refreshes every plugin regardless.
    ///
    /// Walks by index so the immutable borrow of `self.autocmds` isn't
    /// held across the `self.plugins` lookup for each subscriber.
    fn invoke_subscribers(&self, event: &str) -> HashSet<String> {
        let mut affected: HashSet<String> = HashSet::new();
        let Some(subscribers) = self.autocmds.get(event) else {
            return affected;
        };
        if subscribers.is_empty() {
            return affected;
        }
        for i in 0..subscribers.len() {
            let (plugin_id, key) = {
                let list = self
                    .autocmds
                    .get(event)
                    .expect("autocmds entry vanished mid-dispatch");
                let (pid, k) = &list[i];
                (pid.clone(), k)
            };
            let Some(plugin) = self.plugins.get(&plugin_id) else {
                continue;
            };
            let func: Function = match plugin.lua.registry_value(key) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("git_leviathan: autocmd handler lookup failed: {e}");
                    continue;
                }
            };
            if let Err(e) = func.call::<()>(event.to_string()) {
                eprintln!("git_leviathan: autocmd handler error: {e}");
            }
            affected.insert(plugin_id);
        }
        affected
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
            let table = match api::repository::build_table(
                &plugin.lua,
                repo_name,
                workdir_path,
                current_branch_name,
                head_hash,
                default_remote_name,
                refs,
            ) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!(
                        "git_leviathan: build leviathan.repository failed for {}: {e}",
                        plugin.id
                    );
                    continue;
                }
            };
            let leviathan: Table = match plugin.lua.globals().get("leviathan") {
                Ok(t) => t,
                Err(e) => {
                    eprintln!(
                        "git_leviathan: `leviathan` global missing for {}: {e}",
                        plugin.id
                    );
                    continue;
                }
            };
            if let Err(e) = leviathan.set("repository", table) {
                eprintln!(
                    "git_leviathan: set leviathan.repository failed for {}: {e}",
                    plugin.id
                );
            }
        }

        // Run BranchChanged callbacks first so any Lua-side state they
        // mutate is fresh before widgets re-read the globals.
        self.invoke_subscribers("BranchChanged");

        let plugin_ids: Vec<String> = self.plugins.keys().cloned().collect();
        for pid in plugin_ids {
            self.refresh_dynamic_widgets_for_plugin(&pid);
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
            let leviathan: Table = match plugin.lua.globals().get("leviathan") {
                Ok(t) => t,
                Err(e) => {
                    eprintln!(
                        "git_leviathan: `leviathan` global missing for {}: {e}",
                        plugin.id
                    );
                    continue;
                }
            };
            if let Err(e) = api::tab_registry::refresh(&plugin.lua, &leviathan, snapshot) {
                eprintln!(
                    "git_leviathan: tab_registry refresh failed for {}: {e}",
                    plugin.id
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
        let now = Instant::now();
        let ids: Vec<String> = self.plugins.keys().cloned().collect();
        for id in ids {
            let Some(plugin) = self.plugins.get(&id) else {
                continue;
            };
            let lua = Rc::clone(&plugin.lua);
            let queue = Rc::clone(&plugin.deferred);

            let immediate = queue.borrow_mut().drain_immediate();
            for key in immediate {
                match lua.registry_value::<Function>(&key) {
                    Ok(f) => {
                        if let Err(e) = f.call::<()>(()) {
                            eprintln!("git_leviathan: scheduled fn error in {id}: {e}");
                        }
                    }
                    Err(e) => {
                        eprintln!("git_leviathan: scheduled fn lookup failed in {id}: {e}");
                    }
                }
            }

            let due = queue.borrow_mut().drain_due(now);
            for key in due {
                match lua.registry_value::<Function>(&key) {
                    Ok(f) => {
                        if let Err(e) = f.call::<()>(()) {
                            eprintln!("git_leviathan: defer_fn error in {id}: {e}");
                        }
                    }
                    Err(e) => {
                        eprintln!("git_leviathan: defer_fn lookup failed in {id}: {e}");
                    }
                }
            }

            let drained: Vec<RegistryKey> =
                std::mem::take(&mut queue.borrow_mut().coroutines);
            for key in drained {
                let thread: Thread = match lua.registry_value(&key) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("git_leviathan: coroutine lookup failed in {id}: {e}");
                        continue;
                    }
                };
                if let Err(e) = thread.resume::<()>(()) {
                    eprintln!("git_leviathan: coroutine resume error in {id}: {e}");
                    continue;
                }
                if thread.status() == ThreadStatus::Resumable {
                    match lua.create_registry_value(thread) {
                        Ok(new_key) => queue.borrow_mut().coroutines.push(new_key),
                        Err(e) => eprintln!(
                            "git_leviathan: re-parking coroutine failed in {id}: {e}"
                        ),
                    }
                }
            }
        }
    }

    /// Invoke a plugin's named user command. The function is wrapped in
    /// a Lua coroutine so cooperative yields are honoured: if the command
    /// yields, it's parked in the plugin's `coroutines` bucket and
    /// resumed on subsequent `tick` calls. Returns once the first resume
    /// finishes (either completed or yielded).
    pub fn invoke_user_command(
        &mut self,
        plugin_id: &str,
        name: &str,
    ) -> mlua::Result<()> {
        let plugin = self.plugins.get(plugin_id).ok_or_else(|| {
            mlua::Error::external(format!("plugin '{plugin_id}' not loaded"))
        })?;
        let f: Function = {
            let cmds = plugin.user_commands.borrow();
            let key = cmds.commands.get(name).ok_or_else(|| {
                mlua::Error::external(format!(
                    "user command '{name}' not registered"
                ))
            })?;
            plugin.lua.registry_value(key)?
        };
        let thread = plugin.lua.create_thread(f)?;
        thread.resume::<()>(())?;
        if thread.status() == ThreadStatus::Resumable {
            let key = plugin.lua.create_registry_value(thread)?;
            plugin.deferred.borrow_mut().coroutines.push(key);
        }
        Ok(())
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
    fn refresh_dynamic_widgets_for_plugin(&self, plugin_id: &str) {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return;
        };
        for (slot_id, (key, cache)) in &plugin.dynamic_widgets {
            let func: Function = match plugin.lua.registry_value(key) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!(
                        "git_leviathan: dynamic widget fn lookup failed for {slot_id}: {e}"
                    );
                    continue;
                }
            };
            let lua_val: LuaValue = match func.call(()) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "git_leviathan: dynamic widget fn error for {slot_id}: {e}"
                    );
                    continue;
                }
            };
            let json: serde_json::Value = match plugin.lua.from_value(lua_val) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "git_leviathan: dynamic widget→json failed for {slot_id}: {e}"
                    );
                    continue;
                }
            };
            *cache.borrow_mut() = json;
        }
    }
}

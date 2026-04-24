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

use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, Table, Value as LuaValue};
use serde::Deserialize;

use crate::plugin::api::{self, BuildState, RawSlotOp, RawSlotSpec, ScreenDef, WidgetSource};
use crate::plugin::ui::main_bar_slots::{
    parse_section, PreparedMainBarSlot, PreparedSlotOp, SlotWidget,
};
use crate::plugin::ui::split;
use crate::services::RepoRef;
use crate::widgets::chrome::main_bar::MainBarRegistry;

pub const PLUGIN_API_VERSION: u32 = 1;

#[derive(Debug)]
pub enum PluginLoadError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    Lua(mlua::Error),
    BadManifest(String),
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
        }
    }
}

impl std::error::Error for PluginLoadError {}

#[derive(Debug, Deserialize)]
struct Manifest {
    plugin: ManifestPlugin,
}

#[derive(Debug, Deserialize)]
struct ManifestPlugin {
    id: String,
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    version: String,
    #[serde(default)]
    api: Option<u32>,
}

struct LoadedPlugin {
    #[allow(dead_code)]
    id: String,
    /// Absolute path to the plugin's directory. Used as sandbox root when
    /// resolving plugin-bundled assets (icons, etc).
    root: PathBuf,
    lua: Lua,
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
    /// Ordered hook operations from `main_bar.{add,remove,replace}`.
    /// Preserved in plugin-load order so later plugins see the state
    /// earlier plugins produced.
    main_bar_slot_ops: Vec<PreparedSlotOp>,
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
            main_bar_slot_ops: Vec::new(),
            active_screen: None,
            widget_tree: None,
            split_sizes: HashMap::new(),
            split_drag: None,
            autocmds: HashMap::new(),
            last_repository_hash: None,
        }
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
        let manifest: Manifest = toml::from_str(&manifest_str)?;
        if manifest.plugin.api.unwrap_or(PLUGIN_API_VERSION) != PLUGIN_API_VERSION {
            return Err(PluginLoadError::BadManifest(format!(
                "api version mismatch: want {PLUGIN_API_VERSION}"
            )));
        }
        let init_src = fs::read_to_string(dir.join("init.lua"))?;

        let lua = Lua::new();
        let build: Rc<RefCell<BuildState>> = Rc::new(RefCell::new(BuildState::default()));

        api::install_all(&lua, Rc::clone(&build))?;

        lua.load(&init_src)
            .set_name(format!("plugins/{}/init.lua", manifest.plugin.id))
            .exec()?;

        let (screens, slot_ops, autocmds) = {
            let mut b = build.borrow_mut();
            (
                std::mem::take(&mut b.screens),
                std::mem::take(&mut b.slot_ops),
                std::mem::take(&mut b.autocmds),
            )
        };

        // Resolve hook-API slot operations. Each `Add`/`Replace` carries an
        // optional `on_click` RegistryKey — stash it in the plugin's
        // slot-handler map before we move the RawSlotSpec into a
        // PreparedMainBarSlot (which drops the key, keeping only the
        // `has_handler` flag). Dynamic widgets are also split out here
        // into `dynamic_widgets` so the host can re-invoke them on
        // autocmd fire.
        let root = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let mut slot_handlers = HashMap::new();
        let mut dynamic_widgets = HashMap::new();
        for op in slot_ops {
            match op {
                RawSlotOp::Add(raw) => {
                    match prepare_slot(
                        &manifest.plugin.id,
                        &root,
                        raw,
                        &mut slot_handlers,
                        &mut dynamic_widgets,
                    ) {
                        Ok(prepared) => self.main_bar_slot_ops.push(PreparedSlotOp::Add(prepared)),
                        Err(e) => eprintln!(
                            "git_leviathan: plugin {} main_bar.add ignored: {e}",
                            manifest.plugin.id
                        ),
                    }
                }
                RawSlotOp::Remove(id) => {
                    self.main_bar_slot_ops.push(PreparedSlotOp::Remove(id));
                }
                RawSlotOp::Replace(id, raw) => {
                    match prepare_slot(
                        &manifest.plugin.id,
                        &root,
                        raw,
                        &mut slot_handlers,
                        &mut dynamic_widgets,
                    ) {
                        Ok(prepared) => self
                            .main_bar_slot_ops
                            .push(PreparedSlotOp::Replace(id, prepared)),
                        Err(e) => eprintln!(
                            "git_leviathan: plugin {} main_bar.replace ignored: {e}",
                            manifest.plugin.id
                        ),
                    }
                }
            }
        }

        for api::RawAutocmd { event, callback } in autocmds {
            self.autocmds
                .entry(event)
                .or_default()
                .push((manifest.plugin.id.clone(), callback));
        }

        eprintln!(
            "git_leviathan: loaded plugin {} ({})",
            manifest.plugin.id, manifest.plugin.name
        );
        let plugin = LoadedPlugin {
            id: manifest.plugin.id.clone(),
            root,
            lua,
            slot_handlers,
            screens,
            screen_state: HashMap::new(),
            dynamic_widgets,
        };
        self.plugins.insert(manifest.plugin.id.clone(), plugin);

        // Populate dynamic widget caches so the first render has a real
        // tree instead of a placeholder null.
        self.refresh_dynamic_widgets_for_plugin(&manifest.plugin.id);
        Ok(())
    }

    /// Populate `registry` with every ordered slot op this host collected.
    /// Must be called after built-ins are in place so plugin removes and
    /// replaces can target them.
    pub fn register_main_bar_slots(&self, registry: &mut MainBarRegistry) {
        for op in &self.main_bar_slot_ops {
            match op {
                PreparedSlotOp::Add(prepared) => {
                    registry.add(prepared.clone().into_main_bar_slot());
                }
                PreparedSlotOp::Remove(id) => {
                    let removed = registry.remove(id);
                    if !removed {
                        eprintln!("git_leviathan: main_bar.remove(\"{id}\") — no such slot");
                    }
                }
                PreparedSlotOp::Replace(id, prepared) => {
                    let replaced = registry.replace(id, prepared.clone().into_main_bar_slot());
                    if !replaced {
                        eprintln!("git_leviathan: main_bar.replace(\"{id}\", …) — no such slot");
                    }
                }
            }
        }
    }

    /// Invoke a plugin's slot-click handler. Silently no-ops if the plugin
    /// is gone, the slot has no handler, or the Lua call errors.
    pub fn dispatch_slot_click(&mut self, plugin_id: &str, slot_id: &str) {
        let nav: Option<String> = {
            let Some(plugin) = self.plugins.get(plugin_id) else {
                return;
            };
            let Some(key) = plugin.slot_handlers.get(slot_id) else {
                return;
            };
            let func: Function = match plugin.lua.registry_value(key) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("git_leviathan: slot handler lookup failed: {e}");
                    return;
                }
            };
            match func.call::<Option<Table>>(slot_id.to_string()) {
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
fn compute_repo_hash(repo_name: &str, current_branch_name: &str, refs: &[RepoRef]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut h = DefaultHasher::new();
    repo_name.hash(&mut h);
    current_branch_name.hash(&mut h);
    refs.len().hash(&mut h);
    for r in refs {
        r.name.hash(&mut h);
        r.kind.hash(&mut h);
        r.target_hash.hash(&mut h);
        r.remote_name.hash(&mut h);
        r.is_current.hash(&mut h);
        r.upstream_ref.hash(&mut h);
    }
    h.finish()
}

/// Resolve a raw slot spec into the form the registry renderer wants.
///
/// Parses the `section` string, stashes the slot's `on_click` (if any) in
/// `handlers` keyed by the slot id, and splits dynamic widgets into a
/// `(fn key, cache cell)` pair stored in `dynamic_widgets`. The static
/// widget tree (or the cache Rc for dynamic slots) is carried on the
/// returned `PreparedMainBarSlot` for the renderer.
fn prepare_slot(
    plugin_id: &str,
    plugin_root: &Path,
    raw: RawSlotSpec,
    handlers: &mut HashMap<String, RegistryKey>,
    dynamic_widgets: &mut HashMap<String, (RegistryKey, Rc<RefCell<serde_json::Value>>)>,
) -> Result<PreparedMainBarSlot, String> {
    let RawSlotSpec {
        id,
        section,
        priority,
        widget,
        on_click,
    } = raw;

    let section = parse_section(&section)?;
    if let Some(key) = on_click {
        handlers.insert(id.clone(), key);
    }
    let widget = match widget {
        WidgetSource::Static(v) => SlotWidget::Static(v),
        WidgetSource::Dynamic(key) => {
            let cache = Rc::new(RefCell::new(serde_json::Value::Null));
            dynamic_widgets.insert(id.clone(), (key, Rc::clone(&cache)));
            SlotWidget::Dynamic(cache)
        }
    };
    Ok(PreparedMainBarSlot {
        plugin_id: plugin_id.to_string(),
        id,
        section,
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
        current_branch_name: &str,
        refs: &[RepoRef],
    ) {
        let hash = compute_repo_hash(repo_name, current_branch_name, refs);
        if self.last_repository_hash == Some(hash) {
            return;
        }
        self.last_repository_hash = Some(hash);

        for plugin in self.plugins.values() {
            let table = match api::repository::build_table(
                &plugin.lua,
                repo_name,
                current_branch_name,
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

//! Per-plugin Lua module loader, after-directory runner, and strict-
//! globals enforcement.
//!
//! Phase 5 wiring. The host owns every effect:
//!
//! - We replace Lua's stock `require` with a host-implemented closure
//!   that resolves names against the plugin's `PluginRuntimePath`,
//!   reads the file from disk in Rust, validates the bytes, and only
//!   then hands them to `Lua::load(...).set_name(...).exec()`. Plugins
//!   never call `dofile`/`loadfile` — those are removed from the
//!   global env.
//! - The per-generation module cache lives here. It is keyed by module
//!   name (the plugin id is implicit — every loader is bound to one
//!   generation). Reload drops the whole loader, dropping the cache,
//!   so a fresh generation never sees stale module values.
//! - `after/plugin/*.lua` is walked in lexical order *after* `init.lua`
//!   succeeds. Each file is compiled and executed individually so a
//!   single bad file produces a precise diagnostic without skipping
//!   later siblings.
//! - Strict globals install a metatable on `_G` that raises on read of
//!   undeclared globals and on write of new globals. Reads/writes to
//!   already-defined globals (the Lua stdlib, the `leviathan` table,
//!   anything the plugin set up via `local` or its own metatables) go
//!   through the rawget/rawset paths and never trigger our metamethods.
//!
//! Diagnostic codes emitted from this module:
//!
//! - `lua.module.invalid_name` — `require("..")`, `require("a/b")`,
//!   leading slashes, etc.
//! - `lua.module.forbidden` — `require("dep_id.x")` where `dep_id`
//!   isn't in the plugin's `requires_plugins`.
//! - `lua.module.not_found` — name resolved past the host filter but
//!   no file matched.
//! - `lua.module.read_failed` — file matched but I/O failed.
//! - `lua.module.exec_failed` — Lua chunk raised on exec.
//! - `lua.strict_globals.read` — undeclared global read attempt.
//! - `lua.strict_globals.write` — undeclared global write attempt.
//! - `runtime.after_dir_failed` — `read_dir(after/plugin)` or read of
//!   a single file failed during the after-walk.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mlua::{Lua, Table, Value as LuaValue};

use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::resources::{GenerationId, PluginId};
use crate::plugin::runtime_path::{PluginRuntimePath, RuntimePathKind};

#[derive(Debug, Clone)]
pub struct LoadedModuleRecord {
    pub plugin_id: String,
    pub module_name: String,
    pub source_path: PathBuf,
    pub kind: RuntimePathKind,
}

#[derive(Debug, Default)]
struct ModuleCache {
    /// Module name -> source location of the file we loaded. The
    /// actual cached value is held inside a Lua table on the Lua side
    /// (so identity is preserved across calls). This struct only
    /// tracks the metadata `module_graph()` exposes.
    records: BTreeMap<String, LoadedModuleRecord>,
}

/// Per-generation Lua loader. Owns the module cache, the runtime path
/// view, and the strict-globals policy. Cheap-clone (Rc + RefCell);
/// the host hands a clone to the `leviathan.runtime` Lua surface so
/// `runtime.module_graph()` can read live cache state.
#[derive(Clone)]
pub struct LuaLoader {
    inner: Rc<LuaLoaderInner>,
}

struct LuaLoaderInner {
    plugin_id: PluginId,
    generation_id: GenerationId,
    runtime_path: PluginRuntimePath,
    cache: RefCell<ModuleCache>,
    diagnostics: DiagnosticStore,
    strict_globals: bool,
}

impl LuaLoader {
    pub fn new(
        plugin_id: PluginId,
        generation_id: GenerationId,
        runtime_path: PluginRuntimePath,
        diagnostics: DiagnosticStore,
        strict_globals: bool,
    ) -> Self {
        Self {
            inner: Rc::new(LuaLoaderInner {
                plugin_id,
                generation_id,
                runtime_path,
                cache: RefCell::new(ModuleCache::default()),
                diagnostics,
                strict_globals,
            }),
        }
    }

    pub fn runtime_path(&self) -> &PluginRuntimePath {
        &self.inner.runtime_path
    }

    /// Snapshot of currently cached modules, sorted by module name.
    pub fn module_records(&self) -> Vec<LoadedModuleRecord> {
        self.inner
            .cache
            .borrow()
            .records
            .values()
            .cloned()
            .collect()
    }

    /// Install the host-controlled `require`, the module cache table,
    /// and (when strict) the global-access metatable. Must be called
    /// after `api::install_all` so the snapshot of "known globals"
    /// includes the `leviathan.*` surface.
    pub fn install(&self, lua: &Lua) -> mlua::Result<()> {
        // 1. Drop unsafe loaders so plugins can't bypass us.
        let globals = lua.globals();
        globals.set("dofile", LuaValue::Nil)?;
        globals.set("loadfile", LuaValue::Nil)?;

        // 2. Backing cache table living in Lua. Maps module-name -> the
        // value the chunk returned (or `true` if it returned nothing).
        // Owned by Lua so cached modules retain their identity for
        // repeat `require("x") == require("x")` checks.
        let cache_tbl = lua.create_table()?;
        lua.set_named_registry_value("__leviathan_module_cache", cache_tbl)?;

        // 3. Install our require.
        let loader = self.clone();
        let require_fn = lua
            .create_function(move |inner_lua, name: String| loader.do_require(inner_lua, &name))?;
        globals.set("require", require_fn)?;

        // 4. Strict globals.
        if self.inner.strict_globals {
            self.install_strict_globals(lua)?;
        }

        Ok(())
    }

    /// Run the plugin's `<root>/after/plugin/*.lua` files in lexical
    /// order. Each file is independent — failure in one doesn't skip
    /// the rest. Recorded module entries (if any chunk happens to set
    /// a global we own) are not added to the cache: after-files are
    /// run for side effects, not as require'able modules.
    pub fn run_after_plugin(&self, lua: &Lua, plugin_root: &Path) {
        let after_dir = plugin_root.join("after").join("plugin");
        if !after_dir.is_dir() {
            return;
        }
        let entries = match fs::read_dir(&after_dir) {
            Ok(e) => e,
            Err(e) => {
                self.inner.diagnostics.record(
                    PluginDiagnostic::new(
                        self.inner.plugin_id.clone(),
                        DiagnosticSeverity::Warning,
                        "runtime.after_dir_failed",
                        format!("could not read {}: {e}", after_dir.display()),
                    )
                    .with_generation(self.inner.generation_id),
                );
                return;
            }
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|de| de.path())
            .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("lua"))
            .collect();
        files.sort();

        for file in files {
            let chunk_name = format!(
                "plugins/{}/after/plugin/{}",
                self.inner.plugin_id,
                file.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("<unknown>")
            );
            let src = match fs::read(&file) {
                Ok(s) => s,
                Err(e) => {
                    self.inner.diagnostics.record(
                        PluginDiagnostic::new(
                            self.inner.plugin_id.clone(),
                            DiagnosticSeverity::Warning,
                            "runtime.after_dir_failed",
                            format!("could not read {}: {e}", file.display()),
                        )
                        .with_generation(self.inner.generation_id)
                        .with_source(PluginSourceSpan::Lua {
                            file: chunk_name.clone(),
                            line: None,
                            traceback: None,
                        }),
                    );
                    continue;
                }
            };
            if let Err(e) = lua.load(&src).set_name(chunk_name.clone()).exec() {
                self.inner.diagnostics.record(
                    PluginDiagnostic::new(
                        self.inner.plugin_id.clone(),
                        DiagnosticSeverity::Error,
                        "lua.after_failed",
                        format!("after/plugin file {} failed", file.display()),
                    )
                    .with_generation(self.inner.generation_id)
                    .with_mlua_error(&chunk_name, &e),
                );
            }
        }
    }

    fn do_require(&self, lua: &Lua, name: &str) -> mlua::Result<LuaValue> {
        // Validate name shape first — every other failure mode is a
        // diagnostic + Lua error, but an invalid name short-circuits.
        if let Err(err) = PluginRuntimePath::validate_module_name(name) {
            self.emit_module_error("lua.module.invalid_name", name, err.as_str(), None);
            return Err(mlua::Error::external(format!(
                "require(\"{name}\"): {}",
                err.as_str()
            )));
        }

        // Cache hit? Any non-nil entry counts.
        let cache_tbl: Table = lua.named_registry_value("__leviathan_module_cache")?;
        let cached: LuaValue = cache_tbl.get(name)?;
        if !matches!(cached, LuaValue::Nil) {
            return Ok(cached);
        }

        // Resolve to a file via the runtime path.
        let head = name.split('.').next().unwrap_or(name);
        if !self.inner.runtime_path.allows_prefix(head) {
            self.emit_module_error(
                "lua.module.forbidden",
                name,
                &format!("module prefix '{head}' not in this plugin's requires_plugins"),
                None,
            );
            return Err(mlua::Error::external(format!(
                "require(\"{name}\"): plugin '{}' may not load modules from '{head}' \
                 (declare it under [plugin] requires_plugins to allow)",
                self.inner.plugin_id
            )));
        }
        let (entry, source_path) = match self.inner.runtime_path.find(name) {
            Some(found) => found,
            None => {
                self.emit_module_error(
                    "lua.module.not_found",
                    name,
                    &format!("no file for module '{name}' under any allowed lua/ root"),
                    None,
                );
                return Err(mlua::Error::external(format!(
                    "require(\"{name}\"): module not found"
                )));
            }
        };

        let bytes = match fs::read(&source_path) {
            Ok(b) => b,
            Err(e) => {
                self.emit_module_error(
                    "lua.module.read_failed",
                    name,
                    &format!("read failed: {e}"),
                    Some(source_path.display().to_string()),
                );
                return Err(mlua::Error::external(format!(
                    "require(\"{name}\"): could not read {}: {e}",
                    source_path.display()
                )));
            }
        };

        let chunk_name = format!(
            "plugins/{}/lua/{}",
            entry.plugin_id,
            source_path
                .strip_prefix(&entry.lua_root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| source_path.display().to_string()),
        );
        let chunk = lua.load(&bytes).set_name(chunk_name.clone());
        let value: LuaValue = match chunk.eval() {
            Ok(v) => v,
            Err(e) => {
                self.inner.diagnostics.record(
                    PluginDiagnostic::new(
                        self.inner.plugin_id.clone(),
                        DiagnosticSeverity::Error,
                        "lua.module.exec_failed",
                        format!("module '{name}' failed to load"),
                    )
                    .with_generation(self.inner.generation_id)
                    .with_mlua_error(&chunk_name, &e),
                );
                return Err(e);
            }
        };
        // Lua semantics: a chunk that returns nothing yields `true`.
        let to_cache = match value {
            LuaValue::Nil => LuaValue::Boolean(true),
            other => other,
        };
        cache_tbl.set(name.to_string(), to_cache.clone())?;
        self.inner.cache.borrow_mut().records.insert(
            name.to_string(),
            LoadedModuleRecord {
                plugin_id: entry.plugin_id.clone(),
                module_name: name.to_string(),
                source_path: source_path.clone(),
                kind: entry.kind,
            },
        );
        let _ = entry; // RuntimePathEntry borrowed only above.
        Ok(to_cache)
    }

    fn emit_module_error(&self, code: &str, name: &str, msg: &str, file: Option<String>) {
        let mut diag = PluginDiagnostic::new(
            self.inner.plugin_id.clone(),
            DiagnosticSeverity::Error,
            code,
            format!("require(\"{name}\"): {msg}"),
        )
        .with_generation(self.inner.generation_id)
        .with_context(serde_json::json!({ "module": name }));
        if let Some(file) = file {
            diag = diag.with_source(PluginSourceSpan::Lua {
                file,
                line: None,
                traceback: None,
            });
        }
        self.inner.diagnostics.record(diag);
    }

    fn install_strict_globals(&self, lua: &Lua) -> mlua::Result<()> {
        let globals = lua.globals();

        // Snapshot the currently-defined keys. Anything set up to this
        // point (the Lua stdlib, the `leviathan.*` surface, our
        // overridden `require`) is allowed to be read freely.
        let mut existing: HashSet<String> = HashSet::new();
        for pair in globals.clone().pairs::<LuaValue, LuaValue>() {
            let (key, _) = pair?;
            if let LuaValue::String(s) = key {
                if let Ok(name) = s.to_str() {
                    existing.insert(name.to_string());
                }
            }
        }
        // Also pre-allow the `_G` self-reference, the `_ENV`-style
        // `_G` global, and `package`/`_G` if missing — `_G` is in
        // every Lua and is always present, but we want it explicit.
        existing.insert("_G".into());
        existing.insert("_ENV".into());
        // The `arg` global is set by Lua's CLI for scripts; not
        // present in embedded states, but keep it safe.
        existing.insert("arg".into());

        // Lua's `__index` only fires when the key is *missing* from
        // the table itself; same for `__newindex`. Wrapping `_G` with
        // a metatable that catches both is enough — established
        // globals (the stdlib, `leviathan`, `require`) hit rawget /
        // rawset and never trip our metamethods. The `allowed` set
        // shadows that to surface a precise diagnostic for the rare
        // case where Lua's lookup logic still routes through __index
        // (e.g. a `rawset(_G, name, nil)` followed by a re-read), and
        // for `__newindex` writes that should rawset back into `_G`.
        let allowed = Rc::new(existing);

        let mt = lua.create_table()?;

        let store = self.inner.diagnostics.clone();
        let plugin_id = self.inner.plugin_id.clone();
        let gen_id = self.inner.generation_id;
        let allowed_for_index = Rc::clone(&allowed);
        let index_fn = lua.create_function(
            move |_, (_t, key): (Table, LuaValue)| -> mlua::Result<LuaValue> {
                let key_str = match &key {
                    LuaValue::String(s) => s.to_str().map(|c| c.to_string()).ok(),
                    _ => None,
                };
                let key_repr = key_str.clone().unwrap_or_else(|| format!("{key:?}"));
                if let Some(name) = &key_str {
                    if allowed_for_index.contains(name) {
                        return Ok(LuaValue::Nil);
                    }
                }
                store.record(
                    PluginDiagnostic::new(
                        plugin_id.clone(),
                        DiagnosticSeverity::Warning,
                        "lua.strict_globals.read",
                        format!("read of undeclared global '{key_repr}'"),
                    )
                    .with_generation(gen_id)
                    .with_context(serde_json::json!({ "global": key_repr })),
                );
                Err(mlua::Error::external(format!(
                    "strict globals: read of undeclared global '{key_repr}' \
                     (use `local` or set `[runtime] strict_globals = false` to opt out)"
                )))
            },
        )?;
        mt.set("__index", index_fn)?;

        let store = self.inner.diagnostics.clone();
        let plugin_id = self.inner.plugin_id.clone();
        let gen_id = self.inner.generation_id;
        let allowed_for_newindex = Rc::clone(&allowed);
        let newindex_fn = lua.create_function(
            move |inner_lua, (_t, key, value): (Table, LuaValue, LuaValue)| -> mlua::Result<()> {
                let key_str = match &key {
                    LuaValue::String(s) => s.to_str().map(|c| c.to_string()).ok(),
                    _ => None,
                };
                let key_repr = key_str.clone().unwrap_or_else(|| format!("{key:?}"));
                if let Some(name) = key_str {
                    if allowed_for_newindex.contains(&name) {
                        // Reassigning an already-known global. Route
                        // the write straight at rawset so subsequent
                        // reads stay in the fast path.
                        let g: Table = inner_lua.globals();
                        g.raw_set(name.as_str(), value)?;
                        return Ok(());
                    }
                    store.record(
                        PluginDiagnostic::new(
                            plugin_id.clone(),
                            DiagnosticSeverity::Warning,
                            "lua.strict_globals.write",
                            format!("write to undeclared global '{name}'"),
                        )
                        .with_generation(gen_id)
                        .with_context(serde_json::json!({ "global": name })),
                    );
                    return Err(mlua::Error::external(format!(
                        "strict globals: write to undeclared global '{name}' \
                         (use `local` or set `[runtime] strict_globals = false` to opt out)"
                    )));
                }
                store.record(
                    PluginDiagnostic::new(
                        plugin_id.clone(),
                        DiagnosticSeverity::Warning,
                        "lua.strict_globals.write",
                        format!("write to non-string global key '{key_repr}'"),
                    )
                    .with_generation(gen_id),
                );
                Err(mlua::Error::external(format!(
                    "strict globals: write to non-string global key '{key_repr}'"
                )))
            },
        )?;
        mt.set("__newindex", newindex_fn)?;

        globals.set_metatable(Some(mt));
        Ok(())
    }
}

/// Build the Lua-side `leviathan.runtime` table. Three functions, all
/// host-implemented:
///
/// - `path()` -> array of `{plugin = string, kind = string, root = string}`
/// - `find(module)` -> matching `{plugin, source}` table or nil
/// - `module_graph()` -> array of `{plugin = string, modules = {...}}`
///   summarising what's currently cached.
///
/// All three read from the loader the host hands in. Reload swaps the
/// loader, so the runtime table from a freshly-loaded generation never
/// sees a previous generation's records.
pub fn install_runtime_module(lua: &Lua, leviathan: &Table, loader: LuaLoader) -> mlua::Result<()> {
    let runtime_tbl = lua.create_table()?;

    let loader_for_path = loader.clone();
    runtime_tbl.set(
        "path",
        lua.create_function(move |inner_lua, ()| -> mlua::Result<Table> {
            let arr = inner_lua.create_table()?;
            for (idx, entry) in loader_for_path.runtime_path().entries().iter().enumerate() {
                let row = inner_lua.create_table()?;
                row.set("plugin", entry.plugin_id.as_str())?;
                row.set("kind", entry.kind.as_str())?;
                row.set("root", entry.lua_root.display().to_string())?;
                arr.set(idx + 1, row)?;
            }
            Ok(arr)
        })?,
    )?;

    let loader_for_find = loader.clone();
    runtime_tbl.set(
        "find",
        lua.create_function(move |inner_lua, name: String| -> mlua::Result<LuaValue> {
            if PluginRuntimePath::validate_module_name(&name).is_err() {
                return Ok(LuaValue::Nil);
            }
            match loader_for_find.runtime_path().find(&name) {
                Some((entry, path)) => {
                    let tbl = inner_lua.create_table()?;
                    tbl.set("plugin", entry.plugin_id.as_str())?;
                    tbl.set("kind", entry.kind.as_str())?;
                    tbl.set("source", path.display().to_string())?;
                    Ok(LuaValue::Table(tbl))
                }
                None => Ok(LuaValue::Nil),
            }
        })?,
    )?;

    let loader_for_graph = loader.clone();
    runtime_tbl.set(
        "module_graph",
        lua.create_function(move |inner_lua, ()| -> mlua::Result<Table> {
            let mut by_plugin: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for record in loader_for_graph.module_records() {
                by_plugin
                    .entry(record.plugin_id.clone())
                    .or_default()
                    .push(record.module_name.clone());
            }
            let arr = inner_lua.create_table()?;
            for (idx, (plugin, mut modules)) in by_plugin.into_iter().enumerate() {
                modules.sort();
                let row = inner_lua.create_table()?;
                row.set("plugin", plugin)?;
                let modules_tbl = inner_lua.create_table()?;
                for (i, m) in modules.into_iter().enumerate() {
                    modules_tbl.set(i + 1, m)?;
                }
                row.set("modules", modules_tbl)?;
                arr.set(idx + 1, row)?;
            }
            Ok(arr)
        })?,
    )?;

    leviathan.set("runtime", runtime_tbl)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::diagnostic::{DiagnosticStore, NullSink};
    use crate::plugin::runtime_path::{PluginRuntimePath, RuntimePathRegistry};
    use std::sync::Arc;
    use tempfile::tempdir;

    fn quiet_store() -> DiagnosticStore {
        DiagnosticStore::with_sink(Arc::new(NullSink))
    }

    fn make_loader_with_files(
        plugin_id: &str,
        files: &[(&str, &str)],
    ) -> (LuaLoader, tempfile::TempDir) {
        let tmp = tempdir().unwrap();
        let plugin_root = tmp.path().to_path_buf();
        let lua_dir = plugin_root.join("lua").join(plugin_id);
        fs::create_dir_all(&lua_dir).unwrap();
        for (rel, body) in files {
            let path = lua_dir.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, body).unwrap();
        }
        let registry = RuntimePathRegistry::new();
        let path = PluginRuntimePath::resolve(plugin_id, &plugin_root, &[], &registry);
        let loader = LuaLoader::new(
            PluginId::from(plugin_id),
            GenerationId::new(1),
            path,
            quiet_store(),
            true,
        );
        (loader, tmp)
    }

    #[test]
    fn require_loads_a_local_module() {
        let (loader, _tmp) = make_loader_with_files("p", &[("foo.lua", "return { v = 7 }")]);
        let lua = Lua::new();
        loader.install(&lua).unwrap();
        let v: i64 = lua
            .load("local m = require('p.foo') return m.v")
            .eval()
            .unwrap();
        assert_eq!(v, 7);
        assert_eq!(loader.module_records().len(), 1);
    }

    #[test]
    fn require_invalid_name_is_rejected() {
        let (loader, _tmp) = make_loader_with_files("p", &[("foo.lua", "return 1")]);
        let lua = Lua::new();
        loader.install(&lua).unwrap();
        let r: mlua::Result<i64> = lua.load("return require('..')").eval();
        assert!(r.is_err());
    }

    #[test]
    fn require_other_plugin_without_dep_forbidden() {
        let (loader, _tmp) = make_loader_with_files("p", &[("foo.lua", "return 1")]);
        let lua = Lua::new();
        loader.install(&lua).unwrap();
        let r: mlua::Result<i64> = lua.load("return require('other.foo')").eval();
        assert!(r.is_err());
    }

    #[test]
    fn require_caches_modules() {
        let (loader, _tmp) = make_loader_with_files("p", &[("foo.lua", "return { n = 0 }")]);
        let lua = Lua::new();
        loader.install(&lua).unwrap();
        let _: LuaValue = lua.load("require('p.foo')").eval().unwrap();
        let _: LuaValue = lua.load("require('p.foo')").eval().unwrap();
        // Two `require`s, one cached record.
        assert_eq!(loader.module_records().len(), 1);
    }
}

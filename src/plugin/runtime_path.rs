//! Plugin runtimepath registry.
//!
//! The package layout supports multi-file plugins. Every plugin lives in
//! `<plugin_root>/` with the layout:
//!
//! ```text
//! plugin.toml
//! init.lua
//! lua/<plugin_id>/...lua          # plugin's own modules
//! after/plugin/...lua             # post-init bootstrap files
//! assets/                         # plugin assets (icons etc)
//! doc/                            # plugin docs
//! tests/                          # plugin test sources
//! migrations/                     # persist migrations
//! ```
//!
//! The host is the only thing that touches the filesystem — plugins
//! interact with this layout through `require("plugin_id.module")` and
//! through host-driven `after/plugin/*.lua` execution. Both are
//! implemented in `lua_loader`. This module is the data layer they read
//! from: a registry of `(plugin_id, kind, root)` triples.
//!
//! Two structs:
//!
//! - `RuntimePathRegistry` — host-wide. One entry per loaded plugin
//!   carrying its absolute root directory. Cheap-clone (Rc + RefCell).
//!   Updated on `load_plugin` and `unload_plugin`.
//! - `PluginRuntimePath` — per-generation view. Resolved from the
//!   registry at load time using the manifest's `requires_plugins`. The
//!   loader uses this to walk searchers in deterministic order: first
//!   the plugin's own `lua/<plugin_id>/`, then each declared
//!   dependency's `lua/<dep_id>/` in manifest order.
//!
//! The split is deliberate. The registry is the source of truth (where
//! plugins live on disk). The per-plugin view is the access policy
//! (which plugins this plugin may load modules from). The loader only
//! reads the per-plugin view, so an undeclared dependency is invisible
//! even if the registry knows about it.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePathKind {
    /// `lua/<plugin_id>/` directory inside the plugin's own root.
    Own,
    /// `lua/<dep_id>/` directory inside a declared dependency's root.
    Dependency,
}

impl RuntimePathKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Dependency => "dependency",
        }
    }
}

/// One entry in the per-plugin runtime path. `plugin_id` names the
/// owner of the modules (i.e. the prefix the consumer must use in
/// `require("<plugin_id>.module")`). `lua_root` is the absolute path to
/// the directory whose immediate children become module files.
#[derive(Debug, Clone)]
pub struct RuntimePathEntry {
    pub plugin_id: String,
    pub kind: RuntimePathKind,
    pub lua_root: PathBuf,
}

#[derive(Debug, Default)]
struct RegistryInner {
    /// Map plugin_id -> plugin root (the directory containing
    /// `plugin.toml`). Inserted on `load_plugin`, cleared on
    /// `unload_plugin` / failed reloads.
    roots: std::collections::HashMap<String, PathBuf>,
}

/// Cheap-clone host-wide registry of plugin roots. Used by
/// `PluginRuntimePath::resolve` to translate a `requires_plugins`
/// string into an absolute `lua/<dep_id>/` path.
#[derive(Debug, Clone, Default)]
pub struct RuntimePathRegistry {
    inner: Rc<RefCell<RegistryInner>>,
}

impl RuntimePathRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, plugin_id: impl Into<String>, root: impl Into<PathBuf>) {
        self.inner
            .borrow_mut()
            .roots
            .insert(plugin_id.into(), root.into());
    }

    pub fn unregister(&self, plugin_id: &str) {
        self.inner.borrow_mut().roots.remove(plugin_id);
    }

    pub fn root_for(&self, plugin_id: &str) -> Option<PathBuf> {
        self.inner.borrow().roots.get(plugin_id).cloned()
    }
}

/// Per-plugin runtime path. Built once per generation at load time;
/// dropped when the generation is dropped.
#[derive(Debug, Clone)]
pub struct PluginRuntimePath {
    entries: Vec<RuntimePathEntry>,
}

impl PluginRuntimePath {
    /// Resolve a runtime path from the manifest's `requires_plugins`
    /// list. Order is deterministic: own first, then deps in manifest
    /// order. Dependencies that aren't yet loaded are silently dropped
    /// — the loader will surface the missing module as a
    /// `lua.module.not_found` diagnostic if the consumer actually
    /// tries to require something from them. (dependency resolution wires load
    /// ordering so this stops being silent.)
    pub fn resolve(
        plugin_id: &str,
        plugin_root: &Path,
        requires_plugins: &[String],
        registry: &RuntimePathRegistry,
    ) -> Self {
        let mut entries = Vec::with_capacity(1 + requires_plugins.len());
        entries.push(RuntimePathEntry {
            plugin_id: plugin_id.to_string(),
            kind: RuntimePathKind::Own,
            lua_root: plugin_root.join("lua").join(plugin_id),
        });
        for dep_id in requires_plugins {
            if let Some(dep_root) = registry.root_for(dep_id) {
                entries.push(RuntimePathEntry {
                    plugin_id: dep_id.clone(),
                    kind: RuntimePathKind::Dependency,
                    lua_root: dep_root.join("lua").join(dep_id),
                });
            }
        }
        Self { entries }
    }

    pub fn entries(&self) -> &[RuntimePathEntry] {
        &self.entries
    }

    /// Locate the on-disk file for `module_name`. Caller has already
    /// validated the name shape (`Self::validate_module_name`). Returns
    /// the matched entry plus the absolute file path.
    pub fn find(&self, module_name: &str) -> Option<(&RuntimePathEntry, PathBuf)> {
        let (head, tail) = match module_name.split_once('.') {
            Some((h, t)) => (h, Some(t)),
            None => (module_name, None),
        };
        for entry in &self.entries {
            if entry.plugin_id != head {
                continue;
            }
            // `require("plugin_id")` -> `lua/plugin_id/init.lua`
            // `require("plugin_id.foo")` -> `lua/plugin_id/foo.lua`
            //   or `lua/plugin_id/foo/init.lua`
            let candidates: Vec<PathBuf> = match tail {
                None => vec![entry.lua_root.join("init.lua")],
                Some(rest) => {
                    let rel = rest.replace('.', "/");
                    vec![
                        entry.lua_root.join(format!("{rel}.lua")),
                        entry.lua_root.join(&rel).join("init.lua"),
                    ]
                }
            };
            for candidate in candidates {
                if candidate.is_file() {
                    return Some((entry, candidate));
                }
            }
        }
        None
    }

    /// True iff `head_id` (the first dotted segment of a module name)
    /// is one of this plugin's allowed prefixes. Used by the loader to
    /// produce a `lua.module.forbidden` diagnostic when a plugin tries
    /// to reach into another plugin's modules without declaring it as
    /// a `requires_plugins` dependency.
    pub fn allows_prefix(&self, head_id: &str) -> bool {
        self.entries.iter().any(|e| e.plugin_id == head_id)
    }

    /// Validate that `module_name` is a syntactically safe Lua module
    /// name. Returns the canonical "head.tail.tail2" string on success.
    /// Rejects:
    ///   - empty names
    ///   - leading or trailing `.`
    ///   - empty segments (`foo..bar`)
    ///   - segments containing `..`, `/`, `\`, or `:`
    ///   - segments that are exactly `.` or `..`
    pub fn validate_module_name(name: &str) -> Result<&str, ModuleNameError> {
        if name.is_empty() {
            return Err(ModuleNameError::Empty);
        }
        if name.contains(['/', '\\', ':']) {
            return Err(ModuleNameError::IllegalChar);
        }
        for segment in name.split('.') {
            if segment.is_empty() {
                return Err(ModuleNameError::EmptySegment);
            }
            if segment == "." || segment == ".." {
                return Err(ModuleNameError::TraversalSegment);
            }
            if segment.contains("..") {
                return Err(ModuleNameError::IllegalChar);
            }
            if !segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Err(ModuleNameError::IllegalChar);
            }
        }
        Ok(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleNameError {
    Empty,
    EmptySegment,
    TraversalSegment,
    IllegalChar,
}

impl ModuleNameError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "module name is empty",
            Self::EmptySegment => "module name has an empty dotted segment",
            Self::TraversalSegment => "module name contains a path-traversal segment",
            Self::IllegalChar => "module name contains illegal characters",
        }
    }
}

impl std::fmt::Display for ModuleNameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_module_name_accepts_simple_names() {
        assert!(PluginRuntimePath::validate_module_name("foo").is_ok());
        assert!(PluginRuntimePath::validate_module_name("foo.bar").is_ok());
        assert!(PluginRuntimePath::validate_module_name("foo.bar.baz_2").is_ok());
        assert!(PluginRuntimePath::validate_module_name("foo-bar.baz").is_ok());
    }

    #[test]
    fn validate_module_name_rejects_traversal_and_separators() {
        assert!(PluginRuntimePath::validate_module_name("").is_err());
        assert!(PluginRuntimePath::validate_module_name("..").is_err());
        assert!(PluginRuntimePath::validate_module_name(".foo").is_err());
        assert!(PluginRuntimePath::validate_module_name("foo.").is_err());
        assert!(PluginRuntimePath::validate_module_name("foo..bar").is_err());
        assert!(PluginRuntimePath::validate_module_name("../foo").is_err());
        assert!(PluginRuntimePath::validate_module_name("foo/bar").is_err());
        assert!(PluginRuntimePath::validate_module_name("foo\\bar").is_err());
        assert!(PluginRuntimePath::validate_module_name("foo:bar").is_err());
    }

    #[test]
    fn registry_round_trip() {
        let reg = RuntimePathRegistry::new();
        reg.register("zeta", PathBuf::from("/p/zeta"));
        reg.register("alpha", PathBuf::from("/p/alpha"));
        reg.register("mu", PathBuf::from("/p/mu"));
        assert_eq!(reg.root_for("alpha"), Some(PathBuf::from("/p/alpha")));
        assert_eq!(reg.root_for("zeta"), Some(PathBuf::from("/p/zeta")));
        reg.unregister("mu");
        assert!(reg.root_for("mu").is_none());
        assert_eq!(reg.root_for("alpha"), Some(PathBuf::from("/p/alpha")));
    }

    #[test]
    fn resolve_builds_own_then_dependency_entries_in_order() {
        let reg = RuntimePathRegistry::new();
        reg.register("dep_a", PathBuf::from("/plugins/dep_a"));
        reg.register("dep_b", PathBuf::from("/plugins/dep_b"));

        let path = PluginRuntimePath::resolve(
            "consumer",
            Path::new("/plugins/consumer"),
            &["dep_a".into(), "dep_b".into(), "missing".into()],
            &reg,
        );
        let kinds: Vec<_> = path.entries().iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                RuntimePathKind::Own,
                RuntimePathKind::Dependency,
                RuntimePathKind::Dependency
            ]
        );
        let ids: Vec<_> = path
            .entries()
            .iter()
            .map(|e| e.plugin_id.as_str())
            .collect();
        assert_eq!(ids, vec!["consumer", "dep_a", "dep_b"]);
    }

    #[test]
    fn allows_prefix_only_for_own_or_declared_deps() {
        let reg = RuntimePathRegistry::new();
        reg.register("dep_a", PathBuf::from("/plugins/dep_a"));
        let path = PluginRuntimePath::resolve(
            "consumer",
            Path::new("/plugins/consumer"),
            &["dep_a".into()],
            &reg,
        );
        assert!(path.allows_prefix("consumer"));
        assert!(path.allows_prefix("dep_a"));
        assert!(!path.allows_prefix("other"));
    }
}

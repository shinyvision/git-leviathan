use crate::api_version::ApiVersion;
use crate::capability::Capability;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: Version,
    pub api_version: ApiVersion,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub provides_services: Vec<ServiceDecl>,
    #[serde(default)]
    pub consumes_services: Vec<ServiceDecl>,
    #[serde(default)]
    pub dependencies: HashMap<String, VersionReq>,
    /// Optional plugin dependencies. Same shape as
    /// `dependencies` (`id -> VersionReq`); resolution treats a missing
    /// optional dep as a warning rather than a hard failure. Consumers
    /// must tolerate the dependency being absent at runtime.
    #[serde(default)]
    pub optional_dependencies: HashMap<String, VersionReq>,
    /// Other plugin ids whose `lua/<id>/` modules this plugin may
    /// `require`. Empty by default — a plugin can only `require` its
    /// own modules unless it explicitly names dependencies here. The
    /// runtime enforces the per-`require` filesystem boundary, and
    /// dependency resolution uses these declarations for load ordering.
    #[serde(default)]
    pub requires_plugins: Vec<String>,
    /// Optional `[runtime]` section. Currently controls strict-globals
    /// enforcement for the plugin's Lua state.
    #[serde(default)]
    pub runtime: PluginRuntimeConfig,
    /// Optional `[activation]` section. When present, the
    /// host installs lazy stubs for the declared triggers and only
    /// spins up the plugin's Lua state on first match. When absent,
    /// the plugin loads eagerly as before.
    #[serde(default)]
    pub activation: Option<PluginActivation>,
}

/// lazy loading activation triggers. An empty section (`[activation]`
/// with no fields set) marks the plugin as manual-activation only —
/// equivalent to `manual = true`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PluginActivation {
    /// Canonical event names the lazy plugin should activate on. The
    /// host validates these against `events::CanonicalEvent` at
    /// install time; unknown names emit `activation.unknown_event`
    /// and the trigger is dropped.
    #[serde(default)]
    pub events: Vec<String>,
    /// Command ids the lazy plugin will register on activation. The
    /// host installs a stub for each so palette / keymap dispatch
    /// activates the plugin and re-dispatches once it's live. After
    /// activation the plugin must register the same command names —
    /// any missing one becomes `activation.unknown_command`.
    #[serde(default)]
    pub commands: Vec<String>,
    /// `(context, key)` keymap stubs.
    #[serde(default)]
    pub keymaps: Vec<ActivationKeymap>,
    /// Region keys (`main_bar`, `tab_bar`, `repository`, …) whose
    /// first paint triggers activation.
    #[serde(default)]
    pub regions: Vec<String>,
    /// Boolean predicates over the active repository.
    #[serde(default)]
    pub repository_shape: Option<RepositoryShapePredicate>,
    /// Relative paths whose presence in the active repository's
    /// working tree triggers activation.
    #[serde(default)]
    pub files: Vec<String>,
    /// Activate only on explicit user action (devtools, palette,
    /// `host.activate_plugin_for_command(<plugin>:activate)`).
    #[serde(default)]
    pub manual: bool,
}

impl PluginActivation {
    /// True when the manifest has no usable triggers — empty events /
    /// commands / keymaps / regions / files and no repository shape
    /// predicate. The host treats this as manual-only.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
            && self.commands.is_empty()
            && self.keymaps.is_empty()
            && self.regions.is_empty()
            && self.files.is_empty()
            && self.repository_shape.is_none()
            && !self.manual
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActivationKeymap {
    pub context: String,
    pub key: String,
}

/// Boolean predicates over the active repository. All set fields
/// must be true for the trigger to fire; an absent field is
/// permissive.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RepositoryShapePredicate {
    /// When `Some(true)`, the active repository must declare at
    /// least one remote. When `Some(false)`, it must declare none.
    #[serde(default)]
    pub has_remote: Option<bool>,
    /// When `Some(branch)`, the active repository's current branch
    /// must match this exact name.
    #[serde(default)]
    pub has_branch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginRuntimeConfig {
    /// When `true` (the default), reading or writing an undeclared Lua
    /// global from plugin code raises an error and emits a structured
    /// diagnostic. Setting this to `false` is the documented escape
    /// hatch for plugins that intentionally use `_G`.
    #[serde(default = "default_strict_globals")]
    pub strict_globals: bool,
}

impl Default for PluginRuntimeConfig {
    fn default() -> Self {
        Self {
            strict_globals: default_strict_globals(),
        }
    }
}

fn default_strict_globals() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDecl {
    pub name: String,
    pub version: u32,
    /// `true` for load-order dependencies. Providers ignore this flag;
    /// consumers use it to distinguish hard dependencies from optional
    /// integrations.
    pub required: bool,
}

impl<'de> Deserialize<'de> for ServiceDecl {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawServiceDecl {
            String(String),
            Table {
                service: Option<String>,
                name: Option<String>,
                version: Option<u32>,
                #[serde(default)]
                required: Option<bool>,
                #[serde(default)]
                optional: Option<bool>,
            },
        }

        match RawServiceDecl::deserialize(d)? {
            RawServiceDecl::String(s) => {
                parse_service_decl(&s, true).map_err(serde::de::Error::custom)
            }
            RawServiceDecl::Table {
                service,
                name,
                version,
                required,
                optional,
            } => {
                let required = required.unwrap_or(!optional.unwrap_or(false));
                if let Some(service) = service {
                    parse_service_decl(&service, required).map_err(serde::de::Error::custom)
                } else {
                    let name = name.ok_or_else(|| {
                        serde::de::Error::custom("service decl table needs `service` or `name`")
                    })?;
                    let version = version.ok_or_else(|| {
                        serde::de::Error::custom("service decl table needs `version`")
                    })?;
                    Ok(ServiceDecl {
                        name,
                        version,
                        required,
                    })
                }
            }
        }
    }
}

impl Serialize for ServiceDecl {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.required {
            serializer.serialize_str(&format!("{}@{}", self.name, self.version))
        } else {
            use serde::ser::SerializeStruct;
            let mut state = serializer.serialize_struct("ServiceDecl", 2)?;
            state.serialize_field("service", &format!("{}@{}", self.name, self.version))?;
            state.serialize_field("optional", &true)?;
            state.end()
        }
    }
}

fn parse_service_decl(s: &str, required: bool) -> Result<ServiceDecl, String> {
    let (n, v) = s
        .split_once('@')
        .ok_or_else(|| "service decl must be 'name@version'".to_string())?;
    Ok(ServiceDecl {
        name: n.to_string(),
        version: v.parse().map_err(|e| format!("{e}"))?,
        required,
    })
}

impl ServiceDecl {
    pub fn key(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{Capability, FsScope};

    #[test]
    fn parse_minimal_manifest() {
        let toml = r#"
            id = "demo"
            name = "Demo"
            version = "0.1.0"
            api_version = "1.0"
        "#;
        let m: PluginManifest = toml::from_str(toml).unwrap();
        assert_eq!(m.id, "demo");
        assert!(m.capabilities.is_empty());
    }

    #[test]
    fn parse_full_manifest() {
        let toml = r#"
            id = "git-tools"
            name = "Git Tools"
            version = "1.2.0"
            api_version = "1.0"
            capabilities = ["fs:read", "fs:write:state", "process:spawn", "net:fetch"]
            provides_services = ["diff_viewer@1"]
            consumes_services = ["repository@1"]
            requires_plugins = ["repository_info", "common_utils"]

            [dependencies]
            repository_info = ">=0.5.0"

            [runtime]
            strict_globals = false
        "#;
        let m: PluginManifest = toml::from_str(toml).unwrap();
        assert!(m.capabilities.contains(&Capability::FsWrite {
            scope: FsScope::State
        }));
        assert_eq!(m.provides_services.len(), 1);
        assert_eq!(
            m.requires_plugins,
            vec!["repository_info".to_string(), "common_utils".to_string()]
        );
        assert!(!m.runtime.strict_globals);
    }

    #[test]
    fn parse_activation_section() {
        let toml = r#"
            id = "lazy"
            name = "Lazy"
            version = "0.1.0"
            api_version = "1.0"

            [activation]
            events = ["FetchStarted"]
            commands = ["lazy.go"]
            regions = ["main_bar"]
            files = [".eslintrc"]
            manual = false

            [[activation.keymaps]]
            context = "global"
            key = "<leader>x"

            [activation.repository_shape]
            has_remote = true
            has_branch = "main"
        "#;
        let m: PluginManifest = toml::from_str(toml).unwrap();
        let act = m.activation.expect("activation section");
        assert_eq!(act.events, vec!["FetchStarted".to_string()]);
        assert_eq!(act.commands, vec!["lazy.go".to_string()]);
        assert_eq!(act.regions, vec!["main_bar".to_string()]);
        assert_eq!(act.files, vec![".eslintrc".to_string()]);
        assert_eq!(act.keymaps.len(), 1);
        assert_eq!(act.keymaps[0].context, "global");
        assert_eq!(act.keymaps[0].key, "<leader>x");
        let shape = act.repository_shape.expect("repository_shape");
        assert_eq!(shape.has_remote, Some(true));
        assert_eq!(shape.has_branch.as_deref(), Some("main"));
        assert!(!act.manual);
    }

    #[test]
    fn activation_absent_means_eager_load() {
        let toml = r#"
            id = "demo"
            name = "Demo"
            version = "0.1.0"
            api_version = "1.0"
        "#;
        let m: PluginManifest = toml::from_str(toml).unwrap();
        assert!(m.activation.is_none());
    }

    #[test]
    fn strict_globals_default_is_true() {
        let toml = r#"
            id = "demo"
            name = "Demo"
            version = "0.1.0"
            api_version = "1.0"
        "#;
        let m: PluginManifest = toml::from_str(toml).unwrap();
        assert!(m.runtime.strict_globals);
        assert!(m.requires_plugins.is_empty());
    }
}

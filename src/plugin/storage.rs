//! Plugin storage surface layout.
//!
//! persistence and settings splits plugin-owned persistence into explicit surfaces while
//! preserving the v1 default `persist.open(name)` state path.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PluginStorageRoots {
    pub state_root: PathBuf,
    pub config_root: PathBuf,
    pub cache_root: PathBuf,
    pub secrets_root: PathBuf,
}

impl PluginStorageRoots {
    pub fn os_default() -> Self {
        let temp = std::env::temp_dir;
        Self {
            state_root: dirs::state_dir().unwrap_or_else(temp).join("git_leviathan"),
            config_root: dirs::config_dir()
                .unwrap_or_else(temp)
                .join("git_leviathan"),
            cache_root: dirs::cache_dir().unwrap_or_else(temp).join("git_leviathan"),
            secrets_root: dirs::data_local_dir()
                .or_else(dirs::data_dir)
                .unwrap_or_else(temp)
                .join("git_leviathan")
                .join("plugin_secrets"),
        }
    }

    pub fn under_base(base: impl AsRef<Path>) -> Self {
        let base = base.as_ref();
        Self {
            state_root: base.join("state"),
            config_root: base.join("config"),
            cache_root: base.join("cache"),
            secrets_root: base.join("secrets"),
        }
    }

    pub fn for_plugin(
        &self,
        plugin_id: &str,
        plugin_root: impl Into<PathBuf>,
    ) -> PluginStoragePaths {
        PluginStoragePaths {
            plugin_root: plugin_root.into(),
            state_dir: self.state_root.join(plugin_id),
            config_dir: self.config_root.join(plugin_id),
            cache_dir: self.cache_root.join(plugin_id),
            secrets_dir: self.secrets_root.join(plugin_id),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PluginStoragePaths {
    pub plugin_root: PathBuf,
    pub state_dir: PathBuf,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub secrets_dir: PathBuf,
}

impl PluginStoragePaths {
    pub fn path_for_store(
        &self,
        surface: StorageSurface,
        name: &str,
        repo: Option<&str>,
    ) -> Result<PathBuf, String> {
        let name = validate_store_name(name)?;
        let dir = match surface {
            StorageSurface::State => self.state_dir.clone(),
            StorageSurface::Config => self.config_dir.clone(),
            StorageSurface::Cache => self.cache_dir.clone(),
            StorageSurface::Repo => {
                let repo = repo
                    .map(sanitize_component)
                    .unwrap_or_else(|| "default".to_string());
                self.state_dir.join("repos").join(repo)
            }
            StorageSurface::Secrets => {
                return Err(
                    "secrets are available through leviathan.secrets, not persist.open".into(),
                )
            }
            StorageSurface::Settings => {
                return Err("settings are available through leviathan.settings".into())
            }
        };
        Ok(dir.join(format!("{name}.json")))
    }

    pub fn settings_path(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    pub fn surface_dir(&self, surface: StorageSurface) -> PathBuf {
        match surface {
            StorageSurface::State => self.state_dir.clone(),
            StorageSurface::Config | StorageSurface::Settings => self.config_dir.clone(),
            StorageSurface::Cache => self.cache_dir.clone(),
            StorageSurface::Repo => self.state_dir.join("repos"),
            StorageSurface::Secrets => self.secrets_dir.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageSurface {
    State,
    Config,
    Repo,
    Cache,
    Settings,
    Secrets,
}

impl StorageSurface {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "state" => Ok(Self::State),
            "config" => Ok(Self::Config),
            "repo" | "per_repo" | "per-repo" => Ok(Self::Repo),
            "cache" => Ok(Self::Cache),
            "settings" => Ok(Self::Settings),
            "secrets" => Ok(Self::Secrets),
            other => Err(format!("unknown plugin storage surface `{other}`")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Config => "config",
            Self::Repo => "repo",
            Self::Cache => "cache",
            Self::Settings => "settings",
            Self::Secrets => "secrets",
        }
    }

    pub const fn devtools_surfaces() -> [Self; 6] {
        [
            Self::State,
            Self::Config,
            Self::Repo,
            Self::Cache,
            Self::Settings,
            Self::Secrets,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct StorageSurfaceMetadata {
    pub plugin_id: String,
    pub surface: String,
    pub path: String,
    pub exists: bool,
    pub file_count: usize,
    pub byte_count: u64,
    pub corrupt_files: Vec<String>,
}

pub fn surface_metadata(
    plugin_id: &str,
    surface: StorageSurface,
    path: &Path,
) -> StorageSurfaceMetadata {
    let mut meta = StorageSurfaceMetadata {
        plugin_id: plugin_id.to_string(),
        surface: surface.as_str().to_string(),
        path: path.display().to_string(),
        exists: path.exists(),
        file_count: 0,
        byte_count: 0,
        corrupt_files: Vec::new(),
    };
    collect_metadata(path, &mut meta);
    meta.corrupt_files.sort();
    meta
}

fn collect_metadata(path: &Path, meta: &mut StorageSurfaceMetadata) {
    let Ok(md) = fs::metadata(path) else {
        return;
    };
    if md.is_file() {
        meta.file_count += 1;
        meta.byte_count += md.len();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            match fs::read_to_string(path) {
                Ok(raw) if serde_json::from_str::<serde_json::Value>(&raw).is_err() => {
                    meta.corrupt_files.push(path.display().to_string());
                }
                Err(_) => meta.corrupt_files.push(path.display().to_string()),
                _ => {}
            }
        }
        return;
    }
    if !md.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        collect_metadata(&entry.path(), meta);
    }
}

pub fn validate_store_name(name: &str) -> Result<&str, String> {
    if name.is_empty() {
        return Err("store name cannot be empty".into());
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(format!("invalid store name `{name}`"));
    }
    Ok(name)
}

pub fn sanitize_component(value: &str) -> String {
    let out: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "default".to_string()
    } else {
        out
    }
}

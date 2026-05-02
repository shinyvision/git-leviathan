//! Capability enforcement for plugin host calls. Each plugin gets a
//! `CapabilityGuard` derived from its manifest; every gated host call
//! consults the guard before doing work.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use git_leviathan_plugin_api::capability::{Capability, FsScope};

#[derive(Debug, Clone)]
pub struct CapabilityGuard {
    granted: HashSet<Capability>,
    plugin_root: PathBuf,
    state_dir: PathBuf,
    config_dir: PathBuf,
    workdir: Option<PathBuf>,
}

impl CapabilityGuard {
    pub fn new(
        granted: Vec<Capability>,
        plugin_root: PathBuf,
        state_dir: PathBuf,
        config_dir: PathBuf,
        workdir: Option<PathBuf>,
    ) -> Self {
        Self {
            granted: granted.into_iter().collect(),
            plugin_root,
            state_dir,
            config_dir,
            workdir,
        }
    }

    pub fn check_fs_read(&self, path: &Path) -> Result<(), String> {
        for cap in &self.granted {
            if let Capability::FsRead { scope } = cap {
                if self.path_in_scope(path, *scope) {
                    return Ok(());
                }
            }
        }
        Err(format!(
            "capability not granted: fs:read for {}",
            path.display()
        ))
    }

    pub fn check_fs_write(&self, path: &Path) -> Result<(), String> {
        for cap in &self.granted {
            if let Capability::FsWrite { scope } = cap {
                if self.path_in_scope(path, *scope) {
                    return Ok(());
                }
            }
        }
        Err(format!(
            "capability not granted: fs:write for {}",
            path.display()
        ))
    }

    pub fn check_env(&self) -> Result<(), String> {
        if self.granted.contains(&Capability::Env) {
            Ok(())
        } else {
            Err("capability not granted: env".into())
        }
    }

    pub fn plugin_root(&self) -> &Path {
        &self.plugin_root
    }

    fn path_in_scope(&self, path: &Path, scope: FsScope) -> bool {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.plugin_root.join(path)
        };
        let canon = resolved.canonicalize().unwrap_or(resolved);
        match scope {
            FsScope::Plugin => canon.starts_with(&self.plugin_root),
            FsScope::State => canon.starts_with(&self.state_dir),
            FsScope::Config => canon.starts_with(&self.config_dir),
            FsScope::Workdir => self
                .workdir
                .as_ref()
                .is_some_and(|w| canon.starts_with(w)),
            FsScope::Any => true,
        }
    }
}

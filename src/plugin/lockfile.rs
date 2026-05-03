//! Plugin lockfile.
//!
//! `plugins.lock` is a small TOML document that pins each loaded
//! plugin's resolved version + checksum so subsequent runs detect
//! drift. Format:
//!
//! ```toml
//! [[plugin]]
//! id       = "commit_lens"
//! version  = "2.4.1"
//! source   = "local"
//! checksum = "sha256:..."
//! ```
//!
//! A sibling `plugins.lock.local` file uses the same format and, when
//! present, overrides individual entries from `plugins.lock`.
//!
//! The checksum is computed over `plugin.toml`, `init.lua`, and every
//! file under `lua/**` in the plugin directory. Files are processed in
//! sorted relative-path order so the output is stable across runs and
//! filesystems.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const LOCKFILE_NAME: &str = "plugins.lock";
pub const LOCAL_OVERRIDE_NAME: &str = "plugins.lock.local";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedPlugin {
    pub id: String,
    pub version: String,
    /// Origin of the plugin: `"local"`, `"path"`, or `"registry"`.
    pub source: String,
    /// `sha256:...` digest of the plugin's content (see module docs).
    pub checksum: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Lockfile {
    /// Lockfile entries serialise as `[[plugin]]` tables. Keep this
    /// vec sorted by id when written.
    #[serde(default, rename = "plugin")]
    pub plugins: Vec<LockedPlugin>,
}

impl Lockfile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_str(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    pub fn to_string(&self) -> Result<String, toml::ser::Error> {
        let mut sorted = self.clone();
        sorted.plugins.sort_by(|a, b| a.id.cmp(&b.id));
        toml::to_string(&sorted)
    }

    pub fn read(path: &Path) -> Result<Self, LockfileError> {
        let raw = fs::read_to_string(path).map_err(LockfileError::Io)?;
        Self::from_str(&raw).map_err(LockfileError::Parse)
    }

    pub fn write(&self, path: &Path) -> Result<(), LockfileError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(LockfileError::Io)?;
        }
        let body = self.to_string().map_err(LockfileError::Encode)?;
        fs::write(path, body).map_err(LockfileError::Io)
    }

    pub fn lookup(&self, id: &str) -> Option<&LockedPlugin> {
        self.plugins.iter().find(|p| p.id == id)
    }

    /// Apply a local-override lockfile on top of `self`. Entries in
    /// `overlay` replace matching ids; new ids are appended.
    pub fn apply_overlay(&mut self, overlay: &Lockfile) {
        let mut by_id: BTreeMap<String, LockedPlugin> = BTreeMap::new();
        for p in self.plugins.drain(..) {
            by_id.insert(p.id.clone(), p);
        }
        for p in &overlay.plugins {
            by_id.insert(p.id.clone(), p.clone());
        }
        self.plugins = by_id.into_values().collect();
    }
}

#[derive(Debug)]
pub enum LockfileError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Encode(toml::ser::Error),
}

impl std::fmt::Display for LockfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "lockfile io: {e}"),
            Self::Parse(e) => write!(f, "lockfile parse: {e}"),
            Self::Encode(e) => write!(f, "lockfile encode: {e}"),
        }
    }
}

impl std::error::Error for LockfileError {}

/// Compute the sha256 digest over a plugin's content. Returns a
/// `sha256:<hex>` string. Walks `plugin.toml`, `init.lua`, and every
/// regular file under `lua/`. Files are mixed into the hash in sorted
/// relative-path order; each file contributes `"<rel-path>\0<bytes>\0"`
/// so reorderings or empty-file appends can't collide.
pub fn compute_plugin_checksum(plugin_dir: &Path) -> std::io::Result<String> {
    let mut entries: Vec<(String, PathBuf)> = Vec::new();

    for top in ["plugin.toml", "init.lua"] {
        let p = plugin_dir.join(top);
        if p.is_file() {
            entries.push((top.to_string(), p));
        }
    }

    let lua_dir = plugin_dir.join("lua");
    if lua_dir.is_dir() {
        walk_collect(&lua_dir, &lua_dir, &mut entries)?;
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (rel, abs) in &entries {
        let contents = fs::read(abs)?;
        hasher.update(rel.as_bytes());
        hasher.update([0u8]);
        hasher.update(&contents);
        hasher.update([0u8]);
    }
    let digest = hasher.finalize();
    Ok(format!("sha256:{}", hex_lower(&digest)))
}

fn walk_collect(base: &Path, cur: &Path, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    let mut children: Vec<PathBuf> = fs::read_dir(cur)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            walk_collect(base, &child, out)?;
        } else if child.is_file() {
            let rel = child
                .strip_prefix(base)
                .map(|p| format!("lua/{}", p.to_string_lossy().replace('\\', "/")))
                .unwrap_or_else(|_| child.to_string_lossy().to_string());
            out.push((rel, child));
        }
    }
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let lock = Lockfile {
            plugins: vec![
                LockedPlugin {
                    id: "z".into(),
                    version: "0.1.0".into(),
                    source: "local".into(),
                    checksum: "sha256:abc".into(),
                },
                LockedPlugin {
                    id: "a".into(),
                    version: "0.2.0".into(),
                    source: "local".into(),
                    checksum: "sha256:def".into(),
                },
            ],
        };
        let out = lock.to_string().expect("encode");
        // Sorted by id.
        let z_pos = out.find("z").unwrap();
        let a_pos = out.find("0.2.0").unwrap();
        assert!(a_pos < z_pos, "expected a before z, got: {out}");
        let parsed = Lockfile::from_str(&out).expect("decode");
        assert_eq!(parsed.plugins.len(), 2);
        assert_eq!(parsed.lookup("a").unwrap().version, "0.2.0");
    }

    #[test]
    fn checksum_is_stable_for_same_content() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("plugin.toml"), "id = \"x\"\n").unwrap();
        std::fs::write(dir.join("init.lua"), "-- hi").unwrap();
        std::fs::create_dir_all(dir.join("lua/x")).unwrap();
        std::fs::write(dir.join("lua/x/util.lua"), "return 1").unwrap();
        let h1 = compute_plugin_checksum(dir).unwrap();
        let h2 = compute_plugin_checksum(dir).unwrap();
        assert_eq!(h1, h2);
        assert!(h1.starts_with("sha256:"));
    }

    #[test]
    fn checksum_changes_with_content() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("plugin.toml"), "id = \"x\"\n").unwrap();
        std::fs::write(dir.join("init.lua"), "v1").unwrap();
        let h1 = compute_plugin_checksum(dir).unwrap();
        std::fs::write(dir.join("init.lua"), "updated").unwrap();
        let h2 = compute_plugin_checksum(dir).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn overlay_replaces_entries() {
        let mut lock = Lockfile {
            plugins: vec![
                LockedPlugin {
                    id: "a".into(),
                    version: "0.1.0".into(),
                    source: "local".into(),
                    checksum: "sha256:1".into(),
                },
                LockedPlugin {
                    id: "b".into(),
                    version: "0.1.0".into(),
                    source: "local".into(),
                    checksum: "sha256:2".into(),
                },
            ],
        };
        let overlay = Lockfile {
            plugins: vec![LockedPlugin {
                id: "a".into(),
                version: "0.2.0".into(),
                source: "path".into(),
                checksum: "sha256:9".into(),
            }],
        };
        lock.apply_overlay(&overlay);
        assert_eq!(lock.lookup("a").unwrap().version, "0.2.0");
        assert_eq!(lock.lookup("a").unwrap().source, "path");
        assert_eq!(lock.lookup("b").unwrap().version, "0.1.0");
    }
}

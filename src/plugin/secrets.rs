//! Plugin secret store abstraction.
//!
//! The current backend is a host-owned file under the secrets surface.
//! It is intentionally separate from plugin state/config/cache files so
//! the storage browser can prove secret values do not leak into plain
//! plugin persistence. A platform keychain backend can implement the
//! same trait later.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub trait SecretBackend {
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn delete(&self, key: &str) -> Result<(), String>;
    fn list(&self) -> Result<Vec<String>, String>;
}

#[derive(Debug, Clone)]
pub struct FileSecretStore {
    path: PathBuf,
}

impl FileSecretStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            path: dir.as_ref().join("secrets.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl SecretBackend for FileSecretStore {
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        validate_key(key)?;
        let mut file = read_secret_file(&self.path)?;
        file.values.insert(key.to_string(), value.to_string());
        write_secret_file(&self.path, &file)
    }

    fn get(&self, key: &str) -> Result<Option<String>, String> {
        validate_key(key)?;
        Ok(read_secret_file(&self.path)?.values.get(key).cloned())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        validate_key(key)?;
        let mut file = read_secret_file(&self.path)?;
        file.values.remove(key);
        write_secret_file(&self.path, &file)
    }

    fn list(&self) -> Result<Vec<String>, String> {
        let mut keys: Vec<String> = read_secret_file(&self.path)?
            .values
            .keys()
            .cloned()
            .collect();
        keys.sort();
        Ok(keys)
    }
}

#[derive(Debug, Clone)]
pub struct SecretMetadata {
    pub path: String,
    pub key_count: usize,
    pub keys: Vec<String>,
}

pub fn metadata(dir: &Path) -> SecretMetadata {
    let store = FileSecretStore::new(dir);
    let keys = store.list().unwrap_or_default();
    SecretMetadata {
        path: store.path().display().to_string(),
        key_count: keys.len(),
        keys,
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SecretFile {
    #[serde(default)]
    values: BTreeMap<String, String>,
}

fn read_secret_file(path: &Path) -> Result<SecretFile, String> {
    if !path.exists() {
        return Ok(SecretFile::default());
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    match serde_json::from_str::<SecretFile>(&raw) {
        Ok(file) => Ok(file),
        Err(_) => {
            let backup = crate::plugin::persist::next_corrupt_backup_path(path);
            fs::rename(path, backup).map_err(|e| e.to_string())?;
            Ok(SecretFile::default())
        }
    }
}

fn write_secret_file(path: &Path, file: &SecretFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}

fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.contains('/') || key.contains('\\') {
        Err(format!("invalid secret key `{key}`"))
    } else {
        Ok(())
    }
}

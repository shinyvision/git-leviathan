//! Plugin-level persistent KV. Each plugin opens named stores; each store
//! is a JSON file in the plugin's `state_dir` containing `{ version,
//! data }`. Migrations transform the `data` object atomically when the
//! file's `version` doesn't match the requested `version`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug)]
pub enum PersistError {
    Io(io::Error),
    Json(serde_json::Error),
    NoMigration { from: u32, to: u32 },
    MigrationFailed { from: u32, to: u32, message: String },
}

impl std::fmt::Display for PersistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Json(e) => write!(f, "json: {e}"),
            Self::NoMigration { from, to } => {
                write!(f, "no migration from version {from} to {to}")
            }
            Self::MigrationFailed { from, to, message } => {
                write!(f, "migration {from}->{to} failed: {message}")
            }
        }
    }
}

impl std::error::Error for PersistError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for PersistError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for PersistError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

pub type MigrationFn = Box<dyn Fn(Value) -> Value + Send + Sync>;

pub struct Migration {
    pub from: u32,
    pub to: u32,
    pub transform: MigrationFn,
}

impl Migration {
    pub fn new<F: Fn(Value) -> Value + Send + Sync + 'static>(from: u32, to: u32, f: F) -> Self {
        Self {
            from,
            to,
            transform: Box::new(f),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreFile {
    pub version: u32,
    pub data: Value,
}

pub struct PersistStore {
    path: PathBuf,
    version: u32,
    data: Value,
}

impl PersistStore {
    pub fn open(
        path: impl AsRef<Path>,
        target_version: u32,
        migrations: &[Migration],
    ) -> Result<Self, PersistError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = load_store_file_or_default(&path, target_version)?;

        while file.version < target_version {
            let m = migrations.iter().find(|m| m.from == file.version).ok_or(
                PersistError::NoMigration {
                    from: file.version,
                    to: target_version,
                },
            )?;
            let migrated = (m.transform)(std::mem::take(&mut file.data));
            file.data = migrated;
            file.version = m.to;
        }
        if file.version != target_version {
            return Err(PersistError::NoMigration {
                from: file.version,
                to: target_version,
            });
        }

        write_store_file_atomic(&path, &file)?;

        Ok(Self {
            path,
            version: file.version,
            data: file.data,
        })
    }

    /// Construct a `PersistStore` from already-loaded state. Used by the
    /// Lua surface where migrations run inline (calling Lua functions)
    /// and the resulting state is then handed to this constructor.
    pub fn from_loaded(path: PathBuf, version: u32, data: Value) -> Self {
        Self {
            path,
            version,
            data,
        }
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn get_value(&self, key: &str) -> Option<Value> {
        self.data.get(key).cloned()
    }

    pub fn set_value(&mut self, key: &str, value: Value) -> Result<(), PersistError> {
        if let Value::Object(map) = &mut self.data {
            map.insert(key.to_string(), value);
        } else {
            self.data = json!({ key: value });
        }
        let file = StoreFile {
            version: self.version,
            data: self.data.clone(),
        };
        write_store_file_atomic(&self.path, &file)?;
        Ok(())
    }

    pub fn delete_value(&mut self, key: &str) -> Result<(), PersistError> {
        if let Value::Object(map) = &mut self.data {
            map.remove(key);
        }
        let file = StoreFile {
            version: self.version,
            data: self.data.clone(),
        };
        write_store_file_atomic(&self.path, &file)?;
        Ok(())
    }
}

pub fn load_store_file_or_default(
    path: &Path,
    target_version: u32,
) -> Result<StoreFile, PersistError> {
    if !path.exists() {
        return Ok(StoreFile {
            version: target_version,
            data: json!({}),
        });
    }
    let raw = fs::read_to_string(path)?;
    match serde_json::from_str::<StoreFile>(&raw) {
        Ok(file) => Ok(file),
        Err(_) => {
            backup_corrupt_file(path)?;
            Ok(StoreFile {
                version: target_version,
                data: json!({}),
            })
        }
    }
}

pub fn write_store_file_atomic(path: &Path, file: &StoreFile) -> Result<(), PersistError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(file)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw)?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn backup_corrupt_file(path: &Path) -> Result<(), PersistError> {
    let backup = next_corrupt_backup_path(path);
    fs::rename(path, backup)?;
    Ok(())
}

fn next_corrupt_backup_path(path: &Path) -> PathBuf {
    for idx in 0..1000 {
        let candidate = if idx == 0 {
            path.with_extension("json.corrupt")
        } else {
            path.with_extension(format!("json.corrupt.{idx}"))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    path.with_extension("json.corrupt.last")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn opens_empty_creates_default() {
        let dir = tempdir().unwrap();
        let store = PersistStore::open(dir.path().join("state.json"), 1, &[]).unwrap();
        assert_eq!(store.version(), 1);
        assert!(store.get_value("missing").is_none());
    }

    #[test]
    fn set_then_get_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut store = PersistStore::open(&path, 1, &[]).unwrap();
        store.set_value("n", json!(42)).unwrap();
        assert_eq!(store.get_value("n"), Some(json!(42)));

        let store2 = PersistStore::open(&path, 1, &[]).unwrap();
        assert_eq!(store2.get_value("n"), Some(json!(42)));
    }

    #[test]
    fn v1_to_v2_migration_runs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, r#"{"version":1,"data":{"count":5}}"#).unwrap();

        let store = PersistStore::open(
            &path,
            2,
            &[Migration::new(1, 2, |old: serde_json::Value| {
                json!({ "n": old.get("count").cloned().unwrap_or(json!(0)) })
            })],
        )
        .unwrap();
        assert_eq!(store.version(), 2);
        assert_eq!(store.get_value("n"), Some(json!(5)));
    }

    #[test]
    fn missing_migration_path_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, r#"{"version":1,"data":{}}"#).unwrap();
        let r = PersistStore::open(&path, 3, &[]);
        assert!(r.is_err(), "no migration from 1->3 should error");
    }
}

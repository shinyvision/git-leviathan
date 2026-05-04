use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::plugin::slots::SlotAddress;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributionOverrideSummary {
    pub plugin_id: String,
    pub region: String,
    pub container: String,
    pub id: String,
    pub hidden: bool,
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributionKey {
    pub plugin_id: String,
    pub region: String,
    pub container: String,
    pub id: String,
}

impl ContributionKey {
    pub fn new(
        plugin_id: impl Into<String>,
        region: impl Into<String>,
        container: impl Into<String>,
        id: impl Into<String>,
    ) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            region: region.into(),
            container: container.into(),
            id: id.into(),
        }
    }

    pub fn from_address(address: &SlotAddress) -> Self {
        Self::new(
            address.plugin_id().as_str(),
            address.region().as_str(),
            address.container().key(),
            address.slot_id().as_str(),
        )
    }

    fn handle(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.plugin_id, self.region, self.container, self.id
        )
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct OverrideFile {
    version: u32,
    hidden: BTreeSet<String>,
    priorities: BTreeMap<String, i32>,
}

#[derive(Debug, Default)]
struct OverrideInner {
    file: OverrideFile,
    persist_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ContributionOverrides {
    inner: Rc<RefCell<OverrideInner>>,
}

impl Default for ContributionOverrides {
    fn default() -> Self {
        Self::new()
    }
}

impl ContributionOverrides {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(OverrideInner::default())),
        }
    }

    pub fn set_persist_path(&self, path: PathBuf) {
        let mut inner = self.inner.borrow_mut();
        inner.persist_path = Some(path.clone());
        inner.file = load_overrides(&path).unwrap_or_default();
    }

    pub fn is_hidden(&self, address: &SlotAddress) -> bool {
        let key = ContributionKey::from_address(address).handle();
        self.inner.borrow().file.hidden.contains(&key)
    }

    pub fn priority(&self, address: &SlotAddress) -> Option<i32> {
        let key = ContributionKey::from_address(address).handle();
        self.inner.borrow().file.priorities.get(&key).copied()
    }

    pub fn set_hidden(&self, key: &ContributionKey, hidden: bool) {
        let mut inner = self.inner.borrow_mut();
        if hidden {
            inner.file.hidden.insert(key.handle());
        } else {
            inner.file.hidden.remove(&key.handle());
        }
        save_inner(&inner);
    }

    pub fn toggle_hidden(&self, key: &ContributionKey) -> bool {
        let mut inner = self.inner.borrow_mut();
        let handle = key.handle();
        let hidden = if inner.file.hidden.contains(&handle) {
            inner.file.hidden.remove(&handle);
            false
        } else {
            inner.file.hidden.insert(handle);
            true
        };
        save_inner(&inner);
        hidden
    }

    pub fn set_priority(&self, key: &ContributionKey, priority: i32) {
        let mut inner = self.inner.borrow_mut();
        inner.file.priorities.insert(key.handle(), priority);
        save_inner(&inner);
    }

    pub fn reset(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.file = OverrideFile::default();
        save_inner(&inner);
    }

    pub fn inspect(&self, key: &ContributionKey) -> ContributionOverrideSummary {
        let inner = self.inner.borrow();
        let handle = key.handle();
        ContributionOverrideSummary {
            plugin_id: key.plugin_id.clone(),
            region: key.region.clone(),
            container: key.container.clone(),
            id: key.id.clone(),
            hidden: inner.file.hidden.contains(&handle),
            priority: inner.file.priorities.get(&handle).copied(),
        }
    }

    pub fn summaries(&self) -> Vec<ContributionOverrideSummary> {
        let inner = self.inner.borrow();
        let mut keys = BTreeSet::new();
        keys.extend(inner.file.hidden.iter().cloned());
        keys.extend(inner.file.priorities.keys().cloned());
        keys.into_iter()
            .filter_map(|handle| {
                let key = parse_handle(&handle)?;
                Some(ContributionOverrideSummary {
                    hidden: inner.file.hidden.contains(&handle),
                    priority: inner.file.priorities.get(&handle).copied(),
                    plugin_id: key.plugin_id,
                    region: key.region,
                    container: key.container,
                    id: key.id,
                })
            })
            .collect()
    }
}

fn parse_handle(handle: &str) -> Option<ContributionKey> {
    let mut parts = handle.splitn(4, ':');
    Some(ContributionKey {
        plugin_id: parts.next()?.to_string(),
        region: parts.next()?.to_string(),
        container: parts.next()?.to_string(),
        id: parts.next()?.to_string(),
    })
}

fn load_overrides(path: &PathBuf) -> Result<OverrideFile, String> {
    if !path.exists() {
        return Ok(OverrideFile::default());
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

fn save_inner(inner: &OverrideInner) {
    let Some(path) = &inner.persist_path else {
        return;
    };
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let file = OverrideFile {
        version: 1,
        hidden: inner.file.hidden.clone(),
        priorities: inner.file.priorities.clone(),
    };
    let Ok(raw) = serde_json::to_string_pretty(&file) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, raw).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn key() -> ContributionKey {
        ContributionKey::new("p", "main_bar", "left", "slot")
    }

    #[test]
    fn overrides_roundtrip_separate_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("contribution_overrides.json");
        let overrides = ContributionOverrides::new();
        overrides.set_persist_path(path.clone());
        overrides.set_hidden(&key(), true);
        overrides.set_priority(&key(), 42);

        let loaded = ContributionOverrides::new();
        loaded.set_persist_path(path);
        let summary = loaded.inspect(&key());
        assert!(summary.hidden);
        assert_eq!(summary.priority, Some(42));
    }

    #[test]
    fn reset_clears_every_override() {
        let overrides = ContributionOverrides::new();
        overrides.set_hidden(&key(), true);
        overrides.set_priority(&key(), 5);
        overrides.reset();
        assert_eq!(
            overrides.inspect(&key()),
            ContributionOverrideSummary {
                plugin_id: "p".into(),
                region: "main_bar".into(),
                container: "left".into(),
                id: "slot".into(),
                hidden: false,
                priority: None,
            }
        );
    }
}

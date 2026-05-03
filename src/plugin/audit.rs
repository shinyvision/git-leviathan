//! Capability audit log. Each `CapabilityGuard` may have an attached
//! `AuditLog` to record allow/deny events. Used by the devtools panel
//! (reload transactions) and tests.

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditOutcome {
    Allowed,
    Denied,
}

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub plugin_id: String,
    pub capability: String,
    pub target: String,
    pub outcome: AuditOutcome,
    pub timestamp: std::time::SystemTime,
}

#[derive(Debug, Clone, Default)]
pub struct AuditLog {
    inner: Arc<Mutex<Vec<AuditEntry>>>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(
        &self,
        plugin_id: &str,
        capability: impl Into<String>,
        target: impl Into<String>,
        outcome: AuditOutcome,
    ) {
        let entry = AuditEntry {
            plugin_id: plugin_id.to_string(),
            capability: capability.into(),
            target: target.into(),
            outcome,
            timestamp: std::time::SystemTime::now(),
        };
        if let Ok(mut g) = self.inner.lock() {
            g.push(entry);
        }
    }

    pub fn entries(&self) -> Vec<AuditEntry> {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::capabilities::CapabilityGuard;
    use crate::plugin::capability_grants::{DecidedBy, Decision, GrantStore};
    use std::path::{Path, PathBuf};

    fn dirs() -> (PathBuf, PathBuf, PathBuf) {
        let t = std::env::temp_dir();
        (
            t.join("plugin_root_test"),
            t.join("state_test"),
            t.join("config_test"),
        )
    }

    fn build(
        plugin_id: &str,
        plugin_version: &str,
        requested: Vec<git_leviathan_plugin_api::capability::Capability>,
        store: GrantStore,
        log: AuditLog,
    ) -> CapabilityGuard {
        let (root, state, config) = dirs();
        std::fs::create_dir_all(&root).ok();
        CapabilityGuard::new(
            plugin_id,
            plugin_version,
            requested,
            root,
            state,
            config,
            None,
            store,
        )
        .with_audit(log)
    }

    #[test]
    fn audit_records_denial() {
        let log = AuditLog::new();
        let store = GrantStore::new_in_memory();
        let guard = build("test_plugin", "0.1.0", vec![], store, log.clone());
        let _ = guard.check_fs_read(Path::new("/etc/passwd"));
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].plugin_id, "test_plugin");
        assert_eq!(entries[0].outcome, AuditOutcome::Denied);
        assert_eq!(entries[0].capability, "fs:read");
    }

    #[test]
    fn audit_records_allowance() {
        use git_leviathan_plugin_api::capability::{Capability, FsScope};
        let log = AuditLog::new();
        let store = GrantStore::new_in_memory();
        store
            .record_decision(
                "p",
                "0.1.0",
                "fs:read:plugin",
                Decision::Allow,
                DecidedBy::Default,
                None,
            )
            .unwrap();
        let (root, _, _) = dirs();
        std::fs::create_dir_all(&root).ok();
        let guard = build(
            "p",
            "0.1.0",
            vec![Capability::FsRead {
                scope: FsScope::Plugin,
            }],
            store,
            log.clone(),
        );
        let _ = guard.check_fs_read(&root.join("anything.txt"));
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].outcome, AuditOutcome::Allowed);
    }

    #[test]
    fn audit_off_when_not_attached() {
        let log = AuditLog::new();
        let store = GrantStore::new_in_memory();
        let (root, state, config) = dirs();
        let guard = CapabilityGuard::new("p", "0.1.0", vec![], root, state, config, None, store);
        let _ = guard.check_fs_read(Path::new("/x"));
        assert!(
            log.entries().is_empty(),
            "log should be empty when no audit attached"
        );
    }
}

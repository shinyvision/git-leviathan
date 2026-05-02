//! Capability audit log. Each `CapabilityGuard` may have an attached
//! `AuditLog` to record allow/deny events. Used by the devtools panel
//! (Phase 6) and tests.

#![allow(dead_code)] // entries/clear/fields are read by tests and Phase 6 devtools panel.

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditOutcome { Allowed, Denied }

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
    pub fn new() -> Self { Self::default() }

    pub fn record(&self, plugin_id: &str, capability: impl Into<String>, target: impl Into<String>, outcome: AuditOutcome) {
        let entry = AuditEntry {
            plugin_id: plugin_id.to_string(),
            capability: capability.into(),
            target: target.into(),
            outcome,
            timestamp: std::time::SystemTime::now(),
        };
        if let Ok(mut g) = self.inner.lock() { g.push(entry); }
    }

    pub fn entries(&self) -> Vec<AuditEntry> {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() { g.clear(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::capabilities::CapabilityGuard;
    use std::path::{Path, PathBuf};

    fn dirs() -> (PathBuf, PathBuf, PathBuf) {
        let t = std::env::temp_dir();
        (t.join("plugin_root_test"), t.join("state_test"), t.join("config_test"))
    }

    #[test]
    fn audit_records_denial() {
        let log = AuditLog::new();
        let (root, state, config) = dirs();
        let guard = CapabilityGuard::new(vec![], root, state, config, None)
            .with_audit(log.clone(), "test_plugin".into());
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
        let (root, state, config) = dirs();
        std::fs::create_dir_all(&root).ok();
        let guard = CapabilityGuard::new(
            vec![Capability::FsRead { scope: FsScope::Plugin }],
            root.clone(), state, config, None,
        )
        .with_audit(log.clone(), "p".into());
        let _ = guard.check_fs_read(&root.join("anything.txt"));
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].outcome, AuditOutcome::Allowed);
    }

    #[test]
    fn audit_off_when_not_attached() {
        let log = AuditLog::new();
        let (root, state, config) = dirs();
        let guard = CapabilityGuard::new(vec![], root, state, config, None);
        let _ = guard.check_fs_read(Path::new("/x"));
        assert!(log.entries().is_empty(), "log should be empty when no audit attached");
    }
}

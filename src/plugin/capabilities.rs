//! Capability enforcement for plugin host calls.
//!
//! Phase 10: every check now routes through [`GrantStore`]. The
//! `CapabilityGuard` is a per-plugin handle carrying the manifest's
//! requested set, the keys the grant store uses (`plugin_id +
//! plugin_version`), and the resolution dirs needed for fs scope
//! checks. It is the only thing the API surface holds — every gated
//! call calls one of `check_fs_read`, `check_fs_write`, `check_env`,
//! or [`CapabilityGuard::check_named`].

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use git_leviathan_plugin_api::capability::Capability;

use crate::plugin::audit::{AuditLog, AuditOutcome};
use crate::plugin::capability_grants::{CheckOutcome, GrantStore, ScopeResolver};
use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::resources::{GenerationId, PluginId};

thread_local! {
    static SERVICE_CALLER_STACK: RefCell<Vec<Rc<CapabilityGuard>>> = const { RefCell::new(Vec::new()) };
}

pub struct ServiceCallerScope;

impl Drop for ServiceCallerScope {
    fn drop(&mut self) {
        SERVICE_CALLER_STACK.with(|stack| {
            let _ = stack.borrow_mut().pop();
        });
    }
}

#[derive(Debug, Clone)]
pub struct CapabilityGuard {
    plugin_id: String,
    plugin_version: String,
    requested: Vec<Capability>,
    requested_set: HashSet<String>,
    plugin_root: PathBuf,
    state_dir: PathBuf,
    config_dir: PathBuf,
    workdir: Option<PathBuf>,
    audit: Option<AuditLog>,
    diagnostics: Option<(DiagnosticStore, PluginId, GenerationId)>,
    grant_store: GrantStore,
}

impl CapabilityGuard {
    pub fn new(
        plugin_id: impl Into<String>,
        plugin_version: impl Into<String>,
        requested: Vec<Capability>,
        plugin_root: PathBuf,
        state_dir: PathBuf,
        config_dir: PathBuf,
        workdir: Option<PathBuf>,
        grant_store: GrantStore,
    ) -> Self {
        let requested_set: HashSet<String> =
            requested.iter().map(|c| String::from(c.clone())).collect();
        Self {
            plugin_id: plugin_id.into(),
            plugin_version: plugin_version.into(),
            requested,
            requested_set,
            plugin_root,
            state_dir,
            config_dir,
            workdir,
            audit: None,
            diagnostics: None,
            grant_store,
        }
    }

    pub fn with_audit(mut self, log: AuditLog) -> Self {
        self.audit = Some(log);
        self
    }

    /// Forward capability denials into the host diagnostic store. Each
    /// denial emits an `Error` diagnostic in addition to the
    /// audit-log entry.
    pub fn with_diagnostics(
        mut self,
        store: DiagnosticStore,
        plugin_id: PluginId,
        generation_id: GenerationId,
    ) -> Self {
        self.diagnostics = Some((store, plugin_id, generation_id));
        self
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn plugin_version(&self) -> &str {
        &self.plugin_version
    }

    pub fn plugin_root(&self) -> &Path {
        &self.plugin_root
    }

    fn record_audit(&self, capability: &str, target: &str, outcome: AuditOutcome) {
        if let Some(log) = &self.audit {
            log.record(&self.plugin_id, capability, target, outcome);
        }
    }

    fn record_diagnostic(&self, code: &str, capability: &str, target: &str, message: String) {
        if let Some((store, plugin_id, generation_id)) = &self.diagnostics {
            let context = serde_json::json!({
                "capability": capability,
                "target": target,
                "plugin_version": self.plugin_version,
            });
            store.record(
                PluginDiagnostic::new(plugin_id.clone(), DiagnosticSeverity::Error, code, message)
                    .with_generation(*generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("capability:{capability}"),
                    })
                    .with_context(context),
            );
        }
    }

    /// Granted set = the manifest's requested set intersected with the
    /// `Allow` rows in the grant store. Used by the path resolver to
    /// know which scopes are live.
    fn granted_capabilities(&self) -> Vec<Capability> {
        let mut out = Vec::new();
        for cap in &self.requested {
            let s: String = cap.clone().into();
            if matches!(
                self.grant_store
                    .check(&self.plugin_id, &self.plugin_version, &s),
                CheckOutcome::Allow
            ) {
                out.push(cap.clone());
            }
        }
        out
    }

    /// Phase 10: every gated string-form capability funnels through
    /// here. Returns `Ok(())` when the grant store says Allow; emits
    /// the right audit entry + diagnostic and returns the standard
    /// `capability denied: …` string otherwise.
    pub fn check_named(&self, capability: &str) -> Result<(), String> {
        self.check_named_self(capability)?;
        if let Some(caller) = self.active_service_caller() {
            caller.check_named_self(capability).map_err(|reason| {
                format!(
                    "service caller `{}` lacks capability `{capability}`: {reason}",
                    caller.plugin_id()
                )
            })?;
        }
        Ok(())
    }

    fn check_named_self(&self, capability: &str) -> Result<(), String> {
        if !self.requested_set.contains(capability) {
            // Manifest never asked for this capability — the dispatcher
            // is asking about a capability the plugin can't even
            // request. Treat as a hard deny.
            self.record_audit(capability, "", AuditOutcome::Denied);
            self.record_diagnostic(
                "capability.denied",
                capability,
                "",
                format!("capability `{capability}` not declared in plugin.toml"),
            );
            return Err(format!("capability not declared: {capability}"));
        }
        match self
            .grant_store
            .check(&self.plugin_id, &self.plugin_version, capability)
        {
            CheckOutcome::Allow => {
                self.record_audit(capability, "", AuditOutcome::Allowed);
                Ok(())
            }
            CheckOutcome::Deny { reason } => {
                self.record_audit(capability, "", AuditOutcome::Denied);
                let row =
                    self.grant_store
                        .lookup(&self.plugin_id, &self.plugin_version, capability);
                let code = match row {
                    Some(r) if r.notes.as_deref() == Some("revoked") => "capability.revoked",
                    _ => "capability.denied",
                };
                self.record_diagnostic(code, capability, "", reason.clone());
                Err(reason)
            }
            CheckOutcome::RequiresPrompt => {
                self.record_audit(capability, "", AuditOutcome::Denied);
                let msg = format!("capability denied: {capability} (no decision recorded)");
                self.record_diagnostic("capability.denied", capability, "", msg.clone());
                Err(msg)
            }
        }
    }

    pub fn check_fs_read(&self, path: &Path) -> Result<(), String> {
        self.check_fs(path, false)
    }

    /// Phase 12: check `fs:watch[:scope]` against `path`. Records audit
    /// + diagnostic on denial.
    pub fn check_fs_watch(&self, path: &Path) -> Result<(), String> {
        self.check_fs_watch_self(path)?;
        if let Some(caller) = self.active_service_caller() {
            caller.check_fs_watch_self(path).map_err(|reason| {
                format!(
                    "service caller `{}` lacks fs:watch access for `{}`: {reason}",
                    caller.plugin_id(),
                    path.display()
                )
            })?;
        }
        Ok(())
    }

    fn check_fs_watch_self(&self, path: &Path) -> Result<(), String> {
        let granted = self.granted_capabilities();
        let resolver = ScopeResolver {
            plugin_root: &self.plugin_root,
            state_dir: &self.state_dir,
            config_dir: &self.config_dir,
            workdir: self.workdir.as_deref(),
        };
        match resolver.check_watch_scope(path, &granted) {
            Ok(_canon) => {
                self.record_audit(
                    "fs:watch",
                    &path.display().to_string(),
                    AuditOutcome::Allowed,
                );
                Ok(())
            }
            Err((code, msg)) => {
                self.record_audit(
                    "fs:watch",
                    &path.display().to_string(),
                    AuditOutcome::Denied,
                );
                self.record_diagnostic(&code, "fs:watch", &path.display().to_string(), msg.clone());
                Err(msg)
            }
        }
    }

    pub fn check_fs_write(&self, path: &Path) -> Result<(), String> {
        self.check_fs(path, true)
    }

    fn check_fs(&self, path: &Path, want_write: bool) -> Result<(), String> {
        self.check_fs_self(path, want_write)?;
        if let Some(caller) = self.active_service_caller() {
            caller.check_fs_self(path, want_write).map_err(|reason| {
                let cap_label = if want_write { "fs:write" } else { "fs:read" };
                format!(
                    "service caller `{}` lacks {cap_label} access for `{}`: {reason}",
                    caller.plugin_id(),
                    path.display()
                )
            })?;
        }
        Ok(())
    }

    fn check_fs_self(&self, path: &Path, want_write: bool) -> Result<(), String> {
        let granted = self.granted_capabilities();
        let resolver = ScopeResolver {
            plugin_root: &self.plugin_root,
            state_dir: &self.state_dir,
            config_dir: &self.config_dir,
            workdir: self.workdir.as_deref(),
        };
        let cap_label = if want_write { "fs:write" } else { "fs:read" };
        match resolver.check_fs_scope(path, &granted, want_write) {
            Ok(_canon) => {
                self.record_audit(
                    cap_label,
                    &path.display().to_string(),
                    AuditOutcome::Allowed,
                );
                Ok(())
            }
            Err((code, msg)) => {
                self.record_audit(cap_label, &path.display().to_string(), AuditOutcome::Denied);
                self.record_diagnostic(&code, cap_label, &path.display().to_string(), msg.clone());
                Err(msg)
            }
        }
    }

    /// Compatibility wrapper for the env API: returns `Ok` if either
    /// the bare `env` capability or any granted `env:<glob>` allows
    /// reading every variable. Phase 10 wires the bare form; future
    /// phases may add per-glob filtering inside `env::list`.
    pub fn check_env(&self) -> Result<(), String> {
        self.check_env_self()?;
        if let Some(caller) = self.active_service_caller() {
            caller.check_env_self().map_err(|reason| {
                format!(
                    "service caller `{}` lacks env access: {reason}",
                    caller.plugin_id()
                )
            })?;
        }
        Ok(())
    }

    fn check_env_self(&self) -> Result<(), String> {
        // Try the bare `env` capability first. If the manifest
        // declares only globs, that's a no-go for `list()` — list
        // exposes everything.
        if self.requested_set.contains("env") {
            return self.check_named_self("env");
        }
        // No `env` form requested — straight deny.
        self.record_audit("env", "", AuditOutcome::Denied);
        self.record_diagnostic(
            "capability.denied",
            "env",
            "",
            "capability `env` not declared in plugin.toml".into(),
        );
        Err("capability not declared: env".to_string())
    }

    /// True when the manifest requested `capability` (regardless of
    /// whether it's been granted yet). Used by the dispatcher to
    /// short-circuit checks for capabilities the plugin can't even
    /// ask for.
    pub fn requested(&self, capability: &str) -> bool {
        self.requested_set.contains(capability)
    }

    pub fn push_service_caller(caller: Rc<CapabilityGuard>) -> ServiceCallerScope {
        SERVICE_CALLER_STACK.with(|stack| {
            stack.borrow_mut().push(caller);
        });
        ServiceCallerScope
    }

    fn active_service_caller(&self) -> Option<Rc<CapabilityGuard>> {
        SERVICE_CALLER_STACK.with(|stack| {
            stack
                .borrow()
                .last()
                .filter(|caller| caller.plugin_id() != self.plugin_id())
                .cloned()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::capability_grants::{DecidedBy, Decision};
    use git_leviathan_plugin_api::capability::FsScope;
    use std::fs;

    fn build_guard(
        plugin_id: &str,
        plugin_version: &str,
        requested: Vec<Capability>,
        store: &GrantStore,
    ) -> CapabilityGuard {
        let tmp = std::env::temp_dir().join(format!("guard_test_{plugin_id}"));
        fs::create_dir_all(&tmp).ok();
        CapabilityGuard::new(
            plugin_id,
            plugin_version,
            requested,
            tmp.clone(),
            tmp.clone(),
            tmp,
            None,
            store.clone(),
        )
    }

    #[test]
    fn check_named_allows_after_grant() {
        let store = GrantStore::new_in_memory();
        let guard = build_guard("p", "1.0.0", vec![Capability::Env], &store);
        assert!(guard.check_named("env").is_err(), "no grant yet");
        store
            .record_decision(
                "p",
                "1.0.0",
                "env",
                Decision::Allow,
                DecidedBy::Default,
                None,
            )
            .unwrap();
        assert!(guard.check_named("env").is_ok());
    }

    #[test]
    fn check_named_rejects_undeclared_even_if_granted() {
        let store = GrantStore::new_in_memory();
        let guard = build_guard("p", "1.0.0", vec![], &store);
        store
            .record_decision(
                "p",
                "1.0.0",
                "env",
                Decision::Allow,
                DecidedBy::Default,
                None,
            )
            .unwrap();
        assert!(guard.check_named("env").is_err());
    }

    #[test]
    fn fs_read_uses_grant_store_for_scope_check() {
        let store = GrantStore::new_in_memory();
        let tmp = tempfile::tempdir().unwrap();
        let plugin_root = tmp.path().to_path_buf();
        std::fs::write(plugin_root.join("a.txt"), "x").unwrap();
        let guard = CapabilityGuard::new(
            "p",
            "1.0.0",
            vec![Capability::FsRead {
                scope: FsScope::Plugin,
            }],
            plugin_root.clone(),
            plugin_root.clone(),
            plugin_root.clone(),
            None,
            store.clone(),
        );
        // Without a grant: deny.
        assert!(guard.check_fs_read(&plugin_root.join("a.txt")).is_err());
        store
            .record_decision(
                "p",
                "1.0.0",
                "fs:read:plugin",
                Decision::Allow,
                DecidedBy::Default,
                None,
            )
            .unwrap();
        // With a grant: allow.
        assert!(guard.check_fs_read(&plugin_root.join("a.txt")).is_ok());
    }
}

//! lazy activation registry.
//!
//! When a plugin's manifest carries an `[activation]` section the
//! host installs a *stub* registration for each declared trigger
//! instead of running the plugin's `init.lua`. The plugin's Lua state
//! does not exist while it is parked here; on first matching trigger
//! the host removes the stubs, runs the same eager-load path, and
//! re-dispatches the trigger so the plugin sees its first invocation.
//!
//! Engineering invariants honoured here:
//!
//! 1. **Host owns every effect.** Stubs live in the registry keyed
//!    by `(plugin_id)` and are removed atomically when the plugin
//!    activates. No Lua callback runs from a stub — the host
//!    performs all dispatching.
//! 2. **Resource-ledger ownership.** Each stub is also recorded as
//!    a [`crate::plugin::resources::PluginResourceKind::ActivationStub`]
//!    against a synthetic generation id (`0`) so a `unload_plugin`
//!    walk reaps them just like real registrations.
//! 3. **Single activation guarantee.** The first matching trigger
//!    wins; subsequent triggers no-op until the plugin is loaded.
//!    Failed activation marks the plugin **poisoned** so we don't
//!    spin up repeatedly.
//!
//! Diagnostic codes emitted from this module / its callers:
//!
//! - `manifest.activation_invalid` (warning) — bad activation
//!   section (e.g. empty command id).
//! - `activation.unknown_event` (warning) — declared event isn't a
//!   canonical name.
//! - `activation.unknown_command` (warning) — manifest declared a
//!   command that the plugin didn't register after activation.
//! - `activation.unknown_keymap` (warning) — same, for keymaps.
//! - `activation.failed` (error) — load error during activation.
//! - `activation.poisoned` (error) — repeated failures; the plugin
//!   is disabled until manual re-enable.

use std::collections::HashSet;
use std::path::PathBuf;

use git_leviathan_plugin_api::descriptor::api as event_descriptor;
use git_leviathan_plugin_api::manifest::{ActivationKeymap, PluginActivation};

use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::resources::PluginId;

/// Threshold at which a repeatedly-failing lazy plugin is poisoned.
/// Two fits the "transient vs. permanent" split called out in the
/// plan: a single failure can be a dependency-not-loaded blip; the
/// second consecutive failure is treated as the plugin's fault.
pub const MAX_ACTIVATION_FAILURES: u32 = 2;

/// Plugin lifecycle status as seen by the lazy registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LazyStatus {
    /// Stubs are installed; plugin's Lua state does not exist.
    Lazy,
    /// Activation succeeded — plugin loaded eagerly.
    Active,
    /// Activation failed past [`MAX_ACTIVATION_FAILURES`]; the host
    /// will not retry until the user re-enables it.
    Poisoned,
}

impl LazyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lazy => "lazy",
            Self::Active => "active",
            Self::Poisoned => "poisoned",
        }
    }
}

/// One row of the lazy registry.
#[derive(Debug, Clone)]
pub struct LazyEntry {
    pub plugin_id: String,
    pub plugin_dir: PathBuf,
    pub manual: bool,
    pub commands: Vec<String>,
    pub keymaps: Vec<ActivationKeymap>,
    pub events: Vec<String>,
    pub regions: Vec<String>,
    pub files: Vec<PathBuf>,
    pub repository_shape: Option<git_leviathan_plugin_api::manifest::RepositoryShapePredicate>,
    pub status: LazyStatus,
    pub activations: u64,
    pub last_activation_unix_ms: Option<u128>,
    /// Pretty-printed descriptor of the most recent activation
    /// trigger (e.g. `command:foo`, `keymap:global:<leader>x`,
    /// `event:FetchStarted`, `repository_shape`). `None` until first
    /// successful activation. Surfaced through the devtools snapshot
    /// so the inspector can show *what* woke the plugin up.
    pub last_activation_trigger: Option<String>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
}

impl LazyEntry {
    pub fn matches_repo(&self, current_branch: &str, has_remote: bool) -> bool {
        let Some(shape) = self.repository_shape.as_ref() else {
            return false;
        };
        if let Some(want_remote) = shape.has_remote {
            if want_remote != has_remote {
                return false;
            }
        }
        if let Some(want_branch) = shape.has_branch.as_deref() {
            if want_branch != current_branch {
                return false;
            }
        }
        true
    }
}

/// Lazy registration store. The registry itself is small (tens of
/// entries, not millions) so the linear iteration paths used by region
/// / repository shape are fine.
#[derive(Debug, Default)]
pub struct LazyRegistry {
    entries: Vec<LazyEntry>,
}

impl LazyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[LazyEntry] {
        &self.entries
    }

    /// Mutable handle to the entry vector. The lazy registry is
    /// small and the host needs both targeted in-place updates
    /// (failure counters, activation timestamps) and occasional
    /// removal (`unload_plugin`); exposing the slice keeps the
    /// surface narrow without proliferating per-call helpers.
    pub fn entries_mut(&mut self) -> &mut Vec<LazyEntry> {
        &mut self.entries
    }

    pub fn lookup(&self, plugin_id: &str) -> Option<&LazyEntry> {
        self.entries.iter().find(|e| e.plugin_id == plugin_id)
    }

    pub fn insert(&mut self, entry: LazyEntry) {
        if let Some(idx) = self
            .entries
            .iter()
            .position(|e| e.plugin_id == entry.plugin_id)
        {
            self.entries[idx] = entry;
        } else {
            self.entries.push(entry);
        }
    }

    pub fn match_event(&self, name: &str) -> Option<&LazyEntry> {
        self.entries
            .iter()
            .filter(|e| e.status == LazyStatus::Lazy)
            .find(|e| e.events.iter().any(|n| n == name))
    }

    pub fn match_repo_shape(&self, current_branch: &str, has_remote: bool) -> Option<&LazyEntry> {
        self.entries
            .iter()
            .filter(|e| e.status == LazyStatus::Lazy)
            .find(|e| e.matches_repo(current_branch, has_remote))
    }

    /// Mark a plugin successfully activated. `trigger_descriptor` is
    /// stored on the row so devtools can show the originating
    /// trigger after the fact.
    pub fn mark_active(&mut self, plugin_id: &str, now_unix_ms: u128, trigger_descriptor: String) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.plugin_id == plugin_id) {
            e.status = LazyStatus::Active;
            e.activations += 1;
            e.last_activation_unix_ms = Some(now_unix_ms);
            e.last_activation_trigger = Some(trigger_descriptor);
            e.last_error = None;
            e.consecutive_failures = 0;
        }
    }

    /// Record an activation failure; returns true when the entry
    /// crossed the poison threshold.
    pub fn mark_failure(&mut self, plugin_id: &str, error: String) -> bool {
        if let Some(e) = self.entries.iter_mut().find(|e| e.plugin_id == plugin_id) {
            e.consecutive_failures = e.consecutive_failures.saturating_add(1);
            e.last_error = Some(error);
            if e.consecutive_failures >= MAX_ACTIVATION_FAILURES {
                e.status = LazyStatus::Poisoned;
                return true;
            }
        }
        false
    }
}

/// Validate a manifest's activation section. Records
/// `manifest.activation_invalid` for empty ids and
/// `activation.unknown_event` for events absent from the canonical
/// descriptor table. Returns the (filtered) trigger lists.
pub struct ValidatedActivation {
    pub commands: Vec<String>,
    pub keymaps: Vec<ActivationKeymap>,
    pub events: Vec<String>,
    pub regions: Vec<String>,
    pub files: Vec<PathBuf>,
    pub repository_shape: Option<git_leviathan_plugin_api::manifest::RepositoryShapePredicate>,
    pub manual: bool,
}

pub fn validate(
    plugin_id: &str,
    manifest_path: &std::path::Path,
    activation: &PluginActivation,
    diagnostics: &DiagnosticStore,
) -> ValidatedActivation {
    let mut commands: Vec<String> = Vec::new();
    let mut seen_command: HashSet<String> = HashSet::new();
    for cmd in &activation.commands {
        if cmd.trim().is_empty() {
            record_invalid(
                diagnostics,
                plugin_id,
                manifest_path,
                "activation.commands entry is empty",
            );
            continue;
        }
        if seen_command.insert(cmd.clone()) {
            commands.push(cmd.clone());
        }
    }
    let mut keymaps: Vec<ActivationKeymap> = Vec::new();
    let mut seen_km: HashSet<(String, String)> = HashSet::new();
    for km in &activation.keymaps {
        if km.context.trim().is_empty() || km.key.trim().is_empty() {
            record_invalid(
                diagnostics,
                plugin_id,
                manifest_path,
                "activation.keymaps entry has empty context or key",
            );
            continue;
        }
        if seen_km.insert((km.context.clone(), km.key.clone())) {
            keymaps.push(ActivationKeymap {
                context: km.context.clone(),
                key: km.key.clone(),
            });
        }
    }
    let mut events: Vec<String> = Vec::new();
    let mut seen_event: HashSet<String> = HashSet::new();
    for ev in &activation.events {
        if ev.trim().is_empty() {
            record_invalid(
                diagnostics,
                plugin_id,
                manifest_path,
                "activation.events entry is empty",
            );
            continue;
        }
        match event_descriptor::event_descriptor(ev) {
            Some(desc) if !desc.is_alias => {
                if seen_event.insert(ev.clone()) {
                    events.push(ev.clone());
                }
            }
            Some(_) | None => {
                diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Warning,
                        "activation.unknown_event",
                        format!("activation.events lists `{ev}` which is not a canonical event"),
                    )
                    .with_source(PluginSourceSpan::Manifest {
                        path: manifest_path.display().to_string(),
                        key: Some("activation.events".to_string()),
                    })
                    .with_context(serde_json::json!({ "event": ev })),
                );
            }
        }
    }
    let mut regions: Vec<String> = Vec::new();
    let mut seen_region: HashSet<String> = HashSet::new();
    for r in &activation.regions {
        if r.trim().is_empty() {
            record_invalid(
                diagnostics,
                plugin_id,
                manifest_path,
                "activation.regions entry is empty",
            );
            continue;
        }
        if seen_region.insert(r.clone()) {
            regions.push(r.clone());
        }
    }
    let mut files: Vec<PathBuf> = Vec::new();
    let mut seen_file: HashSet<PathBuf> = HashSet::new();
    for f in &activation.files {
        if f.trim().is_empty() {
            record_invalid(
                diagnostics,
                plugin_id,
                manifest_path,
                "activation.files entry is empty",
            );
            continue;
        }
        let path = PathBuf::from(f);
        if seen_file.insert(path.clone()) {
            files.push(path);
        }
    }
    ValidatedActivation {
        commands,
        keymaps,
        events,
        regions,
        files,
        repository_shape: activation.repository_shape.clone(),
        manual: activation.manual,
    }
}

fn record_invalid(
    diagnostics: &DiagnosticStore,
    plugin_id: &str,
    manifest_path: &std::path::Path,
    message: &str,
) {
    diagnostics.record(
        PluginDiagnostic::new(
            PluginId::from(plugin_id),
            DiagnosticSeverity::Warning,
            "manifest.activation_invalid",
            message,
        )
        .with_source(PluginSourceSpan::Manifest {
            path: manifest_path.display().to_string(),
            key: Some("activation".to_string()),
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_diags() -> DiagnosticStore {
        DiagnosticStore::default()
    }

    fn fixture_entry(plugin_id: &str, command: &str) -> LazyEntry {
        LazyEntry {
            plugin_id: plugin_id.into(),
            plugin_dir: PathBuf::new(),
            manual: false,
            commands: vec![command.into()],
            keymaps: vec![],
            events: vec![],
            regions: vec![],
            files: vec![],
            repository_shape: None,
            status: LazyStatus::Lazy,
            activations: 0,
            last_activation_unix_ms: None,
            last_activation_trigger: None,
            last_error: None,
            consecutive_failures: 0,
        }
    }

    /// Linear scan for a lazy command match. Mirrors the host
    /// dispatcher's probe; lives in tests because the only
    /// non-test caller is the test-only `invoke_command`/
    /// `dispatch_key` lazy branch.
    fn match_command<'a>(reg: &'a LazyRegistry, name: &str) -> Option<&'a LazyEntry> {
        reg.entries()
            .iter()
            .filter(|e| e.status == LazyStatus::Lazy)
            .find(|e| e.commands.iter().any(|c| c == name))
    }

    #[test]
    fn validate_drops_unknown_event() {
        let diags = empty_diags();
        let activation = PluginActivation {
            events: vec!["NopeNotAnEvent".into()],
            ..Default::default()
        };
        let v = validate(
            "p",
            std::path::Path::new("plugin.toml"),
            &activation,
            &diags,
        );
        assert!(v.events.is_empty());
        assert!(diags
            .by_code("activation.unknown_event")
            .iter()
            .any(|d| d.context["event"] == "NopeNotAnEvent"));
    }

    #[test]
    fn registry_tracks_first_match_wins() {
        let mut reg = LazyRegistry::new();
        reg.insert(fixture_entry("a", "x"));
        reg.insert(fixture_entry("b", "x"));
        let m = match_command(&reg, "x").expect("match");
        assert_eq!(m.plugin_id, "a");
    }

    #[test]
    fn poison_threshold() {
        let mut reg = LazyRegistry::new();
        reg.insert(fixture_entry("p", "c"));
        assert!(!reg.mark_failure("p", "fail #1".into()));
        assert!(reg.mark_failure("p", "fail #2".into()));
        let entry = reg.lookup("p").unwrap();
        assert_eq!(entry.status, LazyStatus::Poisoned);
        assert_eq!(entry.consecutive_failures, MAX_ACTIVATION_FAILURES);
        assert!(match_command(&reg, "c").is_none());
    }

    #[test]
    fn mark_active_records_trigger_descriptor() {
        let mut reg = LazyRegistry::new();
        reg.insert(fixture_entry("p", "c"));
        reg.mark_active("p", 1234, "command:c".into());
        let entry = reg.lookup("p").unwrap();
        assert_eq!(entry.status, LazyStatus::Active);
        assert_eq!(entry.activations, 1);
        assert_eq!(entry.last_activation_trigger.as_deref(), Some("command:c"));
    }
}

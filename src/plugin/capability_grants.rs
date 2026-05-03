//! Capability grants, prompts, and persistence.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use git_leviathan_plugin_api::capability::{is_known_capability, Capability, FsScope};

use crate::plugin::audit::{AuditLog, AuditOutcome};

/// Decision recorded for one `(plugin_id, plugin_version, capability)`
/// triple. `Pending` exists only inside [`PromptState`]; persisted
/// rows are always `Allow` or `Deny`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny,
    Pending,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Pending => "pending",
        }
    }
}

/// Who decided a capability grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecidedBy {
    User,
    Default,
    Policy,
}

impl DecidedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Default => "default",
            Self::Policy => "policy",
        }
    }
}

/// One persisted grant row. Equality of `plugin_id + plugin_version +
/// capability` is the lookup key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantRow {
    pub plugin_id: String,
    pub plugin_version: String,
    pub capability: String,
    pub granted_at: String,
    pub decision: Decision,
    pub decided_by: DecidedBy,
    pub notes: Option<String>,
}

/// Outcome of a single [`GrantStore::check`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    Allow,
    Deny {
        reason: String,
    },
    /// No decision yet — the host must prompt and re-check.
    RequiresPrompt,
}

/// Auto-grant policy applied at plugin-load time. Bundled plugins (those
/// living under the project's `plugins/` directory) get every requested
/// capability auto-allowed with `decided_by = "default"` so the
/// shipped UI keeps working. Non-bundled plugins go through the prompt
/// path and start out with `pending` decisions.
#[derive(Debug, Clone, Default)]
pub struct AutoGrantPolicy {
    bundled_roots: Vec<PathBuf>,
}

impl AutoGrantPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark a directory as containing trusted bundled plugins. Any
    /// plugin loaded from a path that canonicalises under this root
    /// gets its requested capabilities auto-granted as `default`.
    pub fn trust_bundled_root(&mut self, root: impl Into<PathBuf>) {
        let path = root.into();
        let canon = fs::canonicalize(&path).unwrap_or(path);
        if !self.bundled_roots.contains(&canon) {
            self.bundled_roots.push(canon);
        }
    }

    /// True when `plugin_root` lives under a trusted bundled root.
    pub fn is_bundled(&self, plugin_root: &Path) -> bool {
        let canon = fs::canonicalize(plugin_root).unwrap_or_else(|_| plugin_root.to_path_buf());
        self.bundled_roots
            .iter()
            .any(|root| canon.starts_with(root))
    }
}

/// One pending decision the prompt is waiting on. The prompt overlay
/// lists these to the user.
#[derive(Debug, Clone, Serialize)]
pub struct PendingDecision {
    pub plugin_id: String,
    pub plugin_version: String,
    pub capability: String,
}

/// One row of the security prompt the host hands the renderer. The
/// headless [`PromptState`] commits these into the grant store via
/// [`PromptState::decide`]; devtools will render them in a modal.
#[derive(Debug, Clone)]
pub struct PromptState {
    pending: Vec<PendingDecision>,
    decisions: HashMap<(String, String, String), Decision>,
}

impl PromptState {
    pub fn new(pending: Vec<PendingDecision>) -> Self {
        Self {
            pending,
            decisions: HashMap::new(),
        }
    }

    pub fn pending(&self) -> &[PendingDecision] {
        &self.pending
    }

    /// Record the user's choice for one capability. Subsequent calls
    /// with the same key overwrite the prior decision (matches the
    /// modal "click again" UX).
    pub fn decide(
        &mut self,
        plugin_id: &str,
        plugin_version: &str,
        capability: &str,
        decision: Decision,
    ) {
        self.decisions.insert(
            (
                plugin_id.to_string(),
                plugin_version.to_string(),
                capability.to_string(),
            ),
            decision,
        );
    }

    /// All decisions resolved? Used by the host to know the prompt can
    /// close.
    pub fn is_resolved(&self) -> bool {
        self.pending
            .iter()
            .all(|d| self.decisions.contains_key(&key(d)))
    }

    /// Drain decisions into `(plugin_id, plugin_version, capability,
    /// decision)` tuples ready to commit. Pending capabilities the
    /// user did not explicitly decide on are committed as `Deny` —
    /// the safe default — with `decided_by = "user"` because closing
    /// the prompt is itself a user action.
    pub fn finalize(self) -> Vec<(String, String, String, Decision)> {
        let mut out = Vec::with_capacity(self.pending.len());
        for d in &self.pending {
            let decision = self
                .decisions
                .get(&key(d))
                .copied()
                .unwrap_or(Decision::Deny);
            out.push((
                d.plugin_id.clone(),
                d.plugin_version.clone(),
                d.capability.clone(),
                decision,
            ));
        }
        out
    }
}

fn key(d: &PendingDecision) -> (String, String, String) {
    (
        d.plugin_id.clone(),
        d.plugin_version.clone(),
        d.capability.clone(),
    )
}

/// Serialised shape of a [`PromptState`] for the devtools overlay.
/// The renderer doesn't need direct access to the
/// store — it consumes this descriptor.
#[derive(Debug, Clone, Serialize)]
pub struct OverlayDescriptor {
    pub kind: &'static str,
    pub title: String,
    pub plugin_id: String,
    pub plugin_version: String,
    pub pending: Vec<PendingDecision>,
}

impl OverlayDescriptor {
    pub fn from_prompt(prompt: &PromptState) -> Option<Self> {
        let first = prompt.pending().first()?;
        let plugin_id = first.plugin_id.clone();
        let plugin_version = first.plugin_version.clone();
        let title = format!(
            "{} v{} requests {} capabilities",
            plugin_id,
            plugin_version,
            prompt.pending().len()
        );
        Some(Self {
            kind: "PluginCapabilityPrompt",
            title,
            plugin_id,
            plugin_version,
            pending: prompt.pending().to_vec(),
        })
    }
}

#[derive(Debug, Default)]
struct StoreInner {
    /// Keyed by `(plugin_id, plugin_version, capability)`.
    rows: HashMap<(String, String, String), GrantRow>,
    /// Pending prompts the host has opened but the user has not yet
    /// resolved. Keyed by `(plugin_id, plugin_version)` — one prompt
    /// per plugin version at a time.
    pending: HashMap<(String, String), PromptState>,
    /// Where to mirror grants to (None = in-memory only). Set via
    /// [`GrantStore::with_path`].
    path: Option<PathBuf>,
}

/// Grant store. Cheap-clone — the inner state is `Arc<Mutex<…>>`,
/// so many handles can share the same store and persistent file.
#[derive(Debug, Clone)]
pub struct GrantStore {
    inner: Arc<Mutex<StoreInner>>,
}

impl Default for GrantStore {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StoreInner::default())),
        }
    }
}

impl GrantStore {
    /// Construct an in-memory store. Tests use this; production wires
    /// a file via [`GrantStore::with_path`].
    pub fn new_in_memory() -> Self {
        Self::default()
    }

    /// Construct a store backed by `path`. If the file exists it is
    /// loaded; missing files are treated as empty (the file is created
    /// on the first commit). Read errors do not panic — they are
    /// surfaced via the returned `Vec<String>` of warnings so the
    /// caller can forward them as `capability.persistence_failed`
    /// diagnostics.
    pub fn with_path(path: PathBuf) -> (Self, Vec<String>) {
        let mut inner = StoreInner {
            path: Some(path.clone()),
            ..StoreInner::default()
        };
        let mut warnings = Vec::new();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(raw) => match serde_json::from_str::<PersistedFile>(&raw) {
                    Ok(file) => {
                        for row in file.grants {
                            let key = (
                                row.plugin_id.clone(),
                                row.plugin_version.clone(),
                                row.capability.clone(),
                            );
                            inner.rows.insert(key, row);
                        }
                    }
                    Err(e) => warnings.push(format!("could not parse {}: {e}", path.display())),
                },
                Err(e) => warnings.push(format!("could not read {}: {e}", path.display())),
            }
        }
        (
            Self {
                inner: Arc::new(Mutex::new(inner)),
            },
            warnings,
        )
    }

    /// Insert or replace a grant row. Persisted to the backing file
    /// (when configured); returns a non-empty warning string on I/O
    /// failure so the caller can emit `capability.persistence_failed`.
    pub fn record_decision(
        &self,
        plugin_id: &str,
        plugin_version: &str,
        capability: &str,
        decision: Decision,
        decided_by: DecidedBy,
        notes: Option<String>,
    ) -> Result<GrantRow, String> {
        let row = GrantRow {
            plugin_id: plugin_id.to_string(),
            plugin_version: plugin_version.to_string(),
            capability: capability.to_string(),
            granted_at: now_rfc3339(),
            decision,
            decided_by,
            notes,
        };
        let key = (
            row.plugin_id.clone(),
            row.plugin_version.clone(),
            row.capability.clone(),
        );
        let path_to_write = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|e| format!("grant store poisoned: {e}"))?;
            inner.rows.insert(key, row.clone());
            inner.path.clone()
        };
        if let Some(path) = path_to_write {
            self.persist_to(&path)?;
        }
        Ok(row)
    }

    /// Mark a previously-granted capability as denied. The row stays
    /// in the store with `decision = Deny`; the audit log keeps the
    /// historical `grant.allowed` row that preceded the revoke.
    pub fn revoke(
        &self,
        plugin_id: &str,
        plugin_version: &str,
        capability: &str,
    ) -> Result<(), String> {
        self.record_decision(
            plugin_id,
            plugin_version,
            capability,
            Decision::Deny,
            DecidedBy::User,
            Some("revoked".to_string()),
        )
        .map(|_| ())
    }

    /// Lookup a single grant. `None` when no decision has been
    /// recorded.
    pub fn lookup(
        &self,
        plugin_id: &str,
        plugin_version: &str,
        capability: &str,
    ) -> Option<GrantRow> {
        let inner = self.inner.lock().ok()?;
        inner
            .rows
            .get(&(
                plugin_id.to_string(),
                plugin_version.to_string(),
                capability.to_string(),
            ))
            .cloned()
    }

    /// Snapshot of every grant row. Used by the inspector view.
    pub fn rows(&self) -> Vec<GrantRow> {
        let inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut out: Vec<GrantRow> = inner.rows.values().cloned().collect();
        out.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.plugin_version.cmp(&b.plugin_version))
                .then(a.capability.cmp(&b.capability))
        });
        out
    }

    /// Open a prompt for one plugin version. Replaces any prior
    /// pending prompt for the same plugin_id+version.
    pub fn open_prompt(&self, prompt: PromptState) -> Result<(), String> {
        let key = prompt
            .pending()
            .first()
            .map(|p| (p.plugin_id.clone(), p.plugin_version.clone()))
            .ok_or_else(|| "prompt has no pending decisions".to_string())?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| format!("grant store poisoned: {e}"))?;
        inner.pending.insert(key, prompt);
        Ok(())
    }

    /// Snapshot of the currently-open prompts.
    pub fn pending_prompts(&self) -> Vec<PromptState> {
        match self.inner.lock() {
            Ok(g) => g.pending.values().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Resolve and remove the prompt for `(plugin_id, plugin_version)`,
    /// committing every decision it contains. Returns the committed
    /// rows in the same order they were pending so the caller can
    /// route them to the audit log.
    pub fn resolve_prompt(
        &self,
        plugin_id: &str,
        plugin_version: &str,
    ) -> Result<Vec<GrantRow>, String> {
        let prompt = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|e| format!("grant store poisoned: {e}"))?;
            inner
                .pending
                .remove(&(plugin_id.to_string(), plugin_version.to_string()))
        };
        let prompt = match prompt {
            Some(p) => p,
            None => return Ok(Vec::new()),
        };
        let resolutions = prompt.finalize();
        let mut committed = Vec::with_capacity(resolutions.len());
        for (pid, pver, cap, dec) in resolutions {
            let row = self.record_decision(&pid, &pver, &cap, dec, DecidedBy::User, None)?;
            committed.push(row);
        }
        Ok(committed)
    }

    /// True when there's an open prompt for this plugin version.
    pub fn has_pending_prompt(&self, plugin_id: &str, plugin_version: &str) -> bool {
        match self.inner.lock() {
            Ok(g) => g
                .pending
                .contains_key(&(plugin_id.to_string(), plugin_version.to_string())),
            Err(_) => false,
        }
    }

    /// Run a check at use time. The single chokepoint for every
    /// sensitive API call. Returns:
    /// - `Allow` — proceed.
    /// - `Deny` — refuse and (caller's job) emit `capability.denied`.
    /// - `RequiresPrompt` — no decision recorded yet; caller should
    ///   open a prompt and short-circuit the call.
    pub fn check(&self, plugin_id: &str, plugin_version: &str, capability: &str) -> CheckOutcome {
        let inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => {
                return CheckOutcome::Deny {
                    reason: "grant store poisoned".to_string(),
                };
            }
        };
        let row = inner.rows.get(&(
            plugin_id.to_string(),
            plugin_version.to_string(),
            capability.to_string(),
        ));
        match row {
            None => CheckOutcome::RequiresPrompt,
            Some(r) => match r.decision {
                Decision::Allow => CheckOutcome::Allow,
                Decision::Deny => CheckOutcome::Deny {
                    reason: format!("capability denied: {capability}"),
                },
                Decision::Pending => CheckOutcome::RequiresPrompt,
            },
        }
    }

    /// Compute the set of capabilities this plugin version requests
    /// that have NEVER been decided. Empty when every requested
    /// capability already has a row (regardless of allow/deny).
    pub fn undecided_for(
        &self,
        plugin_id: &str,
        plugin_version: &str,
        requested: &[String],
    ) -> Vec<String> {
        let inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return requested.to_vec(),
        };
        requested
            .iter()
            .filter(|cap| {
                !inner.rows.contains_key(&(
                    plugin_id.to_string(),
                    plugin_version.to_string(),
                    (*cap).clone(),
                ))
            })
            .cloned()
            .collect()
    }

    /// Apply the bundled-plugin auto-grant policy. Every requested
    /// capability the plugin doesn't already have a row for is
    /// committed as `Allow / Default`. Returns the rows actually
    /// inserted so the caller can audit them.
    pub fn auto_grant_bundled(
        &self,
        plugin_id: &str,
        plugin_version: &str,
        requested: &[String],
    ) -> Vec<GrantRow> {
        let to_grant = self.undecided_for(plugin_id, plugin_version, requested);
        let mut out = Vec::with_capacity(to_grant.len());
        for cap in to_grant {
            if let Ok(row) = self.record_decision(
                plugin_id,
                plugin_version,
                &cap,
                Decision::Allow,
                DecidedBy::Default,
                Some("bundled plugin auto-grant".to_string()),
            ) {
                out.push(row);
            }
        }
        out
    }

    fn persist_to(&self, path: &Path) -> Result<(), String> {
        let inner = self
            .inner
            .lock()
            .map_err(|e| format!("grant store poisoned: {e}"))?;
        let mut rows: Vec<GrantRow> = inner.rows.values().cloned().collect();
        rows.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.plugin_version.cmp(&b.plugin_version))
                .then(a.capability.cmp(&b.capability))
        });
        let file = PersistedFile {
            version: PERSIST_VERSION,
            grants: rows,
        };
        let raw =
            serde_json::to_string_pretty(&file).map_err(|e| format!("serialize grants: {e}"))?;
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return Err(format!("create parent dir {}: {e}", parent.display()));
            }
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, raw).map_err(|e| format!("write tmp {}: {e}", tmp.display()))?;
        fs::rename(&tmp, path)
            .map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))
    }
}

const PERSIST_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct PersistedFile {
    version: u32,
    grants: Vec<GrantRow>,
}

/// Project [`GrantStore::rows`] for the inspector. Cheap-clone of the
/// row data with a stable column order.
#[derive(Debug, Clone)]
pub struct CapabilityGrantSummary {
    pub plugin_id: String,
    pub plugin_version: String,
    pub capability: String,
    pub decision: String,
    pub decided_by: String,
    pub granted_at: String,
    pub notes: Option<String>,
}

impl From<GrantRow> for CapabilityGrantSummary {
    fn from(row: GrantRow) -> Self {
        Self {
            plugin_id: row.plugin_id,
            plugin_version: row.plugin_version,
            capability: row.capability,
            decision: row.decision.as_str().to_string(),
            decided_by: row.decided_by.as_str().to_string(),
            granted_at: row.granted_at,
            notes: row.notes,
        }
    }
}

/// Inspector projection of one open prompt.
#[derive(Debug, Clone)]
pub struct PendingPromptSummary {
    pub plugin_id: String,
    pub plugin_version: String,
    pub pending: Vec<String>,
}

impl PendingPromptSummary {
    pub fn from_prompt(prompt: &PromptState) -> Option<Self> {
        let first = prompt.pending().first()?;
        Some(Self {
            plugin_id: first.plugin_id.clone(),
            plugin_version: first.plugin_version.clone(),
            pending: prompt
                .pending()
                .iter()
                .map(|p| p.capability.clone())
                .collect(),
        })
    }
}

/// Resolve `path` against the granted set. Returns the canonical path
/// when allowed; an error code+message otherwise so the caller can
/// emit the right diagnostic. Symlink-defeat lives here: any path
/// that canonicalises outside the granted scope returns
/// `capability.path_outside_scope`.
///
/// The check walks every granted capability for this plugin and
/// returns the first match. `plugin_root` / `state_dir` / `config_dir`
/// / `workdir` provide the resolution targets for the named scopes.
pub struct ScopeResolver<'a> {
    pub plugin_root: &'a Path,
    pub state_dir: &'a Path,
    pub config_dir: &'a Path,
    pub workdir: Option<&'a Path>,
}

impl ScopeResolver<'_> {
    /// True when `path` (after canonicalisation) lives inside the
    /// directory the granted capability names. Returns the resolved
    /// canonical path on success; a `(code, message)` pair on failure.
    pub fn check_fs_scope(
        &self,
        path: &Path,
        granted: &[Capability],
        want_write: bool,
    ) -> Result<PathBuf, (String, String)> {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.plugin_root.join(path)
        };
        // canonicalize collapses symlinks. We use it for both the
        // scope dir and the requested path so a `safe/link_to_denied`
        // ends up under `denied/` and is rejected.
        let canon = fs::canonicalize(&resolved).unwrap_or(resolved.clone());
        for cap in granted {
            match (cap, want_write) {
                (Capability::FsRead { scope }, false) => {
                    if let Some(root) = self.scope_path(*scope) {
                        if canon.starts_with(&root) {
                            return Ok(canon);
                        }
                    }
                }
                (Capability::FsWrite { scope }, true) => {
                    if let Some(root) = self.scope_path(*scope) {
                        if canon.starts_with(&root) {
                            return Ok(canon);
                        }
                    }
                }
                (Capability::FsReadDir { dir }, false) => {
                    let want_canon = fs::canonicalize(dir).unwrap_or_else(|_| PathBuf::from(dir));
                    if canon.starts_with(&want_canon) {
                        return Ok(canon);
                    } else if PathBuf::from(&resolved).starts_with(PathBuf::from(dir)) {
                        // Path looks like it should be inside the scope but
                        // canonicalisation moved it out: the symlink-defeat
                        // path. Surface that distinct code.
                        return Err((
                            "capability.path_outside_scope".to_string(),
                            format!(
                                "path `{}` resolves outside scope `{}` via symlink",
                                path.display(),
                                dir
                            ),
                        ));
                    }
                }
                (Capability::FsWriteDir { dir }, true) => {
                    let want_canon = fs::canonicalize(dir).unwrap_or_else(|_| PathBuf::from(dir));
                    if canon.starts_with(&want_canon) {
                        return Ok(canon);
                    } else if PathBuf::from(&resolved).starts_with(PathBuf::from(dir)) {
                        return Err((
                            "capability.path_outside_scope".to_string(),
                            format!(
                                "path `{}` resolves outside scope `{}` via symlink",
                                path.display(),
                                dir
                            ),
                        ));
                    }
                }
                _ => {}
            }
        }
        Err((
            "capability.denied".to_string(),
            format!(
                "capability denied: fs:{} for {}",
                if want_write { "write" } else { "read" },
                path.display()
            ),
        ))
    }

    /// async runtime watch-scope check. Mirrors `check_fs_scope` but matches
    /// against `Capability::FsWatch` / `FsWatchDir` instead. Returns the
    /// canonical path on success.
    pub fn check_watch_scope(
        &self,
        path: &Path,
        granted: &[Capability],
    ) -> Result<PathBuf, (String, String)> {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.plugin_root.join(path)
        };
        let canon = fs::canonicalize(&resolved).unwrap_or(resolved.clone());
        for cap in granted {
            match cap {
                Capability::FsWatch { scope } => {
                    if let Some(root) = self.scope_path(*scope) {
                        if canon.starts_with(&root) {
                            return Ok(canon);
                        }
                    }
                }
                Capability::FsWatchDir { dir } => {
                    let want_canon = fs::canonicalize(dir).unwrap_or_else(|_| PathBuf::from(dir));
                    if canon.starts_with(&want_canon) {
                        return Ok(canon);
                    } else if PathBuf::from(&resolved).starts_with(PathBuf::from(dir)) {
                        return Err((
                            "capability.path_outside_scope".to_string(),
                            format!(
                                "path `{}` resolves outside scope `{}` via symlink",
                                path.display(),
                                dir
                            ),
                        ));
                    }
                }
                _ => {}
            }
        }
        Err((
            "capability.denied".to_string(),
            format!("capability denied: fs:watch for {}", path.display()),
        ))
    }

    fn scope_path(&self, scope: FsScope) -> Option<PathBuf> {
        match scope {
            FsScope::Plugin => Some(self.plugin_root.to_path_buf()),
            FsScope::State => Some(self.state_dir.to_path_buf()),
            FsScope::Config => Some(self.config_dir.to_path_buf()),
            FsScope::Workdir => self.workdir.map(|p| p.to_path_buf()),
            // `Any` is a wildcard: any path matches.
            FsScope::Any => Some(PathBuf::from("/")),
        }
    }
}

/// Check whether the granted set contains a string-form capability.
/// Used by the env / ui-region / git callers that don't need
/// path-resolution semantics.
pub fn granted_set_contains(granted: &[Capability], wanted: &str) -> bool {
    granted.iter().any(|c| {
        let s: String = c.clone().into();
        s == wanted
    })
}

/// Helper that records a grant lifecycle event on the audit log under
/// the canonical capability-grant codes. Audit entries flow into the same
/// devtools panel as every other capability allow/deny.
pub fn audit_grant_event(
    log: &AuditLog,
    plugin_id: &str,
    code: &str,
    capability: &str,
    decided_by: Option<DecidedBy>,
) {
    let target = match decided_by {
        Some(d) => format!("{capability}|by={}", d.as_str()),
        None => capability.to_string(),
    };
    log.record(plugin_id, code, target, AuditOutcome::Allowed);
}

fn now_rfc3339() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Cheap RFC-3339-ish encoder: enough for sort ordering + human
    // reading; we do NOT depend on chrono. Format: "1970-01-01T00:00:00Z"
    // shifted by the second offset.
    let days = secs / 86_400;
    let h = (secs % 86_400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    let (y, mo, d) = days_to_ymd(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Convert "days since 1970-01-01" into `(year, month, day)`. Public
/// algorithm derived from <howardhinnant.github.io/date_algorithms.html>.
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 {
        z / 146_097
    } else {
        (z - 146_096) / 146_097
    };
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5) + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

/// Build a [`PromptState`] for `(plugin_id, plugin_version)` covering
/// the requested-but-undecided capabilities.
pub fn prompt_for_undecided(
    store: &GrantStore,
    plugin_id: &str,
    plugin_version: &str,
    requested: &[String],
) -> Option<PromptState> {
    let undecided = store.undecided_for(plugin_id, plugin_version, requested);
    if undecided.is_empty() {
        return None;
    }
    let pending: Vec<PendingDecision> = undecided
        .into_iter()
        .map(|cap| PendingDecision {
            plugin_id: plugin_id.to_string(),
            plugin_version: plugin_version.to_string(),
            capability: cap,
        })
        .collect();
    Some(PromptState::new(pending))
}

/// Validate every requested capability against the descriptor table.
/// Unknown strings are returned to the caller so it can record
/// `capability.unknown` warnings without interrupting the load.
pub fn unknown_requested(requested: &[String]) -> Vec<String> {
    requested
        .iter()
        .filter(|s| !is_known_capability(s))
        .cloned()
        .collect()
}

/// Convenience: turn a manifest's `Vec<Capability>` into the canonical
/// strings the grant store keys on. Stable, deduplicated order.
pub fn canonicalize_requested(caps: &[Capability]) -> Vec<String> {
    let mut seen: BTreeMap<String, ()> = BTreeMap::new();
    for cap in caps {
        seen.insert(String::from(cap.clone()), ());
    }
    seen.into_keys().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn record_then_lookup_round_trips() {
        let store = GrantStore::new_in_memory();
        store
            .record_decision(
                "demo",
                "0.1.0",
                "fs:read:plugin",
                Decision::Allow,
                DecidedBy::User,
                None,
            )
            .unwrap();
        let row = store.lookup("demo", "0.1.0", "fs:read:plugin").unwrap();
        assert_eq!(row.decision, Decision::Allow);
        assert_eq!(row.decided_by, DecidedBy::User);
    }

    #[test]
    fn check_returns_requires_prompt_when_unknown() {
        let store = GrantStore::new_in_memory();
        assert_eq!(
            store.check("plug", "1.0.0", "fs:read:plugin"),
            CheckOutcome::RequiresPrompt
        );
    }

    #[test]
    fn check_allow_then_deny_after_revoke() {
        let store = GrantStore::new_in_memory();
        store
            .record_decision(
                "p",
                "1.0.0",
                "fs:read:plugin",
                Decision::Allow,
                DecidedBy::User,
                None,
            )
            .unwrap();
        assert_eq!(
            store.check("p", "1.0.0", "fs:read:plugin"),
            CheckOutcome::Allow
        );
        store.revoke("p", "1.0.0", "fs:read:plugin").unwrap();
        match store.check("p", "1.0.0", "fs:read:plugin") {
            CheckOutcome::Deny { .. } => {}
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn persistence_round_trips_across_store_instances() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("grants.json");
        let (store, warns) = GrantStore::with_path(path.clone());
        assert!(warns.is_empty());
        store
            .record_decision("p", "1.2.3", "env", Decision::Allow, DecidedBy::User, None)
            .unwrap();
        drop(store);
        let (store2, warns2) = GrantStore::with_path(path);
        assert!(warns2.is_empty());
        let row = store2.lookup("p", "1.2.3", "env").unwrap();
        assert_eq!(row.decision, Decision::Allow);
    }

    #[test]
    fn auto_grant_bundled_grants_only_undecided() {
        let store = GrantStore::new_in_memory();
        // Pre-record a Deny so auto-grant must not overwrite it.
        store
            .record_decision(
                "bundled",
                "1.0.0",
                "fs:read:any",
                Decision::Deny,
                DecidedBy::User,
                None,
            )
            .unwrap();
        let granted = store.auto_grant_bundled(
            "bundled",
            "1.0.0",
            &["fs:read:any".to_string(), "env".to_string()],
        );
        assert_eq!(granted.len(), 1);
        assert_eq!(granted[0].capability, "env");
        // The pre-existing Deny is untouched.
        let row = store.lookup("bundled", "1.0.0", "fs:read:any").unwrap();
        assert_eq!(row.decision, Decision::Deny);
    }

    #[test]
    fn prompt_finalize_defaults_undecided_to_deny() {
        let pending = vec![
            PendingDecision {
                plugin_id: "p".into(),
                plugin_version: "1.0.0".into(),
                capability: "env".into(),
            },
            PendingDecision {
                plugin_id: "p".into(),
                plugin_version: "1.0.0".into(),
                capability: "fs:read:plugin".into(),
            },
        ];
        let mut prompt = PromptState::new(pending);
        prompt.decide("p", "1.0.0", "env", Decision::Allow);
        let resolved = prompt.finalize();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].3, Decision::Allow);
        assert_eq!(resolved[1].3, Decision::Deny);
    }

    #[test]
    fn auto_grant_policy_recognises_bundled_root() {
        let dir = tempdir().unwrap();
        let plugin_root = dir.path().join("file_explorer");
        std::fs::create_dir(&plugin_root).unwrap();
        let mut policy = AutoGrantPolicy::new();
        policy.trust_bundled_root(dir.path());
        assert!(policy.is_bundled(&plugin_root));
    }

    #[test]
    fn scope_resolver_blocks_symlink_outside_granted_scope() {
        #[cfg(unix)]
        {
            let dir = tempdir().unwrap();
            let safe = dir.path().join("safe");
            let denied = dir.path().join("denied");
            std::fs::create_dir(&safe).unwrap();
            std::fs::create_dir(&denied).unwrap();
            std::fs::write(denied.join("secret.txt"), b"x").unwrap();
            std::os::unix::fs::symlink(&denied, safe.join("link")).unwrap();

            let resolver = ScopeResolver {
                plugin_root: dir.path(),
                state_dir: dir.path(),
                config_dir: dir.path(),
                workdir: None,
            };
            let granted = vec![Capability::FsReadDir {
                dir: safe.to_string_lossy().into_owned(),
            }];
            let target = safe.join("link/secret.txt");
            let err = resolver
                .check_fs_scope(&target, &granted, false)
                .unwrap_err();
            assert_eq!(err.0, "capability.path_outside_scope");
        }
    }

    #[test]
    fn unknown_requested_flags_unknown_capability_strings() {
        let v = unknown_requested(&[
            "env".to_string(),
            "destroy:everything".to_string(),
            "fs:read:plugin".to_string(),
        ]);
        assert_eq!(v, vec!["destroy:everything".to_string()]);
    }
}

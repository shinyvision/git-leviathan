//! Git write dispatch for `leviathan.git.*`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::plugin::audit::{AuditLog, AuditOutcome};
use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::events::EventPayload;
use crate::plugin::resources::{GenerationId, PluginId};
use crate::services::gateway::SharedRepositoryGateway;

/// One Git write request in flight (or just completed). Capped at 32
/// in the host's [`PendingGitWrites`] ring; older entries are dropped.
/// Devtools render this verbatim.
#[derive(Debug, Clone)]
pub struct PendingGitWrite {
    pub plugin_id: String,
    pub generation_id: u64,
    pub op: String,
    /// Compact JSON args summary so devtools can render the request
    /// without needing the original Lua tree.
    pub args_json: serde_json::Value,
    pub started_at_unix_ms: u128,
    /// Set after [`GitOpDispatcher::execute`] finishes. `None` while
    /// the op is still running.
    pub finished_at_unix_ms: Option<u128>,
    pub outcome: Option<GitOpStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOpStatus {
    Succeeded,
    Failed,
    /// Capability check rejected the op before it ran.
    CapabilityDenied,
    /// Destructive policy rejected the op before it ran.
    DestructiveBlocked,
    /// No repository gateway available (nothing's open).
    NoRepository,
}

impl GitOpStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::CapabilityDenied => "capability_denied",
            Self::DestructiveBlocked => "destructive_blocked",
            Self::NoRepository => "no_repository",
        }
    }
}

/// Bounded ring buffer of recent / in-flight Git writes. Cheap-clone
/// (Arc-backed). The host owns one; devtools clones it per snapshot.
#[derive(Clone, Default)]
pub struct PendingGitWrites {
    inner: Arc<Mutex<VecDeque<PendingGitWrite>>>,
}

impl PendingGitWrites {
    pub const CAP: usize = 32;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, entry: PendingGitWrite) -> u64 {
        let mut g = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return 0,
        };
        if g.len() >= Self::CAP {
            g.pop_front();
        }
        g.push_back(entry);
        g.len() as u64
    }

    pub fn finish(
        &self,
        plugin_id: &str,
        op: &str,
        started_at_unix_ms: u128,
        outcome: GitOpStatus,
    ) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        // Find the most recent matching entry that's still open.
        let now = unix_now_ms();
        for entry in g.iter_mut().rev() {
            if entry.finished_at_unix_ms.is_none()
                && entry.plugin_id == plugin_id
                && entry.op == op
                && entry.started_at_unix_ms == started_at_unix_ms
            {
                entry.finished_at_unix_ms = Some(now);
                entry.outcome = Some(outcome);
                return;
            }
        }
    }

    pub fn entries(&self) -> Vec<PendingGitWrite> {
        self.inner
            .lock()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }
}

fn unix_now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// The Git write operations the dispatcher can perform. Maps 1:1 to the
/// `leviathan.git.*` Lua functions and to a [`Capability::GitWrite`]
/// op.
#[derive(Debug, Clone)]
pub enum GitOpRequest {
    Checkout {
        ref_name: String,
    },
    CreateBranch {
        name: String,
        start_point: Option<String>,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    CreateTag {
        name: String,
        target: Option<String>,
    },
    DeleteTag {
        name: String,
    },
    Commit {
        message: String,
    },
    StashPush {
        message: Option<String>,
    },
    StashPop {
        index: usize,
    },
    Reset {
        ref_name: String,
        mode: ResetMode,
    },
    Fetch {
        remote: Option<String>,
    },
    Push {
        remote: Option<String>,
        ref_name: Option<String>,
    },
    Merge {
        ref_name: String,
    },
    Rebase {
        ref_name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

impl ResetMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Mixed => "mixed",
            Self::Hard => "hard",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "soft" => Some(Self::Soft),
            "mixed" => Some(Self::Mixed),
            "hard" => Some(Self::Hard),
            _ => None,
        }
    }
}

impl GitOpRequest {
    pub fn op_name(&self) -> &'static str {
        match self {
            Self::Checkout { .. } => "checkout",
            Self::CreateBranch { .. } => "create_branch",
            Self::DeleteBranch { .. } => "delete_branch",
            Self::CreateTag { .. } => "create_tag",
            Self::DeleteTag { .. } => "delete_tag",
            Self::Commit { .. } => "commit",
            Self::StashPush { .. } => "stash_push",
            Self::StashPop { .. } => "stash_pop",
            Self::Reset { .. } => "reset",
            Self::Fetch { .. } => "fetch",
            Self::Push { .. } => "push",
            Self::Merge { .. } => "merge",
            Self::Rebase { .. } => "rebase",
        }
    }

    /// String-form `git:write:<op>` capability the host checks before
    /// running this request. Mirrors
    /// [`git_leviathan_plugin_api::capability::GitWriteOp`].
    pub fn capability(&self) -> &'static str {
        match self {
            Self::Checkout { .. } => "git:write:checkout",
            Self::CreateBranch { .. } | Self::DeleteBranch { .. } => "git:write:branch",
            Self::CreateTag { .. } | Self::DeleteTag { .. } => "git:write:tag",
            Self::Commit { .. } => "git:write:commit",
            Self::StashPush { .. } | Self::StashPop { .. } => "git:write:stash",
            Self::Reset { .. } => "git:write:reset",
            Self::Fetch { .. } => "git:write:fetch",
            Self::Push { .. } => "git:write:push",
            Self::Merge { .. } => "git:write:merge",
            Self::Rebase { .. } => "git:write:rebase",
        }
    }

    /// True when the request is destructive (history-rewriting, hard
    /// data loss, or otherwise hard-to-undo). The dispatcher consults
    /// [`DestructiveConfirmPolicy`] for these before executing.
    pub fn destructive(&self) -> bool {
        match self {
            Self::DeleteBranch { force, .. } => *force,
            Self::DeleteTag { .. } => true,
            Self::Reset { mode, .. } => matches!(mode, ResetMode::Hard),
            Self::Merge { .. } | Self::Rebase { .. } => true,
            _ => false,
        }
    }

    /// JSON summary of the request. Used by audit + devtools.
    pub fn args_json(&self) -> serde_json::Value {
        match self {
            Self::Checkout { ref_name } => serde_json::json!({ "ref": ref_name }),
            Self::CreateBranch { name, start_point } => {
                serde_json::json!({ "name": name, "start_point": start_point })
            }
            Self::DeleteBranch { name, force } => {
                serde_json::json!({ "name": name, "force": force })
            }
            Self::CreateTag { name, target } => {
                serde_json::json!({ "name": name, "target": target })
            }
            Self::DeleteTag { name } => serde_json::json!({ "name": name }),
            Self::Commit { message } => serde_json::json!({ "message": message }),
            Self::StashPush { message } => serde_json::json!({ "message": message }),
            Self::StashPop { index } => serde_json::json!({ "index": index }),
            Self::Reset { ref_name, mode } => {
                serde_json::json!({ "ref": ref_name, "mode": mode.as_str() })
            }
            Self::Fetch { remote } => serde_json::json!({ "remote": remote }),
            Self::Push { remote, ref_name } => {
                serde_json::json!({ "remote": remote, "ref": ref_name })
            }
            Self::Merge { ref_name } => serde_json::json!({ "ref": ref_name }),
            Self::Rebase { ref_name } => serde_json::json!({ "ref": ref_name }),
        }
    }
}

/// Outcome of a successful git op, indicating which typed autocmd events
/// event the host should fire (or `None` for ops that don't fit one).
pub struct GitOpEvents {
    pub fires: Vec<(&'static str, EventPayload)>,
}

impl GitOpEvents {
    pub fn empty() -> Self {
        Self { fires: Vec::new() }
    }

    pub fn one(name: &'static str, payload: EventPayload) -> Self {
        Self {
            fires: vec![(name, payload)],
        }
    }
}

/// Per-host destructive-action policy. The headless default rejects
/// every destructive op until the host or a test driver explicitly
/// flips a per-op approval. the Git API ships the headless surface; the
/// visible UI overlay is deferred (devtools already covers visible
/// security UI for capability prompts; the destructive prompt overlay
/// will mount on the same surface).
#[derive(Clone, Default)]
pub struct DestructiveConfirmPolicy {
    inner: Arc<Mutex<DestructiveConfirmInner>>,
}

#[derive(Default)]
struct DestructiveConfirmInner {
    /// (plugin_id, op) -> approved_count. Each approval is single-use:
    /// `consume_approval` decrements (and removes when zero) so an
    /// approved request is consumed by exactly one op.
    approvals: std::collections::HashMap<(String, String), u32>,
    /// Recorded prompt history so devtools / tests can verify a
    /// destructive op was offered for confirmation.
    history: Vec<DestructivePromptRecord>,
}

#[derive(Debug, Clone)]
pub struct DestructivePromptRecord {
    pub plugin_id: String,
    pub op: String,
    pub args_json: serde_json::Value,
    pub timestamp_unix_ms: u128,
    pub approved: bool,
    /// Free-form attribution (`"user"`, `"test"`, `"<auto>"`). Audit
    /// surfaces this as `confirmed_by`.
    pub confirmed_by: String,
}

impl DestructiveConfirmPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-approve the next destructive op of `op` for `plugin_id`.
    pub fn approve_next(
        &self,
        plugin_id: impl Into<String>,
        op: impl Into<String>,
        confirmed_by: impl Into<String>,
        args_json: serde_json::Value,
    ) {
        let plugin_id = plugin_id.into();
        let op = op.into();
        let confirmed_by = confirmed_by.into();
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        *g.approvals
            .entry((plugin_id.clone(), op.clone()))
            .or_insert(0) += 1;
        g.history.push(DestructivePromptRecord {
            plugin_id,
            op,
            args_json,
            timestamp_unix_ms: unix_now_ms(),
            approved: true,
            confirmed_by,
        });
    }

    /// Consume one approval. Returns the `confirmed_by` attribution
    /// when approval was granted, or `None` when the op should be
    /// blocked.
    pub fn consume_approval(
        &self,
        plugin_id: &str,
        op: &str,
        args_json: &serde_json::Value,
    ) -> Option<String> {
        let Ok(mut g) = self.inner.lock() else {
            return None;
        };
        let key = (plugin_id.to_string(), op.to_string());
        let count = g.approvals.entry(key.clone()).or_insert(0);
        if *count == 0 {
            g.history.push(DestructivePromptRecord {
                plugin_id: plugin_id.to_string(),
                op: op.to_string(),
                args_json: args_json.clone(),
                timestamp_unix_ms: unix_now_ms(),
                approved: false,
                confirmed_by: String::new(),
            });
            return None;
        }
        *count -= 1;
        if *count == 0 {
            g.approvals.remove(&key);
        }
        // Look back for the most recent approving record of this
        // (plugin_id, op) so we can surface the same `confirmed_by`
        // string the approver provided.
        let confirmed_by = g
            .history
            .iter()
            .rev()
            .find(|h| h.plugin_id == plugin_id && h.op == op && h.approved)
            .map(|h| h.confirmed_by.clone())
            .unwrap_or_else(|| "<auto>".to_string());
        g.history.push(DestructivePromptRecord {
            plugin_id: plugin_id.to_string(),
            op: op.to_string(),
            args_json: args_json.clone(),
            timestamp_unix_ms: unix_now_ms(),
            approved: true,
            confirmed_by: confirmed_by.clone(),
        });
        Some(confirmed_by)
    }

    pub fn history(&self) -> Vec<DestructivePromptRecord> {
        self.inner
            .lock()
            .map(|g| g.history.clone())
            .unwrap_or_default()
    }
}

#[derive(Clone, Default)]
pub struct ActiveRepositoryGateway {
    inner: Arc<Mutex<Option<SharedRepositoryGateway>>>,
}

impl ActiveRepositoryGateway {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, gateway: Option<SharedRepositoryGateway>) {
        if let Ok(mut g) = self.inner.lock() {
            *g = gateway;
        }
    }

    pub fn get(&self) -> Option<SharedRepositoryGateway> {
        self.inner.lock().ok().and_then(|g| g.clone())
    }
}

/// All shared state the Lua git/repo APIs need. Cheap-clone — every
/// member is `Arc`-backed.
#[derive(Clone)]
pub struct GitOpsContext {
    pub gateway: ActiveRepositoryGateway,
    pub destructive: DestructiveConfirmPolicy,
    pub pending: PendingGitWrites,
    pub audit: AuditLog,
    pub diagnostics: DiagnosticStore,
}

pub struct GitOpExecution {
    pub error: Option<String>,
    pub events: GitOpEvents,
}

impl GitOpsContext {
    /// Single entry point. Validates capability + destructive policy,
    /// records audit + diagnostic, runs the op against the gateway,
    /// and returns the resulting events for the host to fire.
    pub fn execute(
        &self,
        plugin_id: &str,
        generation_id: GenerationId,
        request: GitOpRequest,
        cap_check: impl FnOnce(&str) -> Result<(), String>,
    ) -> GitOpExecution {
        let op = request.op_name().to_string();
        let cap = request.capability();
        let args_json = request.args_json();
        let started_at = unix_now_ms();

        // Push the pending entry so devtools can observe in-flight
        // writes immediately.
        self.pending.push(PendingGitWrite {
            plugin_id: plugin_id.to_string(),
            generation_id: generation_id.get(),
            op: op.clone(),
            args_json: args_json.clone(),
            started_at_unix_ms: started_at,
            finished_at_unix_ms: None,
            outcome: None,
        });

        // Capability check. The supplied closure already audits +
        // diagnoses on failure (capability guard); we just need to
        // record the git_write audit row + the repository Git diagnostic.
        if let Err(reason) = cap_check(cap) {
            self.record_audit(plugin_id, &op, AuditOutcome::Denied, "denied:capability");
            self.record_diagnostic(
                plugin_id,
                generation_id,
                "git.destructive_blocked",
                DiagnosticSeverity::Error,
                &op,
                format!("git.{op} denied: capability `{cap}` not granted ({reason})"),
                serde_json::json!({
                    "op": op,
                    "capability": cap,
                    "reason": reason,
                    "args": args_json,
                }),
            );
            self.pending
                .finish(plugin_id, &op, started_at, GitOpStatus::CapabilityDenied);
            return GitOpExecution {
                error: Some(format!("capability denied: {cap}")),
                events: GitOpEvents::empty(),
            };
        }

        // Destructive policy gate.
        let confirmed_by = if request.destructive() {
            match self
                .destructive
                .consume_approval(plugin_id, &op, &args_json)
            {
                Some(by) => Some(by),
                None => {
                    self.record_audit(plugin_id, &op, AuditOutcome::Denied, "denied:destructive");
                    self.record_diagnostic(
                        plugin_id,
                        generation_id,
                        "git.destructive_blocked",
                        DiagnosticSeverity::Error,
                        &op,
                        format!(
                            "git.{op} blocked: destructive op requires confirmation policy approval"
                        ),
                        serde_json::json!({
                            "op": op,
                            "args": args_json,
                        }),
                    );
                    self.pending.finish(
                        plugin_id,
                        &op,
                        started_at,
                        GitOpStatus::DestructiveBlocked,
                    );
                    return GitOpExecution {
                        error: Some(format!("destructive op `{op}` requires confirmation")),
                        events: GitOpEvents::empty(),
                    };
                }
            }
        } else {
            None
        };

        // Resolve the gateway. With no repository open every op is a
        // no-op error.
        let Some(gateway) = self.gateway.get() else {
            self.record_audit(plugin_id, &op, AuditOutcome::Denied, "denied:no_repository");
            self.record_diagnostic(
                plugin_id,
                generation_id,
                "git.task_failed",
                DiagnosticSeverity::Error,
                &op,
                format!("git.{op} failed: no repository open"),
                serde_json::json!({
                    "op": op,
                    "args": args_json,
                    "cause": "no_repository",
                }),
            );
            self.pending
                .finish(plugin_id, &op, started_at, GitOpStatus::NoRepository);
            return GitOpExecution {
                error: Some("no repository open".to_string()),
                events: GitOpEvents::empty(),
            };
        };

        let started_instant = Instant::now();
        let result = run_request(&request, &gateway);
        let duration_ms = started_instant.elapsed().as_millis();

        match result {
            Ok(events) => {
                let outcome_json = serde_json::json!({
                    "op": op,
                    "args": args_json,
                    "duration_ms": duration_ms,
                    "confirmed_by": confirmed_by,
                });
                self.record_audit(
                    plugin_id,
                    &op,
                    AuditOutcome::Allowed,
                    &format!("ok:{}ms", duration_ms),
                );
                self.record_diagnostic(
                    plugin_id,
                    generation_id,
                    "git.task_succeeded",
                    DiagnosticSeverity::Info,
                    &op,
                    format!("git.{op} completed in {duration_ms}ms"),
                    outcome_json,
                );
                self.pending
                    .finish(plugin_id, &op, started_at, GitOpStatus::Succeeded);
                GitOpExecution {
                    error: None,
                    events,
                }
            }
            Err(err) => {
                let cause = err.clone();
                self.record_audit(
                    plugin_id,
                    &op,
                    AuditOutcome::Denied,
                    &format!("failed:{cause}"),
                );
                self.record_diagnostic(
                    plugin_id,
                    generation_id,
                    "git.task_failed",
                    DiagnosticSeverity::Error,
                    &op,
                    format!("git.{op} failed: {cause}"),
                    serde_json::json!({
                        "op": op,
                        "args": args_json,
                        "duration_ms": duration_ms,
                        "cause": cause,
                    }),
                );
                self.pending
                    .finish(plugin_id, &op, started_at, GitOpStatus::Failed);
                GitOpExecution {
                    error: Some(err),
                    events: GitOpEvents::empty(),
                }
            }
        }
    }

    fn record_audit(&self, plugin_id: &str, op: &str, outcome: AuditOutcome, target: &str) {
        self.audit
            .record(plugin_id, format!("git_write.{op}"), target, outcome);
    }

    fn record_diagnostic(
        &self,
        plugin_id: &str,
        generation_id: GenerationId,
        code: &str,
        severity: DiagnosticSeverity,
        op: &str,
        message: String,
        context: serde_json::Value,
    ) {
        self.diagnostics.record(
            PluginDiagnostic::new(PluginId::from(plugin_id), severity, code, message)
                .with_generation(generation_id)
                .with_source(PluginSourceSpan::ApiFunction {
                    name: format!("git.{op}"),
                })
                .with_context(context),
        );
    }
}

fn run_request(
    request: &GitOpRequest,
    gateway: &SharedRepositoryGateway,
) -> Result<GitOpEvents, String> {
    use crate::services::ResetMode as ServiceResetMode;
    match request {
        GitOpRequest::Checkout { ref_name } => {
            let snapshot = gateway
                .checkout_branch(ref_name)
                .map_err(|e| e.to_string())?;
            let mut payload = EventPayload::new();
            payload.insert(
                "hash".into(),
                serde_json::Value::String(snapshot.head_hash.clone().unwrap_or_default()),
            );
            Ok(GitOpEvents {
                fires: vec![("HeadChanged", payload)],
            })
        }
        GitOpRequest::CreateBranch { name, start_point } => {
            let target = start_point
                .clone()
                .or_else(|| {
                    // Resolve HEAD when start_point omitted. Use the
                    // gateway's load_repo so we don't reach into git2
                    // directly.
                    gateway
                        .load_repo(crate::services::COMMIT_LOAD_LIMIT)
                        .ok()
                        .and_then(|s| s.head_hash)
                })
                .unwrap_or_else(|| "HEAD".to_string());
            let snapshot = gateway
                .create_branch_at_commit(name, &target)
                .map_err(|e| e.to_string())?;
            let mut refs_payload = EventPayload::new();
            refs_payload.insert(
                "count".into(),
                serde_json::Value::Number(serde_json::Number::from(snapshot.refs.len() as u64)),
            );
            let mut branch_payload = EventPayload::new();
            branch_payload.insert("name".into(), serde_json::Value::String(name.clone()));
            branch_payload.insert(
                "head_hash".into(),
                serde_json::Value::String(snapshot.head_hash.clone().unwrap_or_default()),
            );
            Ok(GitOpEvents {
                fires: vec![
                    ("RefsChanged", refs_payload),
                    ("BranchChanged", branch_payload),
                ],
            })
        }
        GitOpRequest::DeleteBranch { name, .. } => {
            let snapshot = gateway
                .delete_branch(name, false)
                .map_err(|e| e.to_string())?;
            let mut refs_payload = EventPayload::new();
            refs_payload.insert(
                "count".into(),
                serde_json::Value::Number(serde_json::Number::from(snapshot.refs.len() as u64)),
            );
            Ok(GitOpEvents::one("RefsChanged", refs_payload))
        }
        GitOpRequest::CreateTag { name, target } => {
            let target_hash = match target {
                Some(t) => t.clone(),
                None => gateway
                    .load_repo(crate::services::COMMIT_LOAD_LIMIT)
                    .map_err(|e| e.to_string())?
                    .head_hash
                    .ok_or_else(|| "no HEAD to tag".to_string())?,
            };
            let snapshot = gateway
                .create_tag(name, &target_hash)
                .map_err(|e| e.to_string())?;
            let mut refs_payload = EventPayload::new();
            refs_payload.insert(
                "count".into(),
                serde_json::Value::Number(serde_json::Number::from(snapshot.refs.len() as u64)),
            );
            Ok(GitOpEvents::one("RefsChanged", refs_payload))
        }
        GitOpRequest::DeleteTag { name } => {
            let snapshot = gateway.delete_tag(name).map_err(|e| e.to_string())?;
            let mut refs_payload = EventPayload::new();
            refs_payload.insert(
                "count".into(),
                serde_json::Value::Number(serde_json::Number::from(snapshot.refs.len() as u64)),
            );
            Ok(GitOpEvents::one("RefsChanged", refs_payload))
        }
        GitOpRequest::Commit { message } => {
            let snapshot = gateway
                .commit_dirty_changes(message, "")
                .map_err(|e| e.to_string())?;
            let mut head_payload = EventPayload::new();
            head_payload.insert(
                "hash".into(),
                serde_json::Value::String(snapshot.head_hash.clone().unwrap_or_default()),
            );
            let mut commit_payload = EventPayload::new();
            commit_payload.insert(
                "count".into(),
                serde_json::Value::Number(serde_json::Number::from(snapshot.commits.len() as u64)),
            );
            Ok(GitOpEvents {
                fires: vec![
                    ("HeadChanged", head_payload),
                    ("CommitListChanged", commit_payload),
                ],
            })
        }
        GitOpRequest::StashPush { .. } => {
            let _ = gateway.create_stash().map_err(|e| e.to_string())?;
            Ok(GitOpEvents::empty())
        }
        GitOpRequest::StashPop { index } => {
            let _ = gateway.pop_stash(*index).map_err(|e| e.to_string())?;
            Ok(GitOpEvents::empty())
        }
        GitOpRequest::Reset { ref_name, mode } => {
            let svc_mode = match mode {
                ResetMode::Soft => ServiceResetMode::Soft,
                ResetMode::Mixed => ServiceResetMode::Mixed,
                ResetMode::Hard => ServiceResetMode::Hard,
            };
            let snapshot = gateway
                .reset_current_branch_to_commit(ref_name, svc_mode)
                .map_err(|e| e.to_string())?;
            let mut head_payload = EventPayload::new();
            head_payload.insert(
                "hash".into(),
                serde_json::Value::String(snapshot.head_hash.clone().unwrap_or_default()),
            );
            Ok(GitOpEvents::one("HeadChanged", head_payload))
        }
        GitOpRequest::Fetch { .. } => {
            let mut started_payload = EventPayload::new();
            started_payload.insert(
                "count".into(),
                serde_json::Value::Number(serde_json::Number::from(0u64)),
            );
            // Fire the started event eagerly via the returned event
            // list so the host emits it before completion. We collect
            // started + finished into the same fire list because
            // execution is currently synchronous (synchronous Git API compromise).
            let result = gateway.fetch_remotes();
            let mut finished_payload = EventPayload::new();
            match result {
                Ok(()) => {
                    finished_payload.insert("ok".into(), serde_json::Value::Bool(true));
                    let snapshot = gateway
                        .load_refs_snapshot(crate::services::COMMIT_LOAD_LIMIT)
                        .map_err(|e| e.to_string())?;
                    let mut refs_payload = EventPayload::new();
                    refs_payload.insert(
                        "count".into(),
                        serde_json::Value::Number(serde_json::Number::from(
                            snapshot.refs.len() as u64
                        )),
                    );
                    Ok(GitOpEvents {
                        fires: vec![
                            ("FetchStarted", started_payload),
                            ("FetchFinished", finished_payload),
                            ("RefsChanged", refs_payload),
                        ],
                    })
                }
                Err(e) => Err(e.to_string()),
            }
        }
        GitOpRequest::Push { .. } => {
            let mut started_payload = EventPayload::new();
            started_payload.insert(
                "count".into(),
                serde_json::Value::Number(serde_json::Number::from(0u64)),
            );
            let push_result = gateway.push_current_branch();
            let mut finished_payload = EventPayload::new();
            match push_result {
                Ok(_) => {
                    finished_payload.insert("ok".into(), serde_json::Value::Bool(true));
                    Ok(GitOpEvents {
                        fires: vec![
                            ("PushStarted", started_payload),
                            ("PushFinished", finished_payload),
                        ],
                    })
                }
                Err(e) => Err(e.to_string()),
            }
        }
        GitOpRequest::Merge { ref_name } => {
            // The gateway exposes branch-into-branch merges; project
            // "merge ref into current" by reading the current branch
            // from a snapshot.
            let snapshot = gateway
                .load_repo(crate::services::COMMIT_LOAD_LIMIT)
                .map_err(|e| e.to_string())?;
            let target = snapshot
                .current_branch
                .clone()
                .ok_or_else(|| "no current branch".to_string())?;
            let _ = gateway
                .merge_branch_into(ref_name, &target)
                .map_err(|e| e.to_string())?;
            let post = gateway
                .load_repo(crate::services::COMMIT_LOAD_LIMIT)
                .map_err(|e| e.to_string())?;
            let mut head_payload = EventPayload::new();
            head_payload.insert(
                "hash".into(),
                serde_json::Value::String(post.head_hash.clone().unwrap_or_default()),
            );
            Ok(GitOpEvents::one("HeadChanged", head_payload))
        }
        GitOpRequest::Rebase { ref_name } => {
            let snapshot = gateway
                .load_repo(crate::services::COMMIT_LOAD_LIMIT)
                .map_err(|e| e.to_string())?;
            let source = snapshot
                .current_branch
                .clone()
                .ok_or_else(|| "no current branch".to_string())?;
            let _ = gateway
                .rebase_current_onto(&source, ref_name)
                .map_err(|e| e.to_string())?;
            let post = gateway
                .load_repo(crate::services::COMMIT_LOAD_LIMIT)
                .map_err(|e| e.to_string())?;
            let mut head_payload = EventPayload::new();
            head_payload.insert(
                "hash".into(),
                serde_json::Value::String(post.head_hash.clone().unwrap_or_default()),
            );
            Ok(GitOpEvents::one("HeadChanged", head_payload))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_writes_caps_at_32() {
        let pending = PendingGitWrites::new();
        for i in 0..50 {
            pending.push(PendingGitWrite {
                plugin_id: format!("p{i}"),
                generation_id: 1,
                op: "checkout".into(),
                args_json: serde_json::json!({}),
                started_at_unix_ms: 0,
                finished_at_unix_ms: None,
                outcome: None,
            });
        }
        assert_eq!(pending.entries().len(), PendingGitWrites::CAP);
    }

    #[test]
    fn destructive_policy_consumes_one_approval() {
        let pol = DestructiveConfirmPolicy::new();
        pol.approve_next("p", "reset", "tester", serde_json::json!({}));
        assert_eq!(
            pol.consume_approval("p", "reset", &serde_json::json!({})),
            Some("tester".to_string())
        );
        assert!(pol
            .consume_approval("p", "reset", &serde_json::json!({}))
            .is_none());
    }

    #[test]
    fn destructive_policy_records_history() {
        let pol = DestructiveConfirmPolicy::new();
        let _ = pol.consume_approval("p", "merge", &serde_json::json!({"ref": "x"}));
        let history = pol.history();
        assert_eq!(history.len(), 1);
        assert!(!history[0].approved);
    }

    #[test]
    fn request_destructive_classification() {
        assert!(GitOpRequest::DeleteBranch {
            name: "x".into(),
            force: true,
        }
        .destructive());
        assert!(!GitOpRequest::DeleteBranch {
            name: "x".into(),
            force: false,
        }
        .destructive());
        assert!(GitOpRequest::DeleteTag { name: "v1".into() }.destructive());
        assert!(GitOpRequest::Reset {
            ref_name: "HEAD~1".into(),
            mode: ResetMode::Hard,
        }
        .destructive());
        assert!(!GitOpRequest::Reset {
            ref_name: "HEAD~1".into(),
            mode: ResetMode::Mixed,
        }
        .destructive());
        assert!(GitOpRequest::Merge {
            ref_name: "x".into()
        }
        .destructive());
        assert!(GitOpRequest::Rebase {
            ref_name: "x".into()
        }
        .destructive());
        assert!(!GitOpRequest::Checkout {
            ref_name: "x".into()
        }
        .destructive());
    }
}

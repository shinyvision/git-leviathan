//! Phase 11 acceptance tests: typed repository + git APIs.
//!
//! Each test cites the acceptance-criterion bullet from the plan
//! (`PLUGIN_REFACTOR.md` Phase 11, lines ~668-672):
//!
//! - Normal plugin Git reads do not require process spawn — exercised
//!   via the `SharedRepositoryGateway` trait, never a subprocess.
//! - Git writes go through the same task and message flow as built-in
//!   UI actions — both land on the gateway trait.
//! - Destructive Git writes require explicit capability and
//!   confirmation policy — see `reset_hard_blocked_until_policy_approves`.
//!
//! The tests use a real on-disk fixture repo (built with `git2` via
//! `init_test_repo`) so the gateway path is the same the production
//! UI takes.

use crate::plugin::audit::AuditOutcome;
use crate::plugin::git_ops::GitOpStatus;
use crate::plugin::tests::harness::MockHost;
use crate::services::gateway::GitRepositoryGateway;
use crate::services::test_support::{create_branch, init_test_repo};

fn manifest_with_caps(id: &str, caps: &[&str]) -> String {
    let cap_array = caps
        .iter()
        .map(|c| format!("{c:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"
id = "{id}"
name = "{id}"
version = "0.1.0"
api_version = "1.0"
capabilities = [{cap_array}]
"#
    )
}

#[test]
fn repository_status_denied_without_capability() {
    let mut host = MockHost::new();
    host.load_inline(
        "no_status",
        &manifest_with_caps("no_status", &[]),
        r#"
        leviathan.api.schedule(function()
            local t, err = leviathan.repository.status()
            _G.status_ok = (t ~= nil) and 1 or 0
            _G.status_err = err or ""
        end)
        "#,
    )
    .expect("load");

    let (_repo_dir, _repo) = init_test_repo("phase11_status_denied");
    let gw = GitRepositoryGateway::from_path(_repo_dir.path_str());
    host.host_mut().set_repository_gateway(Some(gw));
    host.tick();

    assert_eq!(host.read_global_i64("no_status", "status_ok"), Some(0));
    let err = host
        .read_global_string("no_status", "status_err")
        .unwrap_or_default();
    assert!(
        err.contains("capability denied"),
        "expected capability-denied error; got `{err}`"
    );
}

#[test]
fn repository_status_returns_table_with_capability() {
    let mut host = MockHost::new();
    host.load_inline(
        "ok_status",
        &manifest_with_caps("ok_status", &["git:read:status"]),
        r#"
        leviathan.api.schedule(function()
            local t, err = leviathan.repository.status()
            _G.status_ok = (t ~= nil) and 1 or 0
            _G.status_err = err or ""
            if t then
                _G.has_staged = (type(t.staged) == "table") and 1 or 0
                _G.has_unstaged = (type(t.unstaged) == "table") and 1 or 0
                _G.has_conflicted = (type(t.conflicted) == "table") and 1 or 0
            end
        end)
        "#,
    )
    .expect("load");

    let (_repo_dir, _repo) = init_test_repo("phase11_status_ok");
    let gw = GitRepositoryGateway::from_path(_repo_dir.path_str());
    host.host_mut().set_repository_gateway(Some(gw));
    host.tick();

    let err = host
        .read_global_string("ok_status", "status_err")
        .unwrap_or_default();
    assert_eq!(
        host.read_global_i64("ok_status", "status_ok"),
        Some(1),
        "err: {err}"
    );
    assert_eq!(host.read_global_i64("ok_status", "has_staged"), Some(1));
    assert_eq!(host.read_global_i64("ok_status", "has_unstaged"), Some(1));
    assert_eq!(host.read_global_i64("ok_status", "has_conflicted"), Some(1));
}

#[test]
fn checkout_with_capability_moves_head_and_fires_head_changed() {
    let mut host = MockHost::new();
    host.load_inline(
        "checkout_ok",
        &manifest_with_caps("checkout_ok", &["git:write:checkout"]),
        r#"
        _G.head_changed_fires = 0
        _G.head_changed_hash = ""
        leviathan.autocmd.create("HeadChanged", {
            callback = function(ev)
                _G.head_changed_fires = _G.head_changed_fires + 1
                _G.head_changed_hash = ev.payload.hash or ""
            end,
        })
        leviathan.api.schedule(function()
            local ok, err = leviathan.git.checkout({ ref = "feature" })
            _G.checkout_ok = ok and 1 or 0
            _G.checkout_err = err or ""
        end)
        "#,
    )
    .expect("load");

    let (repo_dir, repo) = init_test_repo("phase11_checkout_head");
    create_branch(&repo, "feature");
    let gw = GitRepositoryGateway::from_path(repo_dir.path_str());
    host.host_mut().set_repository_gateway(Some(gw));
    // First tick: deferred queue resumes the schedule callback, which
    // calls leviathan.git.checkout and queues HeadChanged. Second tick:
    // flush_pending_git_events fires HeadChanged into autocmds.
    host.tick();
    host.tick();

    let err = host
        .read_global_string("checkout_ok", "checkout_err")
        .unwrap_or_default();
    assert_eq!(
        host.read_global_i64("checkout_ok", "checkout_ok"),
        Some(1),
        "checkout failed: `{err}`"
    );
    assert_eq!(
        host.read_global_i64("checkout_ok", "head_changed_fires"),
        Some(1),
        "HeadChanged should fire exactly once after checkout"
    );
    let post_hash = host
        .read_global_string("checkout_ok", "head_changed_hash")
        .unwrap_or_default();
    assert!(
        !post_hash.is_empty(),
        "HeadChanged payload should carry a non-empty hash"
    );

    // The git2-side repo agrees we're on `feature` now.
    let head_ref = repo.head().unwrap();
    let head_name = head_ref.shorthand().unwrap_or("");
    assert_eq!(head_name, "feature");
}

#[test]
fn reset_hard_blocked_until_policy_approves() {
    let mut host = MockHost::new();
    let (repo_dir, repo) = init_test_repo("phase11_reset_hard");
    let head_hash = repo.head().unwrap().target().unwrap().to_string();
    host.load_inline(
        "reset_hard",
        &manifest_with_caps("reset_hard", &["git:write:reset"]),
        &format!(
            r#"
        _G.first_ok = -1
        _G.first_err = ""
        _G.second_ok = -1
        _G.second_err = ""
        leviathan.api.create_user_command("do_reset", function()
            local ok, err = leviathan.git.reset({{ ref = "{head_hash}", mode = "hard" }})
            if _G.first_ok == -1 then
                _G.first_ok = ok and 1 or 0
                _G.first_err = err or ""
            else
                _G.second_ok = ok and 1 or 0
                _G.second_err = err or ""
            end
        end)
        "#
        ),
    )
    .expect("load");

    let gw = GitRepositoryGateway::from_path(repo_dir.path_str());
    host.host_mut().set_repository_gateway(Some(gw));

    // Attempt 1: no approval → blocked by destructive policy.
    host.invoke_user_command("reset_hard", "do_reset")
        .expect("invoke");
    assert_eq!(
        host.read_global_i64("reset_hard", "first_ok"),
        Some(0),
        "destructive reset must be blocked by default"
    );
    let first_err = host
        .read_global_string("reset_hard", "first_err")
        .unwrap_or_default();
    assert!(
        first_err.contains("requires confirmation"),
        "expected destructive-confirmation error; got `{first_err}`"
    );

    // Approve the next reset, then re-invoke: it now goes through.
    host.host().destructive_policy().approve_next(
        "reset_hard",
        "reset",
        "test",
        serde_json::json!({}),
    );
    host.invoke_user_command("reset_hard", "do_reset")
        .expect("invoke 2");
    let second_err = host
        .read_global_string("reset_hard", "second_err")
        .unwrap_or_default();
    assert_eq!(
        host.read_global_i64("reset_hard", "second_ok"),
        Some(1),
        "approved reset should succeed; err: `{second_err}`"
    );

    // Policy history records both the rejection and the consumed
    // approval — surfaces `args_json` + `timestamp_unix_ms` so they
    // aren't dead.
    let history = host.host().destructive_policy().history();
    assert!(
        history.iter().any(|h| !h.approved && h.op == "reset"),
        "policy history must record the blocked attempt"
    );
    let approved = history
        .iter()
        .find(|h| h.approved && h.op == "reset" && h.confirmed_by == "test")
        .expect("approval recorded");
    assert!(
        approved.timestamp_unix_ms > 0,
        "approval record must carry a timestamp"
    );
    assert_eq!(approved.args_json, serde_json::json!({}));
}

#[test]
fn checkout_without_capability_denied_and_audited() {
    let mut host = MockHost::new();
    host.load_inline(
        "no_checkout",
        &manifest_with_caps("no_checkout", &[]),
        r#"
        leviathan.api.schedule(function()
            local ok, err = leviathan.git.checkout({ ref = "feature" })
            _G.checkout_ok = ok and 1 or 0
            _G.checkout_err = err or ""
        end)
        "#,
    )
    .expect("load");

    let (repo_dir, repo) = init_test_repo("phase11_checkout_denied");
    create_branch(&repo, "feature");
    let gw = GitRepositoryGateway::from_path(repo_dir.path_str());
    host.host_mut().set_repository_gateway(Some(gw));
    host.tick();

    assert_eq!(host.read_global_i64("no_checkout", "checkout_ok"), Some(0));
    let err = host
        .read_global_string("no_checkout", "checkout_err")
        .unwrap_or_default();
    assert!(
        err.contains("capability denied"),
        "expected capability-denied error; got `{err}`"
    );

    // Audit log records the denied write under `git_write.checkout`.
    let entries = host.host().audit_log().entries();
    let denied = entries.iter().find(|e| {
        e.plugin_id == "no_checkout"
            && e.capability == "git_write.checkout"
            && e.outcome == AuditOutcome::Denied
    });
    assert!(
        denied.is_some(),
        "audit log must record git_write.checkout denial; got entries: {:?}",
        entries
            .iter()
            .map(|e| (e.plugin_id.clone(), e.capability.clone(), e.outcome.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn allowed_write_records_audit_entry() {
    let mut host = MockHost::new();
    host.load_inline(
        "audit_ok",
        &manifest_with_caps("audit_ok", &["git:write:branch"]),
        r#"
        leviathan.api.schedule(function()
            local ok, err = leviathan.git.create_branch({ name = "topic" })
            _G.create_ok = ok and 1 or 0
            _G.create_err = err or ""
        end)
        "#,
    )
    .expect("load");

    let (repo_dir, _repo) = init_test_repo("phase11_audit_ok");
    let gw = GitRepositoryGateway::from_path(repo_dir.path_str());
    host.host_mut().set_repository_gateway(Some(gw));
    host.tick();

    assert_eq!(
        host.read_global_i64("audit_ok", "create_ok"),
        Some(1),
        "create_branch should succeed; err: `{}`",
        host.read_global_string("audit_ok", "create_err")
            .unwrap_or_default()
    );

    let allowed = host.host().audit_log().entries().into_iter().find(|e| {
        e.plugin_id == "audit_ok"
            && e.capability == "git_write.create_branch"
            && e.outcome == AuditOutcome::Allowed
    });
    assert!(
        allowed.is_some(),
        "audit log must record git_write.create_branch allowed entry"
    );
}

#[test]
fn pending_git_writes_records_op_with_args_and_outcome() {
    let mut host = MockHost::new();
    let (repo_dir, repo) = init_test_repo("phase11_pending");
    let head_hash = repo.head().unwrap().target().unwrap().to_string();
    host.load_inline(
        "pending",
        &manifest_with_caps("pending", &["git:write:branch"]),
        &format!(
            r#"
        leviathan.api.schedule(function()
            leviathan.git.create_branch({{ name = "queued", start_point = "{head_hash}" }})
        end)
        "#
        ),
    )
    .expect("load");

    let gw = GitRepositoryGateway::from_path(repo_dir.path_str());
    host.host_mut().set_repository_gateway(Some(gw));
    host.tick();

    let entries = host.host().pending_git_writes();
    let entry = entries
        .iter()
        .find(|e| e.plugin_id == "pending" && e.op == "create_branch")
        .unwrap_or_else(|| {
            panic!(
                "pending entry should exist; got {:?}",
                entries
                    .iter()
                    .map(|e| (e.plugin_id.clone(), e.op.clone(), e.outcome))
                    .collect::<Vec<_>>()
            )
        });
    assert!(entry.generation_id >= 1);
    assert_eq!(
        entry.args_json.get("name").and_then(|v| v.as_str()),
        Some("queued")
    );
    assert_eq!(
        entry.args_json.get("start_point").and_then(|v| v.as_str()),
        Some(head_hash.as_str())
    );
    assert!(
        entry.finished_at_unix_ms.is_some(),
        "synchronous dispatcher must finish before tick returns"
    );
    assert_eq!(
        entry.outcome,
        Some(GitOpStatus::Succeeded),
        "outcome should be Succeeded; got {:?}",
        entry.outcome
    );
    // Read GitOpStatus::as_str via the Display-equivalent surface so
    // the helper isn't dead.
    assert_eq!(GitOpStatus::Succeeded.as_str(), "succeeded");
}

#[test]
fn pending_git_writes_caps_at_32() {
    // Push 50 entries via repeated rejected ops (no capability; cheap).
    let mut host = MockHost::new();
    host.load_inline(
        "spam",
        &manifest_with_caps("spam", &[]),
        r#"
        leviathan.api.schedule(function()
            for i = 1, 50 do
                leviathan.git.checkout({ ref = "x" .. tostring(i) })
            end
        end)
        "#,
    )
    .expect("load");
    host.tick();

    let entries = host.host().pending_git_writes();
    assert_eq!(
        entries.len(),
        crate::plugin::git_ops::PendingGitWrites::CAP,
        "ring buffer should cap at 32"
    );
}

#[test]
fn no_repository_open_returns_typed_error() {
    let mut host = MockHost::new();
    host.load_inline(
        "no_repo",
        &manifest_with_caps("no_repo", &["git:read:status"]),
        r#"
        leviathan.api.schedule(function()
            local t, err = leviathan.repository.status()
            _G.t_is_nil = (t == nil) and 1 or 0
            _G.err = err or ""
        end)
        "#,
    )
    .expect("load");

    // Deliberately do not set a gateway — the host's
    // ActiveRepositoryGateway stays at its default (None).
    host.host_mut().set_repository_gateway(None);
    host.tick();

    assert_eq!(host.read_global_i64("no_repo", "t_is_nil"), Some(1));
    let err = host
        .read_global_string("no_repo", "err")
        .unwrap_or_default();
    assert!(
        err.contains("no repository open"),
        "expected `no repository open`; got `{err}`"
    );
}

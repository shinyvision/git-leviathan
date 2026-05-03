//! `leviathan.repository` — read-only snapshot of the active repository.
//!
//! Populated by [`PluginHost::sync_repository`](crate::plugin::PluginHost::sync_repository)
//! whenever the host detects the repository changed (HEAD moved, local
//! branch created/deleted, fetch updated remote refs, upstream config
//! changed). Plugins observe changes via the `BranchChanged` autocmd and
//! re-read the globals inside their dynamic widget fn — the idiom is the
//! same one `dancing_banana_test` uses for `FetchStart`/`FetchEnd`.
//!
//! Shape (see the spec brainstormed with the user):
//!
//! ```text
//! leviathan.repository = {
//!   name = "git_leviathan",
//!   workdir_path = "/home/user/projects/git_leviathan", -- "" when no repo open
//!   current_branch_name = "main",           -- display label, "HEAD" when detached
//!   current_branch = <LocalBranch | nil>,   -- nil when HEAD detached
//!   is_open = true,                         -- true when any repository is loaded (bare or with worktree)
//!   is_detached = false,                    -- true when HEAD is at a commit but no local branch is current
//!   is_unborn = false,                      -- true when a repo is open but has zero commits (HEAD points at an unborn branch)
//!   is_bare = false,                        -- true when a repo is open but has no working tree
//!   head_hash = "abc123...",                -- "" when no repo / unborn HEAD
//!   default_remote_name = "origin",         -- "" when no remotes configured
//!   local_branches = { <LocalBranch>, ... },
//!   remote_branches = { <RemoteBranch>, ... },
//!   tags = { <Tag>, ... },
//! }
//! LocalBranch  = { name, hash, is_current, upstream_branch = <RemoteBranch | nil> }
//! RemoteBranch = { name, remote_name, hash }
//! Tag          = { name, hash }
//! ```
//!
//! `current_branch` is the same `LocalBranch` table (same identity) as
//! whichever entry in `local_branches` is marked `is_current` by libgit2.
//! Each `LocalBranch.upstream_branch` is the same `RemoteBranch` table
//! (same Lua identity) as the matching entry in `remote_branches`, so
//! plugins can compare via `==` and caches keyed by reference work.
//! Mutations made by a plugin to the table are not propagated back — the
//! table is rebuilt on every sync.

use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Lua, Table, Value as LuaValue};
use serde_json;

use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::git_ops::GitOpsContext;
use crate::plugin::resources::{GenerationId, PluginId};
use crate::services::{RepoRef, RepoRefKind, COMMIT_LOAD_LIMIT};

/// Build the full `leviathan.repository` table from the latest refs.
///
/// Returns the fresh table to the caller; installing it onto the
/// `leviathan` global is the host's job.
pub fn build_table(
    lua: &Lua,
    repo_name: &str,
    workdir_path: &str,
    current_branch_name: &str,
    head_hash: &str,
    default_remote_name: &str,
    refs: &[RepoRef],
) -> mlua::Result<Table> {
    let locals: Vec<&RepoRef> = refs
        .iter()
        .filter(|r| matches!(r.kind, RepoRefKind::LocalBranch))
        .collect();
    let remotes: Vec<&RepoRef> = refs
        .iter()
        .filter(|r| matches!(r.kind, RepoRefKind::RemoteBranch))
        .collect();
    let tags: Vec<&RepoRef> = refs
        .iter()
        .filter(|r| matches!(r.kind, RepoRefKind::Tag))
        .collect();

    // Build each remote-branch table once and stash it by (remote_name,
    // name). The top-level `remote_branches` list and every local's
    // `upstream_branch` share the same table identity, so Lua-side
    // equality and reference caches behave intuitively.
    let remote_branches_table = lua.create_table()?;
    let mut remote_lookup: HashMap<(String, String), Table> = HashMap::new();
    for (i, remote) in remotes.iter().enumerate() {
        let t = build_remote_branch(lua, remote)?;
        let key = (
            remote.remote_name.clone().unwrap_or_default(),
            remote.name.clone(),
        );
        remote_lookup.insert(key, t.clone());
        remote_branches_table.raw_set(i + 1, t)?;
    }

    // Build each local-branch table once; reuse for `current_branch` so
    // Lua `==` identity holds.
    let local_branches_table = lua.create_table()?;
    let mut current_branch: Option<Table> = None;
    for (i, local) in locals.iter().enumerate() {
        let t = build_local_branch(lua, local, &remote_lookup)?;
        if local.is_current {
            current_branch = Some(t.clone());
        }
        local_branches_table.raw_set(i + 1, t)?;
    }

    let tags_table = lua.create_table()?;
    for (i, tag) in tags.iter().enumerate() {
        tags_table.raw_set(i + 1, build_tag(lua, tag)?)?;
    }

    let repo = lua.create_table()?;
    repo.set("name", repo_name)?;
    repo.set("workdir_path", workdir_path)?;
    repo.set("current_branch_name", current_branch_name)?;
    repo.set("head_hash", head_hash)?;
    repo.set("default_remote_name", default_remote_name)?;
    let is_open = !repo_name.is_empty();
    repo.set("is_open", is_open)?;
    let is_detached = !head_hash.is_empty() && current_branch.is_none();
    repo.set("is_detached", is_detached)?;
    let is_unborn = !workdir_path.is_empty() && head_hash.is_empty();
    repo.set("is_unborn", is_unborn)?;
    let is_bare = !repo_name.is_empty() && workdir_path.is_empty();
    repo.set("is_bare", is_bare)?;
    match current_branch {
        Some(t) => repo.set("current_branch", t)?,
        None => repo.set("current_branch", mlua::Value::Nil)?,
    }
    repo.set("local_branches", local_branches_table)?;
    repo.set("remote_branches", remote_branches_table)?;
    repo.set("tags", tags_table)?;
    Ok(repo)
}

fn build_tag(lua: &Lua, tag: &RepoRef) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("name", tag.name.as_str())?;
    t.set("hash", tag.target_hash.as_str())?;
    Ok(t)
}

fn build_local_branch(
    lua: &Lua,
    local: &RepoRef,
    remote_lookup: &HashMap<(String, String), Table>,
) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("name", local.name.as_str())?;
    t.set("hash", local.target_hash.as_str())?;
    t.set("is_current", local.is_current)?;
    let upstream = local
        .upstream_ref
        .as_deref()
        .and_then(|short| resolve_upstream(short, remote_lookup));
    match upstream {
        Some(remote) => {
            t.set("upstream_branch", remote)?;
        }
        None => {
            t.set("upstream_branch", mlua::Value::Nil)?;
        }
    }
    Ok(t)
}

fn build_remote_branch(lua: &Lua, remote: &RepoRef) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("name", remote.name.as_str())?;
    t.set("remote_name", remote.remote_name.as_deref().unwrap_or(""))?;
    t.set("hash", remote.target_hash.as_str())?;
    Ok(t)
}

/// libgit2 surfaces the upstream as `"<remote>/<branch>"` (its short form,
/// e.g. `"origin/main"`). Pre-built remote tables are indexed by the same
/// `(remote_name, branch_name)` pair so the lookup hands back the canonical
/// table the top-level `remote_branches` list also points at.
fn resolve_upstream(
    short: &str,
    remote_lookup: &HashMap<(String, String), Table>,
) -> Option<Table> {
    let (remote_name, branch_name) = short.split_once('/')?;
    remote_lookup
        .get(&(remote_name.to_string(), branch_name.to_string()))
        .cloned()
}

/// Phase 11 typed read APIs.
///
/// `current` always works — it just hands back the cached snapshot
/// table that `sync_repository` keeps fresh on every host tick. The
/// rest (`refs`, `head`, `status`, `commits`, `diff`, `file_at`,
/// `blame`) read through the active gateway, which is the same one
/// the host's UI reads from. No new git2 calls; no new spawn paths.
///
/// All read functions follow the Lua-idiomatic `(value, nil)` /
/// `(nil, err)` shape. Capability gates per the Phase 10 / 11
/// descriptors:
/// - `status` -> `git:read:status`
/// - `commits` -> `git:read:log`
/// - `diff` -> `git:read:diff`
/// - `file_at` -> `git:read:show`
/// - `blame` -> `git:read:blame`
///
/// `current`, `refs`, and `head` are ungated (they expose data the
/// `leviathan.repository` snapshot already exposes).
pub fn install_read_functions(
    lua: &Lua,
    leviathan: &Table,
    git_ctx: GitOpsContext,
    guard: Rc<CapabilityGuard>,
    plugin_id: PluginId,
    generation_id: GenerationId,
) -> mlua::Result<()> {
    let repo_tbl: Table = leviathan.get("repository")?;

    // current() — return the cached snapshot.
    {
        let leviathan_clone = leviathan.clone();
        repo_tbl.set(
            "current",
            lua.create_function(move |_, ()| {
                let t: Table = leviathan_clone.get("repository")?;
                Ok(t)
            })?,
        )?;
    }

    // refs() — return locals/remotes/tags as a typed table.
    {
        let ctx = git_ctx.clone();
        let pid = plugin_id.clone();
        let gid = generation_id;
        repo_tbl.set(
            "refs",
            lua.create_function(move |lua_inner, ()| match ctx.gateway.get() {
                None => Ok((LuaValue::Nil, Some("no repository open".to_string()))),
                Some(gw) => match gw.load_refs_snapshot(COMMIT_LOAD_LIMIT) {
                    Ok(snap) => {
                        let t = build_refs_table(lua_inner, &snap.refs)?;
                        Ok((LuaValue::Table(t), None::<String>))
                    }
                    Err(e) => {
                        record_read_failed(&ctx.diagnostics, &pid, gid, "refs", &e.to_string());
                        Ok((LuaValue::Nil, Some(e.to_string())))
                    }
                },
            })?,
        )?;
    }

    // head() — { hash, ref, detached }.
    {
        let ctx = git_ctx.clone();
        let pid = plugin_id.clone();
        let gid = generation_id;
        repo_tbl.set(
            "head",
            lua.create_function(move |lua_inner, ()| match ctx.gateway.get() {
                None => Ok((LuaValue::Nil, Some("no repository open".to_string()))),
                Some(gw) => match gw.load_repo(COMMIT_LOAD_LIMIT) {
                    Ok(snap) => {
                        let t = lua_inner.create_table()?;
                        let hash = snap.head_hash.clone().unwrap_or_default();
                        let branch = snap.current_branch.clone().unwrap_or_default();
                        t.set("hash", hash.clone())?;
                        t.set("ref", branch.clone())?;
                        t.set("detached", !hash.is_empty() && branch.is_empty())?;
                        Ok((LuaValue::Table(t), None::<String>))
                    }
                    Err(e) => {
                        record_read_failed(&ctx.diagnostics, &pid, gid, "head", &e.to_string());
                        Ok((LuaValue::Nil, Some(e.to_string())))
                    }
                },
            })?,
        )?;
    }

    // status() — { staged[], unstaged[], conflicted[] }.
    {
        let ctx = git_ctx.clone();
        let g = Rc::clone(&guard);
        let pid = plugin_id.clone();
        let gid = generation_id;
        repo_tbl.set(
            "status",
            lua.create_function(move |lua_inner, ()| {
                if let Err(reason) = g.check_named("git:read:status") {
                    return Ok((LuaValue::Nil, Some(format!("capability denied: {reason}"))));
                }
                match ctx.gateway.get() {
                    None => Ok((LuaValue::Nil, Some("no repository open".to_string()))),
                    Some(gw) => match gw.load_repo(COMMIT_LOAD_LIMIT) {
                        Ok(snap) => {
                            let t = lua_inner.create_table()?;
                            let staged: &[_] = snap
                                .dirty
                                .as_ref()
                                .map(|d| d.staged_files.as_slice())
                                .unwrap_or(&[]);
                            let unstaged: &[_] = snap
                                .dirty
                                .as_ref()
                                .map(|d| d.unstaged_files.as_slice())
                                .unwrap_or(&[]);
                            let conflicted: &[_] = snap
                                .dirty
                                .as_ref()
                                .map(|d| d.conflicted_files.as_slice())
                                .unwrap_or(&[]);
                            t.set("staged", build_changed_files(lua_inner, staged)?)?;
                            t.set("unstaged", build_changed_files(lua_inner, unstaged)?)?;
                            t.set("conflicted", build_changed_files(lua_inner, conflicted)?)?;
                            Ok((LuaValue::Table(t), None::<String>))
                        }
                        Err(e) => {
                            record_read_failed(
                                &ctx.diagnostics,
                                &pid,
                                gid,
                                "status",
                                &e.to_string(),
                            );
                            Ok((LuaValue::Nil, Some(e.to_string())))
                        }
                    },
                }
            })?,
        )?;
    }

    // commits({limit, rev})
    {
        let ctx = git_ctx.clone();
        let g = Rc::clone(&guard);
        let pid = plugin_id.clone();
        let gid = generation_id;
        repo_tbl.set(
            "commits",
            lua.create_function(move |lua_inner, opts: Option<Table>| {
                if let Err(reason) = g.check_named("git:read:log") {
                    return Ok((LuaValue::Nil, Some(format!("capability denied: {reason}"))));
                }
                let limit = match opts
                    .as_ref()
                    .and_then(|t| t.get::<Option<i64>>("limit").ok().flatten())
                {
                    Some(n) if n > 0 => n as usize,
                    _ => 100,
                };
                match ctx.gateway.get() {
                    None => Ok((LuaValue::Nil, Some("no repository open".to_string()))),
                    Some(gw) => match gw.load_repo(limit.min(COMMIT_LOAD_LIMIT)) {
                        Ok(snap) => {
                            let arr = lua_inner.create_table()?;
                            for (i, c) in snap.commits.iter().take(limit).enumerate() {
                                let row = lua_inner.create_table()?;
                                row.set("hash", c.hash.as_str())?;
                                row.set(
                                    "summary",
                                    c.message.lines().next().unwrap_or("").to_string(),
                                )?;
                                row.set("author", c.author_name.as_str())?;
                                row.set("timestamp", c.authored_at)?;
                                let parents = lua_inner.create_table()?;
                                for (j, ph) in c.parent_hashes.iter().enumerate() {
                                    parents.set(j + 1, ph.as_str())?;
                                }
                                row.set("parents", parents)?;
                                arr.set(i + 1, row)?;
                            }
                            Ok((LuaValue::Table(arr), None::<String>))
                        }
                        Err(e) => {
                            record_read_failed(
                                &ctx.diagnostics,
                                &pid,
                                gid,
                                "commits",
                                &e.to_string(),
                            );
                            Ok((LuaValue::Nil, Some(e.to_string())))
                        }
                    },
                }
            })?,
        )?;
    }

    // diff({commit})
    {
        let ctx = git_ctx.clone();
        let g = Rc::clone(&guard);
        let pid = plugin_id.clone();
        let gid = generation_id;
        repo_tbl.set(
            "diff",
            lua.create_function(move |lua_inner, opts: Table| {
                if let Err(reason) = g.check_named("git:read:diff") {
                    return Ok((LuaValue::Nil, Some(format!("capability denied: {reason}"))));
                }
                let commit: String = match opts.get::<Option<String>>("commit") {
                    Ok(Some(s)) if !s.is_empty() => s,
                    _ => {
                        return Ok((
                            LuaValue::Nil,
                            Some("missing required arg `commit`".to_string()),
                        ))
                    }
                };
                match ctx.gateway.get() {
                    None => Ok((LuaValue::Nil, Some("no repository open".to_string()))),
                    Some(gw) => match gw.load_commit_diff(0, &commit) {
                        Ok(diff) => {
                            let arr = lua_inner.create_table()?;
                            for (i, file) in diff.files.iter().enumerate() {
                                let row = lua_inner.create_table()?;
                                row.set("path", file.path.as_str())?;
                                row.set("status", change_kind_str(&file.kind))?;
                                arr.set(i + 1, row)?;
                            }
                            let wrap = lua_inner.create_table()?;
                            wrap.set("commit", diff.hash.as_str())?;
                            wrap.set("modified_count", diff.modified_count)?;
                            wrap.set("added_count", diff.added_count)?;
                            wrap.set("deleted_count", diff.deleted_count)?;
                            wrap.set("files", arr)?;
                            Ok((LuaValue::Table(wrap), None::<String>))
                        }
                        Err(e) => {
                            record_read_failed(&ctx.diagnostics, &pid, gid, "diff", &e.to_string());
                            Ok((LuaValue::Nil, Some(e.to_string())))
                        }
                    },
                }
            })?,
        )?;
    }

    // file_at({commit, path})
    {
        let ctx = git_ctx.clone();
        let g = Rc::clone(&guard);
        let pid = plugin_id.clone();
        let gid = generation_id;
        repo_tbl.set(
            "file_at",
            lua.create_function(move |lua_inner, opts: Table| {
                if let Err(reason) = g.check_named("git:read:show") {
                    return Ok((LuaValue::Nil, Some(format!("capability denied: {reason}"))));
                }
                let commit: String = match opts.get::<Option<String>>("commit") {
                    Ok(Some(s)) if !s.is_empty() => s,
                    _ => return Ok((LuaValue::Nil, Some("missing arg `commit`".to_string()))),
                };
                let path: String = match opts.get::<Option<String>>("path") {
                    Ok(Some(s)) if !s.is_empty() => s,
                    _ => return Ok((LuaValue::Nil, Some("missing arg `path`".to_string()))),
                };
                match ctx.gateway.get() {
                    None => Ok((LuaValue::Nil, Some("no repository open".to_string()))),
                    Some(gw) => match gw.load_commit_file_diff(&commit, &path) {
                        Ok(d) => {
                            let t = lua_inner.create_table()?;
                            t.set("commit", commit.as_str())?;
                            t.set("path", path.as_str())?;
                            let lines_tbl = lua_inner.create_table()?;
                            for (i, line) in d.lines.iter().enumerate() {
                                lines_tbl.set(i + 1, line.content.as_str())?;
                            }
                            t.set("lines", lines_tbl)?;
                            t.set(
                                "old_content",
                                d.old_file_content.clone().unwrap_or_default(),
                            )?;
                            t.set(
                                "new_content",
                                d.new_file_content.clone().unwrap_or_default(),
                            )?;
                            Ok((LuaValue::Table(t), None::<String>))
                        }
                        Err(e) => {
                            record_read_failed(
                                &ctx.diagnostics,
                                &pid,
                                gid,
                                "file_at",
                                &e.to_string(),
                            );
                            Ok((LuaValue::Nil, Some(e.to_string())))
                        }
                    },
                }
            })?,
        )?;
    }

    // blame: not yet wired. The capability check still gates it so a
    // plugin without `git:read:blame` sees the same failure shape.
    {
        let g = Rc::clone(&guard);
        repo_tbl.set(
            "blame",
            lua.create_function(move |_, _opts: Table| {
                if let Err(reason) = g.check_named("git:read:blame") {
                    return Ok((LuaValue::Nil, Some(format!("capability denied: {reason}"))));
                }
                Ok((
                    LuaValue::Nil,
                    Some("blame is not yet implemented".to_string()),
                ))
            })?,
        )?;
    }

    leviathan.set("repository", repo_tbl)?;
    Ok(())
}

fn record_read_failed(
    diagnostics: &DiagnosticStore,
    plugin_id: &PluginId,
    generation_id: GenerationId,
    op: &str,
    cause: &str,
) {
    diagnostics.record(
        PluginDiagnostic::new(
            plugin_id.clone(),
            DiagnosticSeverity::Error,
            "repository.read_failed",
            format!("repository.{op} failed: {cause}"),
        )
        .with_generation(generation_id)
        .with_source(PluginSourceSpan::ApiFunction {
            name: format!("repository.{op}"),
        })
        .with_context(serde_json::json!({
            "op": op,
            "cause": cause,
        })),
    );
}

fn build_refs_table(lua: &Lua, refs: &[RepoRef]) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    let locals = lua.create_table()?;
    let remotes = lua.create_table()?;
    let tags = lua.create_table()?;
    let mut local_i = 1;
    let mut remote_i = 1;
    let mut tag_i = 1;
    for r in refs {
        let row = lua.create_table()?;
        row.set("name", r.name.as_str())?;
        row.set("hash", r.target_hash.as_str())?;
        match r.kind {
            RepoRefKind::LocalBranch => {
                row.set("is_current", r.is_current)?;
                locals.set(local_i, row)?;
                local_i += 1;
            }
            RepoRefKind::RemoteBranch => {
                row.set("remote_name", r.remote_name.as_deref().unwrap_or(""))?;
                remotes.set(remote_i, row)?;
                remote_i += 1;
            }
            RepoRefKind::Tag => {
                tags.set(tag_i, row)?;
                tag_i += 1;
            }
        }
    }
    t.set("locals", locals)?;
    t.set("remotes", remotes)?;
    t.set("tags", tags)?;
    Ok(t)
}

fn build_changed_files(lua: &Lua, files: &[crate::core::ChangedFile]) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    for (i, f) in files.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("path", f.path.as_str())?;
        row.set("status", change_kind_str(&f.kind))?;
        t.set(i + 1, row)?;
    }
    Ok(t)
}

fn change_kind_str(kind: &crate::core::ChangeKind) -> &'static str {
    match kind {
        crate::core::ChangeKind::Added => "added",
        crate::core::ChangeKind::Modified => "modified",
        crate::core::ChangeKind::Deleted => "deleted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Lua, Value};

    fn local(name: &str, hash: &str, is_current: bool, upstream: Option<&str>) -> RepoRef {
        RepoRef {
            name: name.to_string(),
            kind: RepoRefKind::LocalBranch,
            target_hash: hash.to_string(),
            remote_name: None,
            is_current,
            upstream_ref: upstream.map(str::to_string),
        }
    }

    fn remote(name: &str, remote_name: &str, hash: &str) -> RepoRef {
        RepoRef {
            name: name.to_string(),
            kind: RepoRefKind::RemoteBranch,
            target_hash: hash.to_string(),
            remote_name: Some(remote_name.to_string()),
            is_current: false,
            upstream_ref: None,
        }
    }

    fn tag(name: &str, hash: &str) -> RepoRef {
        RepoRef {
            name: name.to_string(),
            kind: RepoRefKind::Tag,
            target_hash: hash.to_string(),
            remote_name: None,
            is_current: false,
            upstream_ref: None,
        }
    }

    #[test]
    fn workdir_path_is_exposed_on_table() {
        let lua = Lua::new();
        let t = build_table(&lua, "repo", "/home/u/proj", "HEAD", "", "", &[]).unwrap();
        assert_eq!(t.get::<String>("workdir_path").unwrap(), "/home/u/proj");
    }

    #[test]
    fn head_hash_is_exposed_on_table() {
        let lua = Lua::new();
        let t = build_table(&lua, "repo", "/tmp/repo", "HEAD", "deadbeef", "", &[]).unwrap();
        assert_eq!(t.get::<String>("head_hash").unwrap(), "deadbeef");
    }

    #[test]
    fn default_remote_name_is_exposed_on_table() {
        let lua = Lua::new();
        let t = build_table(&lua, "repo", "/tmp/repo", "main", "", "origin", &[]).unwrap();
        assert_eq!(t.get::<String>("default_remote_name").unwrap(), "origin");
    }

    #[test]
    fn empty_default_remote_name_round_trips_as_empty_string() {
        let lua = Lua::new();
        let t = build_table(&lua, "", "", "", "", "", &[]).unwrap();
        assert_eq!(t.get::<String>("default_remote_name").unwrap(), "");
    }

    #[test]
    fn empty_head_hash_round_trips_as_empty_string() {
        let lua = Lua::new();
        let t = build_table(&lua, "", "", "", "", "", &[]).unwrap();
        assert_eq!(t.get::<String>("head_hash").unwrap(), "");
    }

    #[test]
    fn empty_workdir_path_round_trips_as_empty_string() {
        let lua = Lua::new();
        let t = build_table(&lua, "", "", "", "", "", &[]).unwrap();
        assert_eq!(t.get::<String>("workdir_path").unwrap(), "");
    }

    #[test]
    fn empty_refs_produce_empty_table() {
        let lua = Lua::new();
        let t = build_table(&lua, "repo", "/tmp/repo", "HEAD", "", "", &[]).unwrap();
        assert_eq!(t.get::<String>("name").unwrap(), "repo");
        assert_eq!(t.get::<String>("current_branch_name").unwrap(), "HEAD");
        assert!(matches!(
            t.get::<Value>("current_branch").unwrap(),
            Value::Nil
        ));
        let locals: mlua::Table = t.get("local_branches").unwrap();
        assert_eq!(locals.len().unwrap(), 0);
    }

    #[test]
    fn local_without_upstream_has_nil_upstream_branch() {
        let lua = Lua::new();
        let refs = vec![local("main", "aaaa", false, None)];
        let t = build_table(&lua, "repo", "/tmp/repo", "main", "", "", &refs).unwrap();
        let locals: mlua::Table = t.get("local_branches").unwrap();
        assert_eq!(locals.len().unwrap(), 1);
        let b: mlua::Table = locals.get(1).unwrap();
        assert_eq!(b.get::<String>("name").unwrap(), "main");
        assert_eq!(b.get::<String>("hash").unwrap(), "aaaa");
        assert!(matches!(
            b.get::<Value>("upstream_branch").unwrap(),
            Value::Nil
        ));
    }

    #[test]
    fn upstream_resolves_from_short_ref() {
        let lua = Lua::new();
        let refs = vec![
            local("main", "aaaa", true, Some("origin/main")),
            remote("main", "origin", "bbbb"),
        ];
        let t = build_table(&lua, "repo", "/tmp/repo", "main", "", "", &refs).unwrap();
        let locals: mlua::Table = t.get("local_branches").unwrap();
        let b: mlua::Table = locals.get(1).unwrap();
        let up: mlua::Table = b.get("upstream_branch").unwrap();
        assert_eq!(up.get::<String>("name").unwrap(), "main");
        assert_eq!(up.get::<String>("remote_name").unwrap(), "origin");
        assert_eq!(up.get::<String>("hash").unwrap(), "bbbb");
    }

    #[test]
    fn current_branch_is_the_is_current_local() {
        let lua = Lua::new();
        let refs = vec![
            local("main", "aaaa", false, None),
            local("feature", "bbbb", true, None),
        ];
        let t = build_table(&lua, "repo", "/tmp/repo", "feature", "", "", &refs).unwrap();
        let current: mlua::Table = t.get("current_branch").unwrap();
        assert_eq!(current.get::<String>("name").unwrap(), "feature");
    }

    #[test]
    fn is_detached_true_when_head_at_commit_with_no_current_branch() {
        let lua = Lua::new();
        let refs = vec![local("main", "aaaa", false, None)];
        let t = build_table(&lua, "repo", "/tmp/repo", "HEAD", "deadbeef", "", &refs).unwrap();
        assert!(t.get::<bool>("is_detached").unwrap());
    }

    #[test]
    fn is_detached_false_when_on_a_local_branch() {
        let lua = Lua::new();
        let refs = vec![local("main", "aaaa", true, None)];
        let t = build_table(&lua, "repo", "/tmp/repo", "main", "aaaa", "", &refs).unwrap();
        assert!(!t.get::<bool>("is_detached").unwrap());
    }

    #[test]
    fn is_detached_false_when_no_repo_open() {
        let lua = Lua::new();
        let t = build_table(&lua, "", "", "", "", "", &[]).unwrap();
        assert!(!t.get::<bool>("is_detached").unwrap());
    }

    #[test]
    fn is_unborn_true_when_repo_open_but_no_head_hash() {
        let lua = Lua::new();
        let t = build_table(&lua, "repo", "/tmp/repo", "main", "", "", &[]).unwrap();
        assert!(t.get::<bool>("is_unborn").unwrap());
    }

    #[test]
    fn is_unborn_false_when_no_repo_open() {
        let lua = Lua::new();
        let t = build_table(&lua, "", "", "", "", "", &[]).unwrap();
        assert!(!t.get::<bool>("is_unborn").unwrap());
    }

    #[test]
    fn is_unborn_false_when_head_has_commit() {
        let lua = Lua::new();
        let refs = vec![local("main", "aaaa", true, None)];
        let t = build_table(&lua, "repo", "/tmp/repo", "main", "aaaa", "", &refs).unwrap();
        assert!(!t.get::<bool>("is_unborn").unwrap());
    }

    #[test]
    fn is_unborn_false_when_detached() {
        let lua = Lua::new();
        let t = build_table(&lua, "repo", "/tmp/repo", "HEAD", "deadbeef", "", &[]).unwrap();
        assert!(!t.get::<bool>("is_unborn").unwrap());
    }

    #[test]
    fn is_open_true_when_repo_has_a_name() {
        let lua = Lua::new();
        let refs = vec![local("main", "aaaa", true, None)];
        let t = build_table(&lua, "repo", "/tmp/repo", "main", "aaaa", "", &refs).unwrap();
        assert!(t.get::<bool>("is_open").unwrap());
    }

    #[test]
    fn is_open_true_for_bare_repo() {
        let lua = Lua::new();
        let t = build_table(&lua, "repo", "", "main", "aaaa", "", &[]).unwrap();
        assert!(t.get::<bool>("is_open").unwrap());
    }

    #[test]
    fn is_open_false_when_no_repo_loaded() {
        let lua = Lua::new();
        let t = build_table(&lua, "", "", "", "", "", &[]).unwrap();
        assert!(!t.get::<bool>("is_open").unwrap());
    }

    #[test]
    fn is_bare_true_when_repo_open_with_empty_workdir() {
        let lua = Lua::new();
        let t = build_table(&lua, "repo", "", "main", "", "", &[]).unwrap();
        assert!(t.get::<bool>("is_bare").unwrap());
    }

    #[test]
    fn is_bare_false_when_no_repo_open() {
        let lua = Lua::new();
        let t = build_table(&lua, "", "", "", "", "", &[]).unwrap();
        assert!(!t.get::<bool>("is_bare").unwrap());
    }

    #[test]
    fn is_bare_false_when_repo_has_workdir() {
        let lua = Lua::new();
        let refs = vec![local("main", "aaaa", true, None)];
        let t = build_table(&lua, "repo", "/tmp/repo", "main", "aaaa", "", &refs).unwrap();
        assert!(!t.get::<bool>("is_bare").unwrap());
    }

    #[test]
    fn detached_head_gives_nil_current_branch() {
        let lua = Lua::new();
        let refs = vec![local("main", "aaaa", false, None)];
        let t = build_table(&lua, "repo", "/tmp/repo", "HEAD", "", "", &refs).unwrap();
        assert!(matches!(
            t.get::<Value>("current_branch").unwrap(),
            Value::Nil
        ));
    }

    #[test]
    fn empty_refs_produce_empty_remote_branches() {
        let lua = Lua::new();
        let t = build_table(&lua, "repo", "/tmp/repo", "HEAD", "", "", &[]).unwrap();
        let remotes: mlua::Table = t.get("remote_branches").unwrap();
        assert_eq!(remotes.len().unwrap(), 0);
    }

    #[test]
    fn remote_branches_lists_every_remote_ref() {
        let lua = Lua::new();
        let refs = vec![
            remote("main", "origin", "bbbb"),
            remote("feature", "origin", "cccc"),
            remote("main", "fork", "dddd"),
        ];
        let t = build_table(&lua, "repo", "/tmp/repo", "HEAD", "", "", &refs).unwrap();
        let remotes: mlua::Table = t.get("remote_branches").unwrap();
        assert_eq!(remotes.len().unwrap(), 3);
        let r1: mlua::Table = remotes.get(1).unwrap();
        assert_eq!(r1.get::<String>("name").unwrap(), "main");
        assert_eq!(r1.get::<String>("remote_name").unwrap(), "origin");
        assert_eq!(r1.get::<String>("hash").unwrap(), "bbbb");
        let r3: mlua::Table = remotes.get(3).unwrap();
        assert_eq!(r3.get::<String>("remote_name").unwrap(), "fork");
    }

    #[test]
    fn empty_refs_produce_empty_tags() {
        let lua = Lua::new();
        let t = build_table(&lua, "repo", "/tmp/repo", "HEAD", "", "", &[]).unwrap();
        let tags: mlua::Table = t.get("tags").unwrap();
        assert_eq!(tags.len().unwrap(), 0);
    }

    #[test]
    fn tags_lists_every_tag_ref() {
        let lua = Lua::new();
        let refs = vec![
            local("main", "aaaa", true, None),
            tag("v1.0.0", "bbbb"),
            tag("v1.1.0", "cccc"),
        ];
        let t = build_table(&lua, "repo", "/tmp/repo", "main", "", "", &refs).unwrap();
        let tags: mlua::Table = t.get("tags").unwrap();
        assert_eq!(tags.len().unwrap(), 2);
        let t1: mlua::Table = tags.get(1).unwrap();
        assert_eq!(t1.get::<String>("name").unwrap(), "v1.0.0");
        assert_eq!(t1.get::<String>("hash").unwrap(), "bbbb");
        let t2: mlua::Table = tags.get(2).unwrap();
        assert_eq!(t2.get::<String>("name").unwrap(), "v1.1.0");
        assert_eq!(t2.get::<String>("hash").unwrap(), "cccc");
    }

    #[test]
    fn local_branch_exposes_is_current_flag() {
        let lua = Lua::new();
        let refs = vec![
            local("main", "aaaa", false, None),
            local("feature", "bbbb", true, None),
        ];
        let t = build_table(&lua, "repo", "/tmp/repo", "feature", "", "", &refs).unwrap();
        let locals: mlua::Table = t.get("local_branches").unwrap();
        let main: mlua::Table = locals.get(1).unwrap();
        let feature: mlua::Table = locals.get(2).unwrap();
        assert!(!main.get::<bool>("is_current").unwrap());
        assert!(feature.get::<bool>("is_current").unwrap());
    }

    #[test]
    fn upstream_branch_shares_identity_with_remote_branches_entry() {
        let lua = Lua::new();
        let refs = vec![
            local("main", "aaaa", true, Some("origin/main")),
            remote("main", "origin", "bbbb"),
        ];
        let t = build_table(&lua, "repo", "/tmp/repo", "main", "", "", &refs).unwrap();
        let locals: mlua::Table = t.get("local_branches").unwrap();
        let local_main: mlua::Table = locals.get(1).unwrap();
        let upstream: mlua::Table = local_main.get("upstream_branch").unwrap();
        let remotes: mlua::Table = t.get("remote_branches").unwrap();
        let remote_main: mlua::Table = remotes.get(1).unwrap();
        assert!(
            upstream.equals(&remote_main).unwrap(),
            "upstream_branch must be the same Lua table as the remote_branches entry"
        );
    }
}

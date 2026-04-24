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
//!   current_branch_name = "main",           -- display label, "HEAD" when detached
//!   current_branch = <LocalBranch | nil>,   -- nil when HEAD detached
//!   local_branches = { <LocalBranch>, ... },
//! }
//! LocalBranch  = { name, hash, upstream_branch = <RemoteBranch | nil> }
//! RemoteBranch = { name, remote_name, hash }
//! ```
//!
//! `current_branch` is the same `LocalBranch` table (same identity) as
//! whichever entry in `local_branches` is marked `is_current` by libgit2.
//! Mutations made by a plugin to the table are not propagated back — the
//! table is rebuilt on every sync.

use mlua::{Lua, Table};

use crate::services::{RepoRef, RepoRefKind};

/// Build the full `leviathan.repository` table from the latest refs.
///
/// Returns the fresh table to the caller; installing it onto the
/// `leviathan` global is the host's job.
pub fn build_table(
    lua: &Lua,
    repo_name: &str,
    current_branch_name: &str,
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

    // Build each local-branch table once; index by `&name` so we can
    // reuse the same table for `current_branch` (Lua `==` identity, plus
    // readers that cache on reference equality just work).
    let local_branches_table = lua.create_table()?;
    let mut current_branch: Option<Table> = None;
    for (i, local) in locals.iter().enumerate() {
        let t = build_local_branch(lua, local, &remotes)?;
        if local.is_current {
            current_branch = Some(t.clone());
        }
        local_branches_table.raw_set(i + 1, t)?;
    }

    let repo = lua.create_table()?;
    repo.set("name", repo_name)?;
    repo.set("current_branch_name", current_branch_name)?;
    match current_branch {
        Some(t) => repo.set("current_branch", t)?,
        None => repo.set("current_branch", mlua::Value::Nil)?,
    }
    repo.set("local_branches", local_branches_table)?;
    Ok(repo)
}

fn build_local_branch(lua: &Lua, local: &RepoRef, remotes: &[&RepoRef]) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("name", local.name.as_str())?;
    t.set("hash", local.target_hash.as_str())?;
    let upstream = local
        .upstream_ref
        .as_deref()
        .and_then(|short| resolve_upstream(short, remotes));
    match upstream {
        Some(remote) => {
            t.set("upstream_branch", build_remote_branch(lua, remote)?)?;
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
/// e.g. `"origin/main"`). The remote-kind refs in `refs` have been pre-split
/// into `remote_name = "origin"` + `name = "main"` so we match on the pair.
fn resolve_upstream<'a>(short: &str, remotes: &'a [&'a RepoRef]) -> Option<&'a RepoRef> {
    let (remote_name, branch_name) = short.split_once('/')?;
    remotes
        .iter()
        .copied()
        .find(|r| r.remote_name.as_deref() == Some(remote_name) && r.name == branch_name)
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

    #[test]
    fn empty_refs_produce_empty_table() {
        let lua = Lua::new();
        let t = build_table(&lua, "repo", "HEAD", &[]).unwrap();
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
        let t = build_table(&lua, "repo", "main", &refs).unwrap();
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
        let t = build_table(&lua, "repo", "main", &refs).unwrap();
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
        let t = build_table(&lua, "repo", "feature", &refs).unwrap();
        let current: mlua::Table = t.get("current_branch").unwrap();
        assert_eq!(current.get::<String>("name").unwrap(), "feature");
    }

    #[test]
    fn detached_head_gives_nil_current_branch() {
        let lua = Lua::new();
        let refs = vec![local("main", "aaaa", false, None)];
        let t = build_table(&lua, "repo", "HEAD", &refs).unwrap();
        assert!(matches!(
            t.get::<Value>("current_branch").unwrap(),
            Value::Nil
        ));
    }
}

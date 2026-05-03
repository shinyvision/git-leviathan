//! `leviathan.git.*` — Phase 11 typed Git write namespace.
//!
//! Every entry point captures the supplied options into a typed
//! [`GitOpRequest`] and forwards to the host-owned
//! [`GitOpsContext::execute`]. The host is the only thing that touches
//! a real `git2::Repository`.
//!
//! Returns are uniform: `(true, nil)` on success, `(false, err)` on
//! failure (including `capability denied`, `destructive blocked`, or
//! `no repository open`). Result events fire on the host's typed event
//! bus before this function returns, so a Lua autocmd registered for
//! `HeadChanged` (etc.) sees the post-op state immediately.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Lua, Table};

use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::events::EventPayload;
use crate::plugin::git_ops::{GitOpRequest, GitOpsContext, ResetMode};
use crate::plugin::resources::{GenerationId, PluginId};

/// Outbound queue of Phase 7 events to fire post-execute. The host
/// drains this on `tick` (or after each call when a synchronous tick
/// is forced). Cheap-clone (Rc-backed); shared between the Lua shims
/// and the host.
#[derive(Clone, Default)]
pub struct PendingGitEvents {
    inner: Rc<RefCell<Vec<(&'static str, EventPayload)>>>,
}

impl PendingGitEvents {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_many(&self, events: Vec<(&'static str, EventPayload)>) {
        if events.is_empty() {
            return;
        }
        self.inner.borrow_mut().extend(events);
    }

    pub fn drain(&self) -> Vec<(&'static str, EventPayload)> {
        std::mem::take(&mut *self.inner.borrow_mut())
    }
}

pub fn install(
    lua: &Lua,
    leviathan: &Table,
    git_ctx: GitOpsContext,
    pending_events: PendingGitEvents,
    guard: Rc<CapabilityGuard>,
    plugin_id: PluginId,
    generation_id: GenerationId,
) -> mlua::Result<()> {
    let git_tbl = lua.create_table()?;

    install_op(
        lua,
        &git_tbl,
        "checkout",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let ref_name = opts
                .get::<Option<String>>("ref")?
                .ok_or_else(|| mlua::Error::external("missing arg `ref`"))?;
            Ok(GitOpRequest::Checkout { ref_name })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "create_branch",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let name = opts
                .get::<Option<String>>("name")?
                .ok_or_else(|| mlua::Error::external("missing arg `name`"))?;
            let start_point = opts.get::<Option<String>>("start_point")?;
            Ok(GitOpRequest::CreateBranch { name, start_point })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "delete_branch",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let name = opts
                .get::<Option<String>>("name")?
                .ok_or_else(|| mlua::Error::external("missing arg `name`"))?;
            let force = opts.get::<Option<bool>>("force")?.unwrap_or(false);
            Ok(GitOpRequest::DeleteBranch { name, force })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "create_tag",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let name = opts
                .get::<Option<String>>("name")?
                .ok_or_else(|| mlua::Error::external("missing arg `name`"))?;
            let target = opts.get::<Option<String>>("target")?;
            Ok(GitOpRequest::CreateTag { name, target })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "delete_tag",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let name = opts
                .get::<Option<String>>("name")?
                .ok_or_else(|| mlua::Error::external("missing arg `name`"))?;
            Ok(GitOpRequest::DeleteTag { name })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "commit",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let message = opts
                .get::<Option<String>>("message")?
                .ok_or_else(|| mlua::Error::external("missing arg `message`"))?;
            Ok(GitOpRequest::Commit { message })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "stash_push",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let message = opts.get::<Option<String>>("message")?;
            Ok(GitOpRequest::StashPush { message })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "stash_pop",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let index = opts.get::<Option<i64>>("index")?.unwrap_or(0).max(0) as usize;
            Ok(GitOpRequest::StashPop { index })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "reset",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let ref_name = opts
                .get::<Option<String>>("ref")?
                .ok_or_else(|| mlua::Error::external("missing arg `ref`"))?;
            let mode_str = opts
                .get::<Option<String>>("mode")?
                .unwrap_or_else(|| "mixed".to_string());
            let mode = ResetMode::parse(&mode_str)
                .ok_or_else(|| mlua::Error::external(format!("unknown reset mode `{mode_str}`")))?;
            Ok(GitOpRequest::Reset { ref_name, mode })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "fetch",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let remote = opts.get::<Option<String>>("remote")?;
            Ok(GitOpRequest::Fetch { remote })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "push",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let remote = opts.get::<Option<String>>("remote")?;
            let ref_name = opts.get::<Option<String>>("ref")?;
            Ok(GitOpRequest::Push { remote, ref_name })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "merge",
        Rc::clone(&guard),
        git_ctx.clone(),
        pending_events.clone(),
        plugin_id.clone(),
        generation_id,
        |opts| {
            let ref_name = opts
                .get::<Option<String>>("ref")?
                .ok_or_else(|| mlua::Error::external("missing arg `ref`"))?;
            Ok(GitOpRequest::Merge { ref_name })
        },
    )?;

    install_op(
        lua,
        &git_tbl,
        "rebase",
        Rc::clone(&guard),
        git_ctx,
        pending_events,
        plugin_id,
        generation_id,
        |opts| {
            let ref_name = opts
                .get::<Option<String>>("ref")?
                .ok_or_else(|| mlua::Error::external("missing arg `ref`"))?;
            Ok(GitOpRequest::Rebase { ref_name })
        },
    )?;

    leviathan.set("git", git_tbl)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn install_op(
    lua: &Lua,
    git_tbl: &Table,
    name: &'static str,
    guard: Rc<CapabilityGuard>,
    ctx: GitOpsContext,
    pending: PendingGitEvents,
    plugin_id: PluginId,
    generation_id: GenerationId,
    parse: impl Fn(&Table) -> mlua::Result<GitOpRequest> + 'static,
) -> mlua::Result<()> {
    git_tbl.set(
        name,
        lua.create_function(move |_lua_inner, opts: Option<Table>| {
            let opts = opts.unwrap_or_else(|| {
                _lua_inner
                    .create_table()
                    .expect("create empty options table")
            });
            let request = match parse(&opts) {
                Ok(r) => r,
                Err(e) => {
                    let msg = e.to_string();
                    ctx.diagnostics.record(
                        crate::plugin::diagnostic::PluginDiagnostic::new(
                            plugin_id.clone(),
                            crate::plugin::diagnostic::DiagnosticSeverity::Warning,
                            "git.invalid_args",
                            format!("git.{name} rejected: {msg}"),
                        )
                        .with_generation(generation_id)
                        .with_source(
                            crate::plugin::diagnostic::PluginSourceSpan::ApiFunction {
                                name: format!("git.{name}"),
                            },
                        ),
                    );
                    return Ok((false, Some(msg)));
                }
            };
            let g = Rc::clone(&guard);
            let exec = ctx.execute(plugin_id.as_str(), generation_id, request, |cap| {
                g.check_named(cap)
            });
            pending.push_many(exec.events.fires);
            match exec.error {
                None => Ok((true, None::<String>)),
                Some(err) => Ok((false, Some(err))),
            }
        })?,
    )?;
    Ok(())
}

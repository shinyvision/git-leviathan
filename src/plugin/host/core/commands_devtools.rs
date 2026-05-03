use super::*;

struct BuiltinGitCommandSpec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    args: Vec<commands::CommandArg>,
    destructive: bool,
    background: bool,
}

fn command_arg(
    name: &'static str,
    ty: commands::CommandArgType,
    required: bool,
    default: Option<serde_json::Value>,
    doc: &'static str,
) -> commands::CommandArg {
    commands::CommandArg {
        name: name.into(),
        ty,
        required,
        default,
        doc: doc.into(),
    }
}

fn builtin_git_command_specs() -> Vec<BuiltinGitCommandSpec> {
    use commands::CommandArgType::{Boolean, Integer, String as StringArg};
    vec![
        BuiltinGitCommandSpec {
            name: "git.checkout",
            title: "Git: Checkout",
            description: "Check out a ref.",
            args: vec![command_arg(
                "ref",
                StringArg,
                true,
                None,
                "Ref to check out.",
            )],
            destructive: false,
            background: false,
        },
        BuiltinGitCommandSpec {
            name: "git.create_branch",
            title: "Git: Create Branch",
            description: "Create a branch at start_point.",
            args: vec![
                command_arg("name", StringArg, true, None, "Branch name."),
                command_arg(
                    "start_point",
                    StringArg,
                    false,
                    None,
                    "Commit/ref to create the branch at.",
                ),
            ],
            destructive: false,
            background: false,
        },
        BuiltinGitCommandSpec {
            name: "git.delete_branch",
            title: "Git: Delete Branch",
            description: "Delete a branch.",
            args: vec![
                command_arg("name", StringArg, true, None, "Branch name."),
                command_arg(
                    "force",
                    Boolean,
                    false,
                    Some(serde_json::json!(false)),
                    "Force deletion.",
                ),
            ],
            destructive: true,
            background: false,
        },
        BuiltinGitCommandSpec {
            name: "git.create_tag",
            title: "Git: Create Tag",
            description: "Create a tag at target.",
            args: vec![
                command_arg("name", StringArg, true, None, "Tag name."),
                command_arg("target", StringArg, false, None, "Commit/ref to tag."),
            ],
            destructive: false,
            background: false,
        },
        BuiltinGitCommandSpec {
            name: "git.delete_tag",
            title: "Git: Delete Tag",
            description: "Delete a tag locally.",
            args: vec![command_arg("name", StringArg, true, None, "Tag name.")],
            destructive: true,
            background: false,
        },
        BuiltinGitCommandSpec {
            name: "git.commit",
            title: "Git: Commit",
            description: "Commit staged changes.",
            args: vec![command_arg(
                "message",
                StringArg,
                true,
                None,
                "Commit message.",
            )],
            destructive: false,
            background: false,
        },
        BuiltinGitCommandSpec {
            name: "git.stash_push",
            title: "Git: Stash Push",
            description: "Stash dirty changes.",
            args: vec![command_arg(
                "message",
                StringArg,
                false,
                None,
                "Stash message.",
            )],
            destructive: false,
            background: false,
        },
        BuiltinGitCommandSpec {
            name: "git.stash_pop",
            title: "Git: Stash Pop",
            description: "Pop a stash.",
            args: vec![command_arg(
                "index",
                Integer,
                false,
                Some(serde_json::json!(0)),
                "Stash index.",
            )],
            destructive: false,
            background: false,
        },
        BuiltinGitCommandSpec {
            name: "git.reset",
            title: "Git: Reset",
            description: "Reset current branch.",
            args: vec![
                command_arg("ref", StringArg, true, None, "Target ref."),
                command_arg(
                    "mode",
                    commands::CommandArgType::Enum(vec![
                        "soft".into(),
                        "mixed".into(),
                        "hard".into(),
                    ]),
                    false,
                    Some(serde_json::json!("mixed")),
                    "Reset mode.",
                ),
            ],
            destructive: true,
            background: false,
        },
        BuiltinGitCommandSpec {
            name: "git.fetch",
            title: "Git: Fetch",
            description: "Fetch from remote.",
            args: vec![command_arg(
                "remote",
                StringArg,
                false,
                None,
                "Remote name.",
            )],
            destructive: false,
            background: true,
        },
        BuiltinGitCommandSpec {
            name: "git.push",
            title: "Git: Push",
            description: "Push current branch.",
            args: vec![
                command_arg("remote", StringArg, false, None, "Remote name."),
                command_arg("ref", StringArg, false, None, "Ref to push."),
            ],
            destructive: false,
            background: true,
        },
        BuiltinGitCommandSpec {
            name: "git.merge",
            title: "Git: Merge",
            description: "Merge a ref into current.",
            args: vec![command_arg("ref", StringArg, true, None, "Ref to merge.")],
            destructive: true,
            background: false,
        },
        BuiltinGitCommandSpec {
            name: "git.rebase",
            title: "Git: Rebase",
            description: "Rebase onto a ref.",
            args: vec![command_arg(
                "ref",
                StringArg,
                true,
                None,
                "Ref to rebase onto.",
            )],
            destructive: true,
            background: false,
        },
    ]
}

fn git_request_from_command(
    name: &str,
    args: &serde_json::Value,
) -> Result<crate::plugin::git_ops::GitOpRequest, String> {
    use crate::plugin::git_ops::{GitOpRequest, ResetMode};

    match name {
        "git.checkout" => Ok(GitOpRequest::Checkout {
            ref_name: required_string_arg(args, "ref")?,
        }),
        "git.create_branch" => Ok(GitOpRequest::CreateBranch {
            name: required_string_arg(args, "name")?,
            start_point: string_arg(args, "start_point"),
        }),
        "git.delete_branch" => Ok(GitOpRequest::DeleteBranch {
            name: required_string_arg(args, "name")?,
            force: bool_arg(args, "force", false),
        }),
        "git.create_tag" => Ok(GitOpRequest::CreateTag {
            name: required_string_arg(args, "name")?,
            target: string_arg(args, "target"),
        }),
        "git.delete_tag" => Ok(GitOpRequest::DeleteTag {
            name: required_string_arg(args, "name")?,
        }),
        "git.commit" => Ok(GitOpRequest::Commit {
            message: required_string_arg(args, "message")?,
        }),
        "git.stash_push" => Ok(GitOpRequest::StashPush {
            message: string_arg(args, "message"),
        }),
        "git.stash_pop" => Ok(GitOpRequest::StashPop {
            index: integer_arg(args, "index", 0).max(0) as usize,
        }),
        "git.reset" => {
            let mode_raw = string_arg(args, "mode").unwrap_or_else(|| "mixed".to_string());
            let mode = ResetMode::parse(&mode_raw)
                .ok_or_else(|| format!("unknown reset mode `{mode_raw}`"))?;
            Ok(GitOpRequest::Reset {
                ref_name: required_string_arg(args, "ref")?,
                mode,
            })
        }
        "git.fetch" => Ok(GitOpRequest::Fetch {
            remote: string_arg(args, "remote"),
        }),
        "git.push" => Ok(GitOpRequest::Push {
            remote: string_arg(args, "remote"),
            ref_name: string_arg(args, "ref"),
        }),
        "git.merge" => Ok(GitOpRequest::Merge {
            ref_name: required_string_arg(args, "ref")?,
        }),
        "git.rebase" => Ok(GitOpRequest::Rebase {
            ref_name: required_string_arg(args, "ref")?,
        }),
        other => Err(format!("unknown git command `{other}`")),
    }
}

fn string_arg(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn required_string_arg(args: &serde_json::Value, key: &str) -> Result<String, String> {
    string_arg(args, key).ok_or_else(|| format!("missing required arg `{key}`"))
}

fn bool_arg(args: &serde_json::Value, key: &str, default: bool) -> bool {
    args.get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(default)
}

fn integer_arg(args: &serde_json::Value, key: &str, default: i64) -> i64 {
    args.get(key)
        .and_then(|value| value.as_i64())
        .unwrap_or(default)
}

impl PluginHost {
    /// Build the [`CommandDispatchEnv`] handle plugins are handed at
    /// API install time. Cheap-clone (every member is `Rc`-backed); the
    /// host keeps its own clones in `Self`.
    pub(super) fn command_dispatch_env(&self) -> CommandDispatchEnv {
        CommandDispatchEnv {
            commands: Rc::clone(&self.command_registry),
            plugin_registry: self.command_plugin_registry.clone(),
            diagnostics: self.diagnostics.clone(),
            pending_events: self.pending_command_events.clone(),
            budget_tracker: self.budget_tracker.clone(),
        }
    }

    pub(super) fn register_builtin_host_commands(&mut self) {
        let commands_to_register = [
            (
                "repository.fetch",
                "Repository: Fetch",
                "Fetch from the active remote.",
            ),
            (
                "repository.refresh",
                "Repository: Refresh",
                "Refresh the active repository projection.",
            ),
            (
                "repository.open",
                "Repository: Open",
                "Open a repository tab by path.",
            ),
        ];
        for (name, title, description) in commands_to_register {
            let descriptor = CommandDescriptor {
                name: name.into(),
                title: title.into(),
                description: description.into(),
                plugin_id: HOST_COMMAND_PLUGIN_ID.into(),
                generation_id: None,
                context: CommandContext::GLOBAL.into(),
                args: Vec::new(),
                destructive: false,
                capabilities: Vec::new(),
                run: CommandBody::Host(Box::new(|_args| Ok(()))),
            };
            self.command_registry
                .borrow_mut()
                .register(descriptor, &self.diagnostics);
        }

        // Register `git.<op>` host commands so command palettes and
        // keymaps can route through `leviathan.command.invoke(...)`.
        // Bodies use the same GitOpsContext funnel as `leviathan.git.*`.
        for spec in builtin_git_command_specs() {
            let git_ctx = self.git_ops_context();
            let pending_events = self.pending_git_events.clone();
            let command_name = spec.name.to_string();
            let background = spec.background;
            let descriptor = CommandDescriptor {
                name: spec.name.into(),
                title: spec.title.into(),
                description: spec.description.into(),
                plugin_id: HOST_COMMAND_PLUGIN_ID.into(),
                generation_id: None,
                context: CommandContext::GLOBAL.into(),
                args: spec.args,
                destructive: spec.destructive,
                capabilities: Vec::new(),
                run: CommandBody::Host(Box::new(move |args| {
                    let request = git_request_from_command(&command_name, args)?;
                    if background {
                        let git_ctx = git_ctx.clone();
                        let pending_events = pending_events.clone();
                        std::thread::spawn(move || {
                            let exec = git_ctx.execute(
                                HOST_COMMAND_PLUGIN_ID,
                                GenerationId::new(0),
                                request,
                                |_| Ok(()),
                            );
                            pending_events.push_many(exec.events.fires);
                        });
                        Ok(())
                    } else {
                        let exec = git_ctx.execute(
                            HOST_COMMAND_PLUGIN_ID,
                            GenerationId::new(0),
                            request,
                            |_| Ok(()),
                        );
                        pending_events.push_many(exec.events.fires);
                        match exec.error {
                            None => Ok(()),
                            Some(err) => Err(err),
                        }
                    }
                })),
            };
            self.command_registry
                .borrow_mut()
                .register(descriptor, &self.diagnostics);
        }
    }

    /// Register the ten devtools commands. Each body
    /// captures the host's
    /// [`crate::plugin::devtools_commands::DevtoolsActionQueue`] so
    /// the body can queue an action for the host to apply (with
    /// `&mut self`) after dispatch returns. Called once from
    /// [`PluginHost::new`] after the command registry builtin commands.
    pub(super) fn register_builtin_devtools_commands(&mut self) {
        crate::plugin::devtools_commands::register(
            &mut self.command_registry.borrow_mut(),
            Rc::clone(&self.devtools_action_queue),
            &self.diagnostics,
        );
    }

    /// devtools entry point: invoke a devtools command by name and
    /// return its structured JSON result. Routes through the same
    /// [`PluginHost::invoke_command`] funnel as every other
    /// dispatched command so palette listing, schema validation,
    /// capability gating, audit + diagnostic emission stay uniform.
    /// After dispatch returns, queued
    /// [`crate::plugin::devtools_commands::DevtoolsAction`]s are
    /// drained and applied with `&mut self`; this wrapper keeps the
    /// last action's result and returns it alongside the outcome.
    /// Returns `(InvokeOutcome, Value::Null)` when the dispatch
    /// failed before the body ran.
    pub fn invoke_devtools_command(
        &mut self,
        name: &str,
        args: serde_json::Value,
    ) -> (InvokeOutcome, serde_json::Value) {
        let outcome = self.invoke_command(name, args);
        let value = self
            .last_devtools_result
            .take()
            .unwrap_or(serde_json::Value::Null);
        (outcome, value)
    }

    /// Apply every queued devtools action against `&mut self`. The
    /// last action's result is stashed in `self.last_devtools_result`
    /// for `invoke_devtools_command` to return. Empty queue is a
    /// no-op (e.g. the body short-circuited on bad args).
    pub(super) fn drain_devtools_actions(&mut self) {
        let actions = std::mem::take(&mut *self.devtools_action_queue.borrow_mut());
        for action in actions {
            let result = self.apply_devtools_action(action);
            self.last_devtools_result = Some(result);
        }
    }

    pub(super) fn apply_devtools_action(
        &mut self,
        action: crate::plugin::devtools_commands::DevtoolsAction,
    ) -> serde_json::Value {
        match action.name.as_str() {
            "plugin.reload" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                match self.reload_plugin(&plugin_id) {
                    Ok(()) => serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                    }),
                    Err(e) => serde_json::json!({
                        "ok": false,
                        "plugin_id": plugin_id,
                        "error": e.to_string(),
                    }),
                }
            }
            "plugin.disable" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                let unloaded = self.disable_plugin(&plugin_id);
                serde_json::json!({
                    "ok": true,
                    "plugin_id": plugin_id,
                    "unloaded": unloaded,
                })
            }
            "plugin.enable" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                match self.enable_plugin(&plugin_id) {
                    Ok(reloaded) => serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                        "reloaded": reloaded,
                    }),
                    Err(e) => serde_json::json!({
                        "ok": false,
                        "plugin_id": plugin_id,
                        "error": e,
                    }),
                }
            }
            "plugin.open_log" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                match self.plugin_log_path(&plugin_id) {
                    Some(p) => serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                        "path": p,
                    }),
                    None => {
                        self.record_unknown_plugin("plugin.open_log", &plugin_id);
                        serde_json::json!({
                            "ok": false,
                            "plugin_id": plugin_id,
                            "error": "plugin not loaded",
                        })
                    }
                }
            }
            "plugin.inspect_ui_tree" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                match self.widget_ast_inventory(&plugin_id) {
                    Some(value) => serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                        "inventory": value,
                    }),
                    None => {
                        self.record_unknown_plugin("plugin.inspect_ui_tree", &plugin_id);
                        serde_json::json!({
                            "ok": false,
                            "plugin_id": plugin_id,
                            "error": "plugin not loaded",
                        })
                    }
                }
            }
            "plugin.run_health_check" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                if !self.plugins.contains_key(&plugin_id) {
                    self.record_unknown_plugin("plugin.run_health_check", &plugin_id);
                    serde_json::json!({
                        "ok": false,
                        "plugin_id": plugin_id,
                        "error": "plugin not loaded",
                    })
                } else {
                    let report = self.run_health_checks();
                    let items = report
                        .for_plugin(&plugin_id)
                        .map(|h| {
                            h.items
                                .iter()
                                .map(|i| {
                                    serde_json::json!({
                                        "severity": match i.severity {
                                            crate::plugin::api::health::Severity::Ok => "ok",
                                            crate::plugin::api::health::Severity::Info => "info",
                                            crate::plugin::api::health::Severity::Warn => "warn",
                                            crate::plugin::api::health::Severity::Error => "error",
                                        },
                                        "message": i.message,
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                        "items": items,
                    })
                }
            }
            "plugin.clear_state" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                let surfaces_raw = action.arg_str("surfaces").unwrap_or("state,cache");
                let surfaces: Vec<String> = surfaces_raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                self.devtools_clear_state(&plugin_id, &surfaces)
            }
            "plugin.export_diagnostic_bundle" => {
                let plugin_id = action.arg_str("plugin_id").map(str::to_string);
                let include_state = action
                    .args
                    .get("include_state")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let include_secrets = action
                    .args
                    .get("include_secrets")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.build_diagnostic_bundle(plugin_id.as_deref(), include_state, include_secrets)
            }
            "plugin.show_capability_audit" => {
                let plugin_id = action.arg_str("plugin_id").map(str::to_string);
                let limit = action
                    .args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50)
                    .min(usize::MAX as u64) as usize;
                let entries = self.audit_log.entries();
                let filtered: Vec<crate::plugin::audit::AuditEntry> = entries
                    .into_iter()
                    .filter(|e| {
                        plugin_id
                            .as_deref()
                            .map(|pid| e.plugin_id == pid)
                            .unwrap_or(true)
                    })
                    .collect();
                let n = filtered.len();
                let start = n.saturating_sub(limit);
                let rows: Vec<serde_json::Value> = filtered[start..]
                    .iter()
                    .map(|e| {
                        let ts = e
                            .timestamp
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or(0);
                        serde_json::json!({
                            "plugin_id": e.plugin_id,
                            "capability": e.capability,
                            "target": e.target,
                            "outcome": match e.outcome {
                                crate::plugin::audit::AuditOutcome::Allowed => "allowed",
                                crate::plugin::audit::AuditOutcome::Denied => "denied",
                            },
                            "timestamp_unix_ms": ts,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "ok": true,
                    "filter_plugin_id": plugin_id,
                    "limit": limit,
                    "entries": rows,
                })
            }
            "plugin.show_runtime_path" => {
                let plugin_id = action.arg_str("plugin_id").unwrap_or("").to_string();
                match self.runtime_path_entries_for(&plugin_id) {
                    Some(rows) => serde_json::json!({
                        "ok": true,
                        "plugin_id": plugin_id,
                        "entries": rows,
                    }),
                    None => {
                        self.record_unknown_plugin("plugin.show_runtime_path", &plugin_id);
                        serde_json::json!({
                            "ok": false,
                            "plugin_id": plugin_id,
                            "error": "plugin not loaded",
                        })
                    }
                }
            }
            other => {
                // Defensive: an unknown action name slipped past the
                // body's allow-list. Surface as a diagnostic so the
                // mismatch is visible.
                let msg = format!("unknown devtools action `{other}`");
                self.diagnostics.record(PluginDiagnostic::new(
                    PluginId::from(HOST_COMMAND_PLUGIN_ID),
                    DiagnosticSeverity::Error,
                    "devtools.command.unknown",
                    msg.clone(),
                ));
                serde_json::json!({
                    "ok": false,
                    "error": msg,
                })
            }
        }
    }

    /// Emit a `devtools.command.unknown_plugin` diagnostic when an
    /// action references a plugin id that isn't loaded (or in the
    /// disabled set).
    pub(super) fn record_unknown_plugin(&self, command: &str, plugin_id: &str) {
        self.diagnostics.record(
            PluginDiagnostic::new(
                PluginId::from(HOST_COMMAND_PLUGIN_ID),
                DiagnosticSeverity::Warning,
                "devtools.command.unknown_plugin",
                format!("devtools command `{command}` references unknown plugin `{plugin_id}`"),
            )
            .with_context(serde_json::json!({
                "command": command,
                "plugin_id": plugin_id,
            })),
        );
    }

    // ----- devtools host helpers consumed by devtools commands -----

    /// Mark `plugin_id` as disabled. If the plugin is currently
    /// loaded, unload it as part of the disable so the user sees
    /// the effect immediately. Returns true when the plugin had to
    /// be unloaded; false when it was already absent.
    pub fn disable_plugin(&mut self, plugin_id: &str) -> bool {
        self.disabled_plugins.insert(plugin_id.to_string());
        self.diagnostics.record(PluginDiagnostic::new(
            PluginId::from(plugin_id),
            DiagnosticSeverity::Info,
            "plugin.disable",
            format!("plugin `{plugin_id}` marked disabled"),
        ));
        if self.plugins.contains_key(plugin_id) {
            // Best effort — unload errors land in diagnostics.
            let _ = self.unload_plugin(plugin_id);
            true
        } else {
            false
        }
    }

    /// Remove `plugin_id` from the disabled set. If the plugin's
    /// last-known root is still recorded, attempt a fresh load.
    /// Returns `Ok(true)` when a reload happened; `Ok(false)` when
    /// the disabled flag was cleared but no root was on file (so the
    /// plugin will be loaded on the next discovery pass).
    pub fn enable_plugin(&mut self, plugin_id: &str) -> Result<bool, String> {
        let was_disabled = self.disabled_plugins.remove(plugin_id);
        self.diagnostics.record(PluginDiagnostic::new(
            PluginId::from(plugin_id),
            DiagnosticSeverity::Info,
            "plugin.enable",
            format!("plugin `{plugin_id}` enabled (was_disabled={was_disabled})"),
        ));
        let Some(root) = self.last_plugin_roots.get(plugin_id).cloned() else {
            return Ok(false);
        };
        if self.plugins.contains_key(plugin_id) {
            return Ok(false);
        }
        self.load_plugin(&root)
            .map(|()| true)
            .map_err(|e| e.to_string())
    }

    /// Returns true when `plugin_id` is currently on the disabled
    /// list.
    pub fn is_plugin_disabled(&self, plugin_id: &str) -> bool {
        self.disabled_plugins.contains(plugin_id)
    }

    /// Cheap-clone snapshot of the disabled-plugins set. Sorted for
    /// stable test output.
    pub fn disabled_plugin_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.disabled_plugins.iter().cloned().collect();
        ids.sort();
        ids
    }

    /// Best-effort log path for a plugin. Today the host doesn't
    /// emit per-plugin log files; the diagnostic store *is* the log,
    /// so we return the storage state path (which devtools can use
    /// as a near-the-plugin location to dump a `diagnostic.log`).
    /// Returns `None` for unknown plugin ids.
    pub fn plugin_log_path(&self, plugin_id: &str) -> Option<String> {
        let plugin = self.plugins.get(plugin_id)?;
        let storage = self.storage_paths(plugin_id, plugin.root.clone());
        Some(
            storage
                .state_dir
                .join("diagnostic.log")
                .display()
                .to_string(),
        )
    }

    /// Read-only manifest accessor. Returns `None` when the plugin
    /// is not loaded.
    pub fn plugin_manifest(
        &self,
        plugin_id: &str,
    ) -> Option<&git_leviathan_plugin_api::manifest::PluginManifest> {
        self.plugins.get(plugin_id).map(|p| &p.manifest)
    }

    /// JSON projection of a plugin's widget AST inventory. Includes
    /// every slot, the active screen widget tree (if any), and every
    /// overlay / context-menu / decoration this plugin owns.
    /// Returns `None` for unknown plugin ids.
    pub fn widget_ast_inventory(&self, plugin_id: &str) -> Option<serde_json::Value> {
        if !self.plugins.contains_key(plugin_id) {
            return None;
        }
        // Slots owned by this plugin. We walk `slot_ops` the same way
        // `introspect()` does to materialise the live set, then filter
        // to `plugin_id`.
        let mut slot_map: std::collections::BTreeMap<(String, String, String), serde_json::Value> =
            std::collections::BTreeMap::new();
        for op in &self.slot_ops {
            match op {
                PreparedSlotOp::Add(p) if p.plugin_id == plugin_id => {
                    let key = (p.region.clone(), p.container.key(), p.id.clone());
                    slot_map.insert(
                        key,
                        serde_json::json!({
                            "id": p.id,
                            "region": p.region,
                            "container": p.container.key(),
                            "priority": p.priority,
                            "kind": match &p.widget {
                                crate::plugin::ui::main_bar_slots::SlotWidget::Static(_) => "static",
                                crate::plugin::ui::main_bar_slots::SlotWidget::Dynamic(_) => "dynamic",
                            },
                        }),
                    );
                }
                PreparedSlotOp::Replace {
                    region,
                    container,
                    id,
                    spec,
                } if spec.plugin_id == plugin_id => {
                    let key = (region.clone(), container.key(), id.clone());
                    slot_map.insert(
                        key,
                        serde_json::json!({
                            "id": id,
                            "region": region,
                            "container": container.key(),
                            "priority": spec.priority,
                            "kind": match &spec.widget {
                                crate::plugin::ui::main_bar_slots::SlotWidget::Static(_) => "static",
                                crate::plugin::ui::main_bar_slots::SlotWidget::Dynamic(_) => "dynamic",
                            },
                        }),
                    );
                }
                _ => {}
            }
        }
        let slots: Vec<serde_json::Value> = slot_map.into_values().collect();
        let overlays: Vec<serde_json::Value> = self
            .extension_registry
            .overlays()
            .into_iter()
            .filter(|o| o.plugin_id == plugin_id)
            .map(|o| {
                serde_json::json!({
                    "id": o.id,
                    "priority": o.priority,
                    "dismissible": o.dismissible,
                    "widget_kind": o.widget.node.kind(),
                })
            })
            .collect();
        let context_menu: Vec<serde_json::Value> = self
            .extension_registry
            .all_context_menu_items()
            .into_iter()
            .filter(|c| c.plugin_id == plugin_id)
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "region": c.region,
                    "label": c.label,
                    "command": c.command,
                    "priority": c.priority,
                })
            })
            .collect();
        let active_screen = self
            .active_screen
            .as_ref()
            .filter(|(pid, _)| pid == plugin_id)
            .map(|(_, screen)| {
                serde_json::json!({
                    "screen_id": screen,
                    "has_widget_tree": self.widget_tree.is_some(),
                    "widget_kind": self.widget_tree.as_ref().map(|t| t.node.kind()),
                })
            });
        Some(serde_json::json!({
            "slots": slots,
            "overlays": overlays,
            "context_menu": context_menu,
            "active_screen": active_screen,
        }))
    }

    /// Apply a `Plugin: Clear State` request. Iterates the requested
    /// surfaces, clearing each. Even when `secrets` appears in the
    /// list, the clear path goes through `reset_plugin_storage` so
    /// the same audit / fs path used by every other surface applies.
    pub(super) fn devtools_clear_state(
        &mut self,
        plugin_id: &str,
        surfaces: &[String],
    ) -> serde_json::Value {
        if !self.plugins.contains_key(plugin_id) {
            self.record_unknown_plugin("plugin.clear_state", plugin_id);
            return serde_json::json!({
                "ok": false,
                "plugin_id": plugin_id,
                "error": "plugin not loaded",
            });
        }
        let mut cleared: Vec<String> = Vec::new();
        let mut errors: Vec<serde_json::Value> = Vec::new();
        for surface in surfaces {
            match self.reset_plugin_storage(plugin_id, surface) {
                Ok(()) => cleared.push(surface.clone()),
                Err(e) => errors.push(serde_json::json!({
                    "surface": surface,
                    "error": e,
                })),
            }
        }
        serde_json::json!({
            "ok": errors.is_empty(),
            "plugin_id": plugin_id,
            "cleared": cleared,
            "errors": errors,
        })
    }

    /// Build a [`crate::plugin::diagnostic_bundle::DiagnosticBundle`]
    /// for `plugin_id` (or the host as a whole when `None`). Always
    /// excludes raw secret values; with `include_secrets = true`,
    /// only secret *keys* (metadata) are added.
    pub fn build_diagnostic_bundle(
        &self,
        plugin_id: Option<&str>,
        include_state: bool,
        include_secrets: bool,
    ) -> serde_json::Value {
        use crate::plugin::audit::AuditOutcome;
        let snap = self.introspect();
        let generated_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let host_version = env!("CARGO_PKG_VERSION").to_string();

        let manifest_json = plugin_id.and_then(|pid| {
            self.plugin_manifest(pid)
                .and_then(|m| serde_json::to_value(m).ok())
        });
        let api_version = plugin_id.and_then(|pid| {
            self.plugin_manifest(pid)
                .map(|m| format!("{}.{}", m.api_version.major, m.api_version.minor))
        });
        let installed_capabilities: Vec<String> = match plugin_id {
            Some(pid) => self
                .plugin_manifest(pid)
                .map(|m| m.capabilities.iter().cloned().map(String::from).collect())
                .unwrap_or_default(),
            None => snap
                .plugins
                .iter()
                .flat_map(|p| p.capabilities.iter().cloned())
                .collect(),
        };
        let diagnostics: Vec<serde_json::Value> = snap
            .diagnostics
            .iter()
            .filter(|d| {
                plugin_id
                    .map(|pid| d.plugin_id == pid || d.plugin_id == HOST_COMMAND_PLUGIN_ID)
                    .unwrap_or(true)
            })
            .map(|d| {
                serde_json::json!({
                    "plugin_id": d.plugin_id,
                    "generation_id": d.generation_id,
                    "severity": d.severity,
                    "code": d.code,
                    "message": d.message,
                    "source": d.source,
                    "context": d.context,
                    "timestamp_unix_ms": d.timestamp_unix_ms,
                })
            })
            .collect();
        let recent_audit: Vec<serde_json::Value> = snap
            .audit_recent
            .iter()
            .filter(|e| plugin_id.map(|pid| e.plugin_id == pid).unwrap_or(true))
            .map(|e| {
                let ts = e
                    .timestamp
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                serde_json::json!({
                    "plugin_id": e.plugin_id,
                    "capability": e.capability,
                    "target": e.target,
                    "outcome": match e.outcome {
                        AuditOutcome::Allowed => "allowed",
                        AuditOutcome::Denied => "denied",
                    },
                    "timestamp_unix_ms": ts,
                })
            })
            .collect();
        let reload_history: Vec<serde_json::Value> = snap
            .reload_history
            .iter()
            .filter(|e| plugin_id.map(|pid| e.plugin_id == pid).unwrap_or(true))
            .map(|e| {
                serde_json::json!({
                    "plugin_id": e.plugin_id,
                    "from_generation_id": e.from_generation_id,
                    "to_generation_id": e.to_generation_id,
                    "outcome": e.outcome.as_str(),
                    "duration_ms": e.duration_ms,
                    "stage_reached": e.stage_reached,
                    "error_code": e.error_code,
                    "error_message": e.error_message,
                    "timestamp_unix_ms": e.timestamp_unix_ms,
                })
            })
            .collect();
        let performance_traces: Vec<serde_json::Value> = snap
            .performance_traces
            .iter()
            .filter(|t| plugin_id.map(|pid| t.plugin_id == pid).unwrap_or(true))
            .map(|t| {
                serde_json::json!({
                    "plugin_id": t.plugin_id,
                    "generation_id": t.generation_id,
                    "callback_id": t.callback_id,
                    "kind": t.kind,
                    "duration_ms": t.duration_ms,
                    "ok": t.ok,
                    "timestamp_unix_ms": t.timestamp_unix_ms,
                })
            })
            .collect();
        let circuit_breakers: Vec<serde_json::Value> = snap
            .circuit_breakers
            .iter()
            .filter(|c| plugin_id.map(|pid| c.plugin_id == pid).unwrap_or(true))
            .map(|c| {
                serde_json::json!({
                    "plugin_id": c.plugin_id,
                    "generation_id": c.generation_id,
                    "callback_id": c.callback_id,
                    "kind": c.kind,
                    "state": c.state,
                    "consecutive_failures": c.consecutive_failures,
                    "count": c.count,
                    "ok_count": c.ok_count,
                    "err_count": c.err_count,
                    "p50_ms": c.p50_ms,
                    "p95_ms": c.p95_ms,
                    "last_failure": c.last_failure,
                })
            })
            .collect();

        // Plugin state (when requested). Walks every storage surface
        // EXCEPT secrets; secret values must never appear in the
        // bundle. The walk reads the surface's path through
        // `surface_dir` and collects file paths + sizes; values for
        // small JSON files are inlined.
        let state = if include_state {
            Some(self.collect_plugin_state(plugin_id))
        } else {
            None
        };

        // Secret metadata (when requested). Only the path + key
        // names are copied; secret *values* are never read into the
        // bundle. This is the same shape `SecretSummary` already
        // exposes through `introspect()`.
        let secret_metadata: Vec<serde_json::Value> = if include_secrets {
            snap.secrets
                .iter()
                .filter(|s| plugin_id.map(|pid| s.plugin_id == pid).unwrap_or(true))
                .map(|s| {
                    serde_json::json!({
                        "plugin_id": s.plugin_id,
                        "path": s.path,
                        "key_count": s.key_count,
                        "keys": s.keys,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        serde_json::json!({
            "generated_at_unix_ms": generated_at_unix_ms,
            "host_version": host_version,
            "plugin_id": plugin_id,
            "manifest": manifest_json,
            "api_version": api_version,
            "installed_capabilities": installed_capabilities,
            "diagnostics": diagnostics,
            "recent_audit": recent_audit,
            "reload_history": reload_history,
            "performance_traces": performance_traces,
            "circuit_breakers": circuit_breakers,
            "state": state,
            "include_secrets": include_secrets,
            "secret_metadata": secret_metadata,
        })
    }

    pub(super) fn collect_plugin_state(&self, plugin_id: Option<&str>) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        for (id, plugin) in &self.plugins {
            if let Some(pid) = plugin_id {
                if id != pid {
                    continue;
                }
            }
            let storage = self.storage_paths(id, plugin.root.clone());
            let mut surfaces = serde_json::Map::new();
            for surface in [
                StorageSurface::State,
                StorageSurface::Cache,
                StorageSurface::Config,
                StorageSurface::Repo,
                StorageSurface::Settings,
            ] {
                let path = match surface {
                    StorageSurface::Settings => storage.settings_path(),
                    _ => storage.surface_dir(surface),
                };
                let value = read_state_tree(&path);
                surfaces.insert(surface.as_str().to_string(), value);
            }
            out.insert(id.clone(), serde_json::Value::Object(surfaces));
        }
        serde_json::Value::Object(out)
    }

    /// Runtime-path rows for `plugin_id`. Returns `None` when the
    /// plugin is not loaded.
    pub(super) fn runtime_path_entries_for(
        &self,
        plugin_id: &str,
    ) -> Option<Vec<serde_json::Value>> {
        let plugin = self.plugins.get(plugin_id)?;
        Some(
            plugin
                .lua_loader
                .runtime_path()
                .entries()
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "entry_plugin_id": entry.plugin_id,
                        "kind": entry.kind.as_str(),
                        "root": entry.lua_root.display().to_string(),
                    })
                })
                .collect(),
        )
    }

    /// Public Rust dispatch entry point used by UI buttons, keymaps
    /// (keymaps), and tests. Routes through the same
    /// [`commands::dispatch_command`] funnel as the Lua API so all
    /// entry paths share a single dispatcher (command registry acceptance
    /// gate). Returns the structured outcome unchanged.
    pub fn invoke_command(&mut self, name: &str, args: serde_json::Value) -> InvokeOutcome {
        // lazy loading: when no live command matches but a lazy plugin
        // declared this command, activate the plugin and re-dispatch
        // through the now-live registry. The activation entry path
        // does its own re-dispatch and returns the final outcome.
        // Gated to test builds: the binary doesn't yet wire commands
        // through this entry point, and `activate_plugin_for_command`
        // is only reachable from tests.
        #[cfg(test)]
        {
            let live_has = self
                .command_registry
                .borrow()
                .entries()
                .iter()
                .any(|e| e.descriptor.name == name);
            let lazy_has = self
                .lazy_registry
                .entries()
                .iter()
                .filter(|e| e.status == crate::plugin::activation::LazyStatus::Lazy)
                .any(|e| e.commands.iter().any(|c| c == name));
            if !live_has && lazy_has {
                if let Some(outcome) = self.activate_plugin_for_command(name, args.clone()) {
                    return outcome;
                }
            }
        }
        let env = self.command_dispatch_env();
        let outcome = commands::dispatch_command(&env, name, args);
        // Drain queued events immediately so tests that observe
        // `CommandExecuted` after a single call see the firing
        // synchronously without needing a separate `tick()`.
        self.flush_pending_command_events();
        // Any host git command may have queued typed events
        // (HeadChanged etc.) — fire them in the same flush so plugin
        // autocmds see post-op state before this returns.
        self.flush_pending_git_events();
        // When the dispatched command was a host devtools
        // command, drain queued devtools actions and apply each
        // against `&mut self`. This wires the devtools surface into
        // the same `invoke_command` entry point every other caller
        // uses (palette / keymap / Lua API) so the production bin
        // automatically picks up devtools functionality without a
        // separate dispatch path.
        if crate::plugin::devtools_commands::devtools_command_names().contains(&name) {
            self.drain_devtools_actions();
        }
        outcome
    }

    pub(super) fn drain_lua_command_effects(&mut self) {
        self.flush_pending_command_events();
        self.flush_pending_git_events();
        self.drain_devtools_actions();
    }

    pub(super) fn flush_pending_command_events(&mut self) {
        let events = self.pending_command_events.drain();
        for event in events {
            let mut payload = EventPayload::new();
            payload.insert("name".into(), serde_json::Value::String(event.name.clone()));
            payload.insert(
                "plugin_id".into(),
                serde_json::Value::String(event.plugin_id.clone()),
            );
            payload.insert("ok".into(), serde_json::Value::Bool(event.ok));
            // `duration_ms` isn't in the descriptor's required field
            // set, but plugins can still observe it through the
            // generic payload pass-through.
            payload.insert(
                "duration_ms".into(),
                serde_json::Value::Number(serde_json::Number::from(event.duration_ms as u64)),
            );
            self.fire_event_typed("CommandExecuted", payload);
        }
    }

    pub fn command_registry(&self) -> Rc<RefCell<CommandRegistry>> {
        Rc::clone(&self.command_registry)
    }

    pub fn keymap_registry(&self) -> Rc<RefCell<KeymapRegistry>> {
        Rc::clone(&self.keymap_registry)
    }

    /// Set the active leader prefix. Empty leader is rejected by the
    /// parser at use time. Tests pin it to `,` so `<leader>gh` parses
    /// to a deterministic chord.
    pub fn set_keymap_leader(&mut self, leader: impl Into<String>) {
        self.keymap_registry.borrow_mut().set_leader(leader);
    }

    pub fn set_user_keymap(
        &mut self,
        context: impl Into<String>,
        key: &str,
        command: impl Into<String>,
    ) {
        self.keymap_registry.borrow_mut().set_user_keymap(
            context,
            key,
            command,
            serde_json::Value::Null,
            String::new(),
            &self.diagnostics,
        );
    }

    /// Insert a built-in (host-owned) binding. Same tier as
    /// `Ctrl+Tab` and friends — beats every plugin / user binding for
    /// the same `(context, chord)`.
    pub fn set_builtin_keymap(
        &mut self,
        context: impl Into<String>,
        key: &str,
        command: impl Into<String>,
        description: impl Into<String>,
    ) {
        self.keymap_registry.borrow_mut().set_builtin(
            context,
            key,
            command,
            serde_json::Value::Null,
            description,
            &self.diagnostics,
        );
    }

    /// keymaps dispatch entry. Pass the active context and the
    /// in-flight keystroke buffer; the host walks the resolver and,
    /// on a match, routes through the command dispatcher.
    /// Returns the structured outcome the live `src/app/input.rs`
    /// (or a test driver) consults to decide whether to keep
    /// buffering, dispatch, or fall back to built-in app handling.
    pub fn dispatch_key(&mut self, context: &str, buffer: &[Keystroke]) -> KeymapDispatchOutcome {
        // lazy loading: when the live registry has no entry but a lazy
        // plugin's `[activation.keymaps]` declared this chord, route
        // through the activation entry. After activation, re-dispatch
        // through the live registry. `match_chord` ignores
        // conflict-lost rows, so probing it here is safe. Test-only
        // because the binary's input layer doesn't yet flow chords
        // through `dispatch_key`.
        #[cfg(test)]
        {
            let live_match = self.keymap_registry.borrow().match_chord(context, buffer);
            if matches!(live_match, keymap_mod::MatchOutcome::None) && !buffer.is_empty() {
                // lazy loading: parse each lazy entry's declared key with
                // the active leader and compare against the buffer.
                // The manifest carries the lhs verbatim (`<leader>l`),
                // so we can't shortcut via `render_chord` (which spells
                // out the leader explicitly).
                let leader = self.keymap_registry.borrow().leader().to_string();
                let lazy_match: Option<(String, String)> = self
                    .lazy_registry
                    .entries()
                    .iter()
                    .filter(|e| e.status == crate::plugin::activation::LazyStatus::Lazy)
                    .find_map(|entry| {
                        entry
                            .keymaps
                            .iter()
                            .filter(|k| k.context == context)
                            .find_map(|k| {
                                let parsed =
                                    keymap_mod::parse_key_sequence(&k.key, &leader).ok()?;
                                if parsed == buffer {
                                    Some((entry.plugin_id.clone(), k.key.clone()))
                                } else {
                                    None
                                }
                            })
                    });
                if let Some((_plugin_id, key)) = lazy_match {
                    if let Some(outcome) = self.activate_plugin_for_keymap(context, &key) {
                        if let KeymapDispatchOutcome::Dispatched {
                            context: ref ctx,
                            ref key,
                            ref command,
                            ref plugin_id,
                            ref outcome,
                        } = outcome
                        {
                            let payload = keymap_mod::keymap_triggered_payload(
                                ctx,
                                key,
                                command,
                                plugin_id,
                                matches!(outcome, InvokeOutcome::Ok),
                            );
                            self.fire_event_typed("KeymapTriggered", payload);
                        }
                        return outcome;
                    }
                }
            }
        }
        let env = self.command_dispatch_env();
        let outcome =
            self.keymap_registry
                .borrow()
                .dispatch(context, buffer, &env, &self.diagnostics);
        // Drain queued `CommandExecuted` events so subscribers see
        // them synchronously, same as `invoke_command` does.
        self.flush_pending_command_events();
        // Keymap-triggered git commands queue typed events
        // through the shared pending-git-events handle; flush so the
        // `HeadChanged` / `RefsChanged` etc. fire before this returns.
        self.flush_pending_git_events();
        if let KeymapDispatchOutcome::Dispatched {
            context: ref ctx,
            ref key,
            ref command,
            ref plugin_id,
            ref outcome,
        } = outcome
        {
            let payload = keymap_mod::keymap_triggered_payload(
                ctx,
                key,
                command,
                plugin_id,
                matches!(outcome, InvokeOutcome::Ok),
            );
            self.fire_event_typed("KeymapTriggered", payload);
        }
        outcome
    }

    pub(super) fn record_reload_event(&mut self, event: ReloadEventSummary) {
        let bucket = self
            .reload_history
            .entry(event.plugin_id.clone())
            .or_default();
        if bucket.len() == RELOAD_HISTORY_CAP {
            bucket.pop_front();
        }
        bucket.push_back(event);
    }
}

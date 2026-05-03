impl PluginHost {
    pub fn load_from_default_dirs(&mut self) {
        let cwd_plugins = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("plugins");
        if cwd_plugins.is_dir() {
            self.load_from_dir(&cwd_plugins);
        }
    }

    /// Discover every plugin under `dir`, resolve their dependency
    /// graph, then load successful plugins in deterministic order.
    /// Reads `plugins.lock` (and `plugins.lock.local`) from `dir` to
    /// validate checksums; rewrites `plugins.lock` afterwards so the
    /// next run sees the resolved set. Failures emit structured
    /// diagnostics; this entry never returns a `Result` because the
    /// startup path needs to keep going past per-plugin failures.
    pub fn load_from_dir(&mut self, dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut candidate_dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.join("plugin.toml").exists() && p.join("init.lua").exists())
            .collect();
        candidate_dirs.sort();
        self.resolve_and_load(dir, &candidate_dirs);
    }

    /// dependency resolver entry point. `lockfile_dir` is the root used
    /// for `plugins.lock` / `plugins.lock.local`; `candidate_dirs`
    /// is the set of plugin directories to consider for loading.
    pub fn resolve_and_load(&mut self, lockfile_dir: &Path, candidate_dirs: &[PathBuf]) {
        use crate::plugin::dependency::{
            resolve, BlockReason, OptionalMissReason, ResolutionReport,
        };
        use crate::plugin::lockfile::{
            compute_plugin_checksum, LockedPlugin, Lockfile, LOCAL_OVERRIDE_NAME, LOCKFILE_NAME,
        };

        // 1) Read manifests, drop dirs whose manifest fails to parse
        //    (load_plugin's own diagnostic path runs at load time, but
        //    here we surface a separate code so callers can tell the
        //    difference between "discovery failed" and "load failed").
        let mut manifests: Vec<PluginManifest> = Vec::new();
        let mut dir_by_id: HashMap<String, PathBuf> = HashMap::new();
        for plugin_dir in candidate_dirs {
            let manifest_path = plugin_dir.join("plugin.toml");
            let manifest_str = match fs::read_to_string(&manifest_path) {
                Ok(s) => s,
                Err(e) => {
                    let id = plugin_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("<unknown>")
                        .to_string();
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(id),
                            DiagnosticSeverity::Error,
                            "manifest.read_failed",
                            format!("could not read plugin.toml: {e}"),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: manifest_path.display().to_string(),
                            key: None,
                        }),
                    );
                    continue;
                }
            };
            let manifest: PluginManifest = match toml::from_str(&manifest_str) {
                Ok(m) => m,
                Err(e) => {
                    let id = plugin_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("<unknown>")
                        .to_string();
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(id),
                            DiagnosticSeverity::Error,
                            "manifest.parse_failed",
                            format!("invalid plugin.toml: {e}"),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: manifest_path.display().to_string(),
                            key: None,
                        }),
                    );
                    continue;
                }
            };
            dir_by_id.insert(manifest.id.clone(), plugin_dir.clone());
            manifests.push(manifest);
        }

        // 2) Resolve.
        let report: ResolutionReport = resolve(&manifests);
        self.dependency_graph = report.graph.clone();

        // 3) Diagnose blocked plugins. Each (plugin, reason) becomes
        //    its own diagnostic so devtools can group and dedupe.
        for (plugin_id, reasons) in &report.blocked {
            let path = dir_by_id
                .get(plugin_id)
                .map(|p| p.join("plugin.toml").display().to_string())
                .unwrap_or_else(|| format!("plugins/{plugin_id}/plugin.toml"));
            for reason in reasons {
                let (code, message, ctx) = match reason {
                    BlockReason::MissingRequired {
                        dependency_id,
                        requirement,
                    } => (
                        "dependency.missing_required",
                        format!(
                            "plugin `{plugin_id}` requires `{dependency_id} {requirement}` but it is not present"
                        ),
                        serde_json::json!({
                            "dependency": dependency_id,
                            "requirement": requirement,
                        }),
                    ),
                    BlockReason::Conflict {
                        dependency_id,
                        requirement,
                        actual_version,
                    } => (
                        "dependency.conflict",
                        format!(
                            "plugin `{plugin_id}` requires `{dependency_id} {requirement}` but found `{actual_version}`"
                        ),
                        serde_json::json!({
                            "dependency": dependency_id,
                            "requirement": requirement,
                            "actual_version": actual_version,
                        }),
                    ),
                    BlockReason::Cycle { cycle } => (
                        "dependency.cycle",
                        format!(
                            "plugin `{plugin_id}` participates in dependency cycle [{}]",
                            cycle.join(" -> ")
                        ),
                        serde_json::json!({ "cycle": cycle }),
                    ),
                    BlockReason::BlockedTransitive { dependency_id } => (
                        "dependency.blocked_transitive",
                        format!(
                            "plugin `{plugin_id}` blocked because dependency `{dependency_id}` is blocked"
                        ),
                        serde_json::json!({ "dependency": dependency_id }),
                    ),
                };
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id.clone()),
                        DiagnosticSeverity::Error,
                        code,
                        message,
                    )
                    .with_source(PluginSourceSpan::Manifest {
                        path: path.clone(),
                        key: Some("dependencies".to_string()),
                    })
                    .with_context(ctx),
                );
            }
        }
        for miss in &report.optional_misses {
            let path = dir_by_id
                .get(&miss.consumer_plugin_id)
                .map(|p| p.join("plugin.toml").display().to_string())
                .unwrap_or_else(|| format!("plugins/{}/plugin.toml", miss.consumer_plugin_id));
            let (msg, extra) = match &miss.reason {
                OptionalMissReason::NotPresent => (
                    format!(
                        "optional dependency `{} {}` not present; `{}` will load without it",
                        miss.dependency_id, miss.requirement, miss.consumer_plugin_id
                    ),
                    serde_json::json!({ "reason": "not_present" }),
                ),
                OptionalMissReason::VersionMismatch { actual_version } => (
                    format!(
                        "optional dependency `{} {}` present at `{}`, requirement not met; `{}` loads without it",
                        miss.dependency_id, miss.requirement, actual_version, miss.consumer_plugin_id
                    ),
                    serde_json::json!({
                        "reason": "version_mismatch",
                        "actual_version": actual_version,
                    }),
                ),
            };
            let mut ctx = serde_json::json!({
                "dependency": miss.dependency_id,
                "requirement": miss.requirement,
            });
            if let (Some(obj), Some(extra_obj)) = (ctx.as_object_mut(), extra.as_object()) {
                for (k, v) in extra_obj {
                    obj.insert(k.clone(), v.clone());
                }
            }
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(miss.consumer_plugin_id.clone()),
                    DiagnosticSeverity::Warning,
                    "dependency.optional_missing",
                    msg,
                )
                .with_source(PluginSourceSpan::Manifest {
                    path,
                    key: Some("optional_dependencies".to_string()),
                })
                .with_context(ctx),
            );
        }

        // 4) Read lockfile + local override (if either exists). Apply
        //    overrides on top, then validate checksums against the
        //    plugins about to load. Mismatches and unknown ids emit
        //    diagnostics; they don't block the load — the next write
        //    will refresh the lockfile to match disk.
        let lock_path = lockfile_dir.join(LOCKFILE_NAME);
        let local_path = lockfile_dir.join(LOCAL_OVERRIDE_NAME);
        let mut effective_lock = if lock_path.is_file() {
            match Lockfile::read(&lock_path) {
                Ok(l) => l,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from("<lockfile>"),
                            DiagnosticSeverity::Warning,
                            "lockfile.read_failed",
                            format!("could not read plugins.lock: {e}"),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: lock_path.display().to_string(),
                            key: None,
                        }),
                    );
                    Lockfile::new()
                }
            }
        } else {
            Lockfile::new()
        };
        if local_path.is_file() {
            match Lockfile::read(&local_path) {
                Ok(overlay) => effective_lock.apply_overlay(&overlay),
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from("<lockfile>"),
                            DiagnosticSeverity::Warning,
                            "lockfile.read_failed",
                            format!("could not read plugins.lock.local: {e}"),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: local_path.display().to_string(),
                            key: None,
                        }),
                    );
                }
            }
        }

        let live_ids: HashSet<String> = report.load_order.iter().cloned().collect();
        for entry in &effective_lock.plugins {
            if !live_ids.contains(&entry.id) {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(entry.id.clone()),
                        DiagnosticSeverity::Warning,
                        "lockfile.unknown_plugin",
                        format!(
                            "plugins.lock pins `{}` but no plugin with that id is present; entry will be dropped on next write",
                            entry.id
                        ),
                    )
                    .with_source(PluginSourceSpan::Manifest {
                        path: lock_path.display().to_string(),
                        key: Some("plugin".to_string()),
                    })
                    .with_context(serde_json::json!({ "version": entry.version })),
                );
            }
        }

        // 5) Validate checksums for plugins we're about to load.
        let mut new_lock_entries: Vec<LockedPlugin> = Vec::new();
        for plugin_id in &report.load_order {
            let Some(plugin_dir) = dir_by_id.get(plugin_id) else {
                continue;
            };
            let manifest = manifests
                .iter()
                .find(|m| &m.id == plugin_id)
                .expect("manifest present for load-order id");
            let checksum = match compute_plugin_checksum(plugin_dir) {
                Ok(c) => c,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Warning,
                            "lockfile.checksum_failed",
                            format!("could not compute plugin checksum: {e}"),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: plugin_dir.display().to_string(),
                            key: None,
                        }),
                    );
                    String::new()
                }
            };
            if let Some(locked) = effective_lock.lookup(plugin_id) {
                if !checksum.is_empty() && locked.checksum != checksum {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Warning,
                            "lockfile.checksum_mismatch",
                            format!(
                                "plugin `{plugin_id}` content changed since plugins.lock was written"
                            ),
                        )
                        .with_source(PluginSourceSpan::Manifest {
                            path: plugin_dir.display().to_string(),
                            key: None,
                        })
                        .with_context(serde_json::json!({
                            "expected": locked.checksum,
                            "actual": checksum,
                            "locked_version": locked.version,
                        })),
                    );
                }
            }
            new_lock_entries.push(LockedPlugin {
                id: plugin_id.clone(),
                version: manifest.version.to_string(),
                source: "local".to_string(),
                checksum,
            });
        }

        // 6) Split into eager / lazy cohorts. Plugins with a non-empty
        //    `[activation]` section are parked in the lazy registry
        //    via `install_lazy_stubs`; everyone else loads eagerly.
        //    `load_plugin` already records its own diagnostics; we
        //    just keep going on per-plugin failures so a misbehaving
        //    plugin can't shadow the rest.
        for plugin_id in &report.load_order {
            let Some(plugin_dir) = dir_by_id.get(plugin_id) else {
                continue;
            };
            let manifest = manifests
                .iter()
                .find(|m| &m.id == plugin_id)
                .expect("manifest present for load-order id");
            if let Some(activation) = manifest.activation.as_ref() {
                if !activation.is_empty() {
                    self.install_lazy_stubs(plugin_id, plugin_dir, activation);
                    continue;
                }
            }
            if let Err(e) = self.load_plugin(plugin_dir) {
                let _ = e;
            }
        }

        // 7) Re-write the lockfile in sorted order.
        let new_lock = Lockfile {
            plugins: new_lock_entries,
        };
        if !new_lock.plugins.is_empty() {
            if let Err(e) = new_lock.write(&lock_path) {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from("<lockfile>"),
                        DiagnosticSeverity::Warning,
                        "lockfile.write_failed",
                        format!("could not write plugins.lock: {e}"),
                    )
                    .with_source(PluginSourceSpan::Manifest {
                        path: lock_path.display().to_string(),
                        key: None,
                    }),
                );
            }
        }
    }
    /// lazy loading: park a plugin in the lazy registry. Records stubs
    /// against a synthetic ledger keyed under `(plugin_id, gen=0)`
    /// so `unload_plugin` can reap them. Validation diagnostics
    /// fire here for unknown events / empty trigger fields. A
    /// plugin with an activation section that has nothing usable
    /// (after validation) falls back to eager-load so users aren't
    /// surprised by a never-activating plugin.
    fn install_lazy_stubs(
        &mut self,
        plugin_id: &str,
        plugin_dir: &Path,
        activation: &git_leviathan_plugin_api::manifest::PluginActivation,
    ) {
        use crate::plugin::activation::{LazyEntry, LazyStatus};

        let manifest_path = plugin_dir.join("plugin.toml");
        let validated = crate::plugin::activation::validate(
            plugin_id,
            &manifest_path,
            activation,
            &self.diagnostics,
        );

        let any_stub = !validated.commands.is_empty()
            || !validated.keymaps.is_empty()
            || !validated.events.is_empty()
            || !validated.regions.is_empty()
            || !validated.files.is_empty()
            || validated.repository_shape.is_some()
            || validated.manual;
        if !any_stub {
            // Empty activation (after validation) — fall back to eager
            // load. We've already recorded any `manifest.activation_invalid`
            // diagnostics in the validate path.
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(plugin_id),
                    DiagnosticSeverity::Warning,
                    "manifest.activation_invalid",
                    format!(
                        "plugin `{plugin_id}` has an [activation] section with no usable triggers; loading eagerly"
                    ),
                )
                .with_source(PluginSourceSpan::Manifest {
                    path: manifest_path.display().to_string(),
                    key: Some("activation".to_string()),
                }),
            );
            if let Err(e) = self.load_plugin(plugin_dir) {
                let _ = e;
            }
            return;
        }

        // Allocate a synthetic ledger so `unload_plugin` (and the
        // standard `cleanup_ledger` walk) can reap stubs uniformly.
        let plugin_id_typed = PluginId::from(plugin_id);
        let synthetic_gen = GenerationId::new(0);
        let ledger = ResourceLedger::new(plugin_id_typed.clone(), synthetic_gen);
        for cmd in &validated.commands {
            ledger.record(
                PluginResourceKind::ActivationStub,
                format!("command:{cmd}"),
                None,
            );
        }
        for km in &validated.keymaps {
            ledger.record(
                PluginResourceKind::ActivationStub,
                format!("keymap:{}:{}", km.context, km.key),
                None,
            );
        }
        for ev in &validated.events {
            ledger.record(
                PluginResourceKind::ActivationStub,
                format!("event:{ev}"),
                None,
            );
        }
        for r in &validated.regions {
            ledger.record(
                PluginResourceKind::ActivationStub,
                format!("region:{r}"),
                None,
            );
        }
        for f in &validated.files {
            ledger.record(
                PluginResourceKind::ActivationStub,
                format!("file:{}", f.display()),
                None,
            );
        }
        if validated.repository_shape.is_some() {
            ledger.record(
                PluginResourceKind::ActivationStub,
                "repository_shape".to_string(),
                None,
            );
        }
        if validated.manual {
            ledger.record(
                PluginResourceKind::ActivationStub,
                "manual".to_string(),
                None,
            );
        }

        self.lazy_ledgers
            .insert(plugin_id.to_string(), ledger.clone());
        self.lazy_registry.insert(LazyEntry {
            plugin_id: plugin_id.to_string(),
            plugin_dir: plugin_dir.to_path_buf(),
            manual: validated.manual,
            commands: validated.commands,
            keymaps: validated.keymaps,
            events: validated.events,
            regions: validated.regions,
            files: validated.files,
            repository_shape: validated.repository_shape,
            status: LazyStatus::Lazy,
            activations: 0,
            last_activation_unix_ms: None,
            last_activation_trigger: None,
            last_error: None,
            consecutive_failures: 0,
        });

        self.diagnostics.record(
            PluginDiagnostic::new(
                plugin_id_typed,
                DiagnosticSeverity::Info,
                "activation.parked",
                format!("plugin `{plugin_id}` parked for lazy activation"),
            )
            .with_source(PluginSourceSpan::Manifest {
                path: manifest_path.display().to_string(),
                key: Some("activation".to_string()),
            }),
        );
        // Drop our ref; ledger persists via `lazy_ledgers`.
        let _ = ledger;
    }

    /// Activate the lazy plugin matching `command_name` and
    /// re-dispatch the command into the live registry. Returns the
    /// dispatch outcome when a match was found; `None` when no
    /// lazy plugin owns the command id.
    ///
    /// Test-only entry point: the binary's command dispatch path is
    /// not yet wired into this funnel, so the binary build hides
    /// the lazy branch entirely.
    #[cfg(test)]
    pub fn activate_plugin_for_command(
        &mut self,
        command_name: &str,
        args: serde_json::Value,
    ) -> Option<InvokeOutcome> {
        let plugin_id = self
            .lazy_registry
            .entries()
            .iter()
            .filter(|e| e.status == crate::plugin::activation::LazyStatus::Lazy)
            .find(|e| e.commands.iter().any(|c| c == command_name))?
            .plugin_id
            .clone();
        match self.activate_now(&plugin_id, "command", format!("command:{command_name}")) {
            Ok(()) => {
                // Re-dispatch directly through the funnel; the lazy
                // registry no longer contains this command, so the
                // outer guard in `invoke_command` won't recurse.
                let env = self.command_dispatch_env();
                let outcome = commands::dispatch_command(&env, command_name, args);
                self.flush_pending_command_events();
                self.flush_pending_git_events();
                Some(outcome)
            }
            Err(_) => None,
        }
    }
    /// Activate a lazy plugin in response to a keymap chord. The
    /// caller has already determined the `(context, key)` pair is
    /// declared on a lazy plugin. After load, we look up the
    /// command id the now-live keymap registry resolves the chord
    /// to and dispatch it. When no live keymap matches (e.g. the
    /// plugin failed to register the binding), records an
    /// `activation.unknown_keymap` diagnostic.
    ///
    /// Test-only entry point — see `activate_plugin_for_command`.
    #[cfg(test)]
    pub fn activate_plugin_for_keymap(
        &mut self,
        context: &str,
        key: &str,
    ) -> Option<KeymapDispatchOutcome> {
        let plugin_id = self
            .lazy_registry
            .entries()
            .iter()
            .filter(|e| e.status == crate::plugin::activation::LazyStatus::Lazy)
            .find(|e| {
                e.keymaps
                    .iter()
                    .any(|k| k.context == context && k.key == key)
            })
            .map(|e| e.plugin_id.clone())?;
        match self.activate_now(&plugin_id, "keymap", format!("keymap:{context}:{key}")) {
            Ok(()) => {
                // Re-dispatch through the unified dispatcher; the lazy
                // entry is gone now so probing the registry directly
                // avoids re-entering the activation path.
                let leader = self.keymap_registry.borrow().leader().to_string();
                let parsed = keymap_mod::parse_key_sequence(key, &leader).unwrap_or_default();
                if parsed.is_empty() {
                    self.diagnostics.record(PluginDiagnostic::new(
                        PluginId::from(plugin_id.as_str()),
                        DiagnosticSeverity::Warning,
                        "activation.unknown_keymap",
                        format!(
                            "lazy keymap `{context}:{key}` could not be parsed after activation"
                        ),
                    ));
                    return Some(KeymapDispatchOutcome::Unhandled);
                }
                let env = self.command_dispatch_env();
                let outcome = self.keymap_registry.borrow().dispatch(
                    context,
                    &parsed,
                    &env,
                    &self.diagnostics,
                );
                self.flush_pending_command_events();
                self.flush_pending_git_events();
                if matches!(outcome, KeymapDispatchOutcome::Unhandled) {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.as_str()),
                            DiagnosticSeverity::Warning,
                            "activation.unknown_keymap",
                            format!(
                                "plugin `{plugin_id}` activated for keymap `{context}:{key}` but did not register the binding"
                            ),
                        ),
                    );
                }
                Some(outcome)
            }
            Err(_) => None,
        }
    }

    /// lazy loading internal activation worker. Removes the plugin's
    /// lazy stubs (ledger entries), runs `load_plugin`, then either
    /// marks the entry `Active` (success) or bumps the failure
    /// counter and records `activation.failed` /
    /// `activation.poisoned`. `trigger_kind` and `trigger_descriptor`
    /// are recorded on the diagnostic context and (on success) on
    /// the lazy registry row so the inspector can see what woke the
    /// plugin up.
    fn activate_now(
        &mut self,
        plugin_id: &str,
        trigger_kind: &str,
        trigger_descriptor: String,
    ) -> Result<(), String> {
        use crate::plugin::activation::LazyStatus;
        let entry = match self.lazy_registry.lookup(plugin_id) {
            Some(e) if e.status == LazyStatus::Lazy => e.clone(),
            Some(e) => {
                return Err(format!(
                    "plugin `{plugin_id}` not in lazy state ({})",
                    e.status.as_str()
                ));
            }
            None => return Err(format!("no lazy entry for `{plugin_id}`")),
        };
        // Drop the stub ledger so its records disappear before the
        // real load creates fresh ones.
        if let Some(ledger) = self.lazy_ledgers.remove(plugin_id) {
            self.cleanup_ledger(&ledger);
        }
        match self.load_plugin(&entry.plugin_dir) {
            Ok(()) => {
                self.lazy_registry.mark_active(
                    plugin_id,
                    now_unix_ms(),
                    trigger_descriptor.clone(),
                );
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Info,
                        "activation.completed",
                        format!("plugin `{plugin_id}` activated by trigger `{trigger_descriptor}`"),
                    )
                    .with_context(serde_json::json!({
                        "trigger_kind": trigger_kind,
                        "trigger": trigger_descriptor,
                    })),
                );
                // Verify the plugin actually registered every command
                // and keymap it declared in `[activation]`. Anything
                // missing emits a warning but does NOT poison the
                // plugin — the user's first invocation already
                // succeeded (the redispatch above) or the command
                // simply was not auto-callable.
                self.verify_activation_promises(plugin_id, &entry);
                Ok(())
            }
            Err(err) => {
                let msg = err.to_string();
                // Re-install lazy stubs *before* recording failure
                // so the registry has a row to mark. The reinsert
                // resets counters; we then carry over the live
                // failure counter via `mark_failure` so the second
                // attempt poisons.
                let activation = git_leviathan_plugin_api::manifest::PluginActivation {
                    events: entry.events.clone(),
                    commands: entry.commands.clone(),
                    keymaps: entry.keymaps.clone(),
                    regions: entry.regions.clone(),
                    files: entry
                        .files
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect(),
                    repository_shape: entry.repository_shape.clone(),
                    manual: entry.manual,
                };
                self.install_lazy_stubs(plugin_id, &entry.plugin_dir, &activation);
                // Carry over the prior consecutive-failure count
                // before bumping; otherwise every failure looks like
                // the first.
                let prior = entry.consecutive_failures;
                if let Some(e) = self
                    .lazy_registry
                    .entries_mut()
                    .iter_mut()
                    .find(|e| e.plugin_id == plugin_id)
                {
                    e.consecutive_failures = prior;
                }
                let poisoned = self.lazy_registry.mark_failure(plugin_id, msg.clone());
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "activation.failed",
                        format!("plugin `{plugin_id}` activation failed: {msg}"),
                    )
                    .with_context(serde_json::json!({
                        "trigger_kind": trigger_kind,
                        "trigger": trigger_descriptor,
                        "error": msg,
                    })),
                );
                if poisoned {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "activation.poisoned",
                            format!(
                                "plugin `{plugin_id}` poisoned after {} consecutive activation failures",
                                crate::plugin::activation::MAX_ACTIVATION_FAILURES
                            ),
                        ),
                    );
                }
                Err(msg)
            }
        }
    }

    fn verify_activation_promises(
        &self,
        plugin_id: &str,
        entry: &crate::plugin::activation::LazyEntry,
    ) {
        let registry = self.command_registry.borrow();
        for cmd in &entry.commands {
            let registered = registry
                .entries()
                .iter()
                .any(|e| e.descriptor.plugin_id == plugin_id && e.descriptor.name == *cmd);
            if !registered {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Warning,
                        "activation.unknown_command",
                        format!(
                            "plugin `{plugin_id}` declared activation command `{cmd}` but did not register it after activation"
                        ),
                    )
                    .with_context(serde_json::json!({ "command": cmd })),
                );
            }
        }
        drop(registry);
        let kreg = self.keymap_registry.borrow();
        for km in &entry.keymaps {
            let registered = kreg
                .entries()
                .iter()
                .any(|e| e.plugin_id == plugin_id && e.context == km.context);
            if !registered {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Warning,
                        "activation.unknown_keymap",
                        format!(
                            "plugin `{plugin_id}` declared activation keymap `{}:{}` but did not register a binding after activation",
                            km.context, km.key
                        ),
                    )
                    .with_context(serde_json::json!({
                        "context": km.context,
                        "key": km.key,
                    })),
                );
            }
        }
    }
}

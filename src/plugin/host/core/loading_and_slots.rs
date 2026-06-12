use super::*;

impl PluginHost {
    pub fn load_plugin(&mut self, dir: &Path) -> Result<(), PluginLoadError> {
        // Respect the disabled-plugins set. If the id (derived
        // from the manifest below) is in `self.disabled_plugins`, we
        // refuse to load. Pre-parse the manifest to know the id; the
        // actual `disabled` check happens after we have a manifest in
        // hand. We surface the refusal as a structured diagnostic +
        // error so callers see the same shape as a manifest-bad load.
        let manifest_path = dir.join("plugin.toml");
        let manifest_str = match fs::read_to_string(&manifest_path) {
            Ok(s) => s,
            Err(e) => {
                let plugin_id = dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("<unknown>")
                    .to_string();
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "manifest.read_failed",
                        format!("could not read plugin.toml: {e}"),
                    )
                    .with_source(PluginSourceSpan::Manifest {
                        path: manifest_path.display().to_string(),
                        key: None,
                    }),
                );
                return Err(PluginLoadError::Io(e));
            }
        };
        let manifest: PluginManifest = match toml::from_str(&manifest_str) {
            Ok(m) => m,
            Err(e) => {
                let plugin_id = dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("<unknown>")
                    .to_string();
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "manifest.parse_failed",
                        format!("invalid plugin.toml: {e}"),
                    )
                    .with_source(PluginSourceSpan::Manifest {
                        path: manifest_path.display().to_string(),
                        key: None,
                    }),
                );
                return Err(PluginLoadError::Toml(e));
            }
        };
        self.diagnostics
            .set_plugin_debug(&manifest.id, manifest.debug);
        // Short-circuit when the plugin is on the
        // disabled-plugins list. The diagnostic carries enough
        // context to debug a "why isn't my plugin loading" question
        // without scraping stderr; the error variant is `Disabled`
        // so callers can surface the reason explicitly.
        if self.disabled_plugins.contains(&manifest.id) {
            let msg = format!("plugin `{}` is disabled", manifest.id);
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Info,
                    "plugin.disabled",
                    msg.clone(),
                )
                .with_source(PluginSourceSpan::Manifest {
                    path: manifest_path.display().to_string(),
                    key: None,
                }),
            );
            // Record the root anyway so a later `enable` knows where
            // to look. This is safe: the path was readable enough to
            // parse a manifest, so it's a real plugin directory.
            self.last_plugin_roots
                .insert(manifest.id.clone(), dir.to_path_buf());
            return Err(PluginLoadError::Disabled(msg));
        }
        if let Some(msg) = git_leviathan_plugin_api::api_version::plugin_api_compatibility_error(
            manifest.api_version,
        ) {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Error,
                    "manifest.incompatible_api_version",
                    msg.clone(),
                )
                .with_source(PluginSourceSpan::Manifest {
                    path: manifest_path.display().to_string(),
                    key: Some("api_version".to_string()),
                }),
            );
            return Err(PluginLoadError::BadManifest(msg));
        }
        let init_path = dir.join("init.lua");
        let init_src = match fs::read_to_string(&init_path) {
            Ok(s) => s,
            Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(manifest.id.clone()),
                        DiagnosticSeverity::Error,
                        "lua.read_failed",
                        format!("could not read init.lua: {e}"),
                    )
                    .with_source(PluginSourceSpan::Lua {
                        file: init_path.display().to_string(),
                        line: None,
                        traceback: None,
                    }),
                );
                return Err(PluginLoadError::Io(e));
            }
        };

        let plugin_root = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let storage_paths = self.storage_paths(&manifest.id, plugin_root.clone());
        let state_dir = storage_paths.state_dir.clone();
        let config_dir = storage_paths.config_dir.clone();
        let workdir: Option<PathBuf> = None;

        let plugin_id = PluginId::from(manifest.id.clone());
        let generation_id = self.allocate_generation_id(&manifest.id);
        let plugin_version = manifest.version.to_string();

        self.validate_service_dependencies(&manifest, generation_id)?;

        // Walk the requested capability list. Anything the
        // descriptor table doesn't know becomes a `capability.unknown`
        // warning. Local development plugins under auto-grant roots
        // get declared capabilities granted at load time; other roots
        // queue undecided capabilities for the security prompt.
        let requested_strings = canonicalize_requested(&manifest.capabilities);
        for unknown in unknown_requested(&requested_strings) {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    plugin_id.clone(),
                    DiagnosticSeverity::Warning,
                    "capability.unknown",
                    format!(
                        "manifest declares unknown capability `{unknown}`; ignored at use time"
                    ),
                )
                .with_generation(generation_id),
            );
        }
        if self.auto_grant_policy.is_auto_granted(&plugin_root) {
            let granted = self.grant_store.auto_grant_declared(
                &manifest.id,
                &plugin_version,
                &requested_strings,
            );
            for row in granted {
                audit_grant_event(
                    &self.audit_log,
                    &row.plugin_id,
                    "grant.allowed",
                    &row.capability,
                    Some(row.decided_by),
                );
            }
        } else if let Some(prompt) = prompt_for_undecided(
            &self.grant_store,
            &manifest.id,
            &plugin_version,
            &requested_strings,
        ) {
            audit_grant_event(
                &self.audit_log,
                &manifest.id,
                "grant.upgrade_prompted",
                &format!("{} new capabilities", prompt.pending().len()),
                None,
            );
            if let Err(e) = self.grant_store.open_prompt(prompt) {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        plugin_id.clone(),
                        DiagnosticSeverity::Warning,
                        "capability.persistence_failed",
                        format!("could not open capability prompt: {e}"),
                    )
                    .with_generation(generation_id),
                );
            }
        }

        let guard = Rc::new(
            CapabilityGuard::new(
                manifest.id.clone(),
                plugin_version.clone(),
                manifest.capabilities.clone(),
                crate::plugin::capabilities::CapabilityPaths {
                    plugin_root: plugin_root.clone(),
                    state_dir,
                    config_dir,
                    workdir,
                },
                self.grant_store.clone(),
            )
            .with_audit(self.audit_log.clone())
            .with_diagnostics(
                self.diagnostics.clone(),
                plugin_id.clone(),
                generation_id,
            ),
        );

        let lua = Rc::new(Lua::new());
        let generation = PluginGeneration::new(plugin_id.clone(), generation_id, Rc::clone(&lua));
        let ledger = generation.ledger.clone();
        let build: Rc<RefCell<BuildState>> = Rc::new(RefCell::new(BuildState::default()));
        let deferred: Rc<RefCell<DeferredQueue>> = Rc::new(RefCell::new(DeferredQueue::default()));
        let user_commands: Rc<RefCell<UserCommands>> =
            Rc::new(RefCell::new(UserCommands::default()));
        let health_checks_sink: Rc<RefCell<Vec<HealthCheckRegistration>>> =
            Rc::new(RefCell::new(Vec::new()));

        let services_ctx = ServicesContext {
            registry: Rc::clone(&self.service_registry),
            lookup_registry: None,
            plugin_id: manifest.id.clone(),
            generation_id,
            provides: manifest.provides_services.clone(),
            consumes: manifest.consumes_services.clone(),
            plugin_lua: Rc::clone(&lua),
            capability_guard: Rc::clone(&guard),
        };
        self.service_registry
            .borrow()
            .set_budget_tracker(self.budget_tracker.clone());

        let persist_ctx = PersistContext {
            storage: storage_paths,
        };

        let job_callbacks: Rc<RefCell<JobCallbacks>> = Rc::new(RefCell::new(JobCallbacks::new()));
        let timer_callbacks: Rc<RefCell<PluginTimerCallbacks>> =
            Rc::new(RefCell::new(PluginTimerCallbacks::new()));
        let watcher_callbacks: Rc<RefCell<PluginWatcherCallbacks>> =
            Rc::new(RefCell::new(PluginWatcherCallbacks::new()));
        let overlay_callbacks: Rc<RefCell<OverlayCallbacks>> =
            Rc::new(RefCell::new(OverlayCallbacks::new()));
        let dialog_callbacks: Rc<RefCell<crate::plugin::ui::dialog::DialogCallbacks>> = Rc::new(
            RefCell::new(crate::plugin::ui::dialog::DialogCallbacks::new()),
        );
        let decoration_provider_callbacks: Rc<RefCell<DecorationProviderCallbacks>> =
            Rc::new(RefCell::new(DecorationProviderCallbacks::new()));
        let chrome_widgets: crate::plugin::api::ui_ext::ChromeWidgetMap =
            Rc::new(RefCell::new(HashMap::new()));
        let ui_context =
            crate::plugin::ui::context::UiContextStore::new(manifest.id.as_str(), generation_id);
        let async_ctx = self.build_async_runtime_context(
            Rc::clone(&job_callbacks),
            Rc::clone(&timer_callbacks),
            Rc::clone(&watcher_callbacks),
        );

        if let Err(e) = api::install_all(
            &lua,
            api::InstallAllContext {
                build: Rc::clone(&build),
                pending_tab_ops: Rc::clone(&self.pending_tab_ops),
                guard: Rc::clone(&guard),
                services_ctx,
                persist_ctx,
                deferred: Rc::clone(&deferred),
                user_commands: Rc::clone(&user_commands),
                health_checks: Rc::clone(&health_checks_sink),
                ledger: ledger.clone(),
                command_dispatch: self.command_dispatch_env(),
                keymaps: Rc::clone(&self.keymap_registry),
                git_ctx: self.git_ops_context(),
                pending_git_events: self.pending_git_events.clone(),
                pending_ui_effects: self.pending_ui_effects.clone(),
                async_ctx,
                plugin_id: plugin_id.clone(),
                generation_id,
                diagnostics: self.diagnostics.clone(),
                extension_registry: self.extension_registry.clone(),
                overlay_callbacks: Rc::clone(&overlay_callbacks),
                dialog_callbacks: Rc::clone(&dialog_callbacks),
                decoration_provider_callbacks: Rc::clone(&decoration_provider_callbacks),
                ui_context: ui_context.clone(),
                chrome_widgets: Rc::clone(&chrome_widgets),
            },
        ) {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Error,
                    "host.install_api_failed",
                    String::new(),
                )
                .with_generation(generation_id)
                .with_mlua_error(&format!("plugins/{}/install_api", manifest.id), &e),
            );
            self.cleanup_ledger(&ledger);
            return Err(e.into());
        }

        // Build the runtime path. The plugin's own root is
        // passed in directly; dependency roots are looked up from the
        // host-wide registry. Registration of *this* plugin's root is
        // deferred until init.lua succeeds, so a failed load doesn't
        // briefly expose a plugin that doesn't actually exist.
        let runtime_path = PluginRuntimePath::resolve(
            &manifest.id,
            &plugin_root,
            &manifest.requires_plugins,
            &self.runtime_path_registry,
        );
        let lua_loader = LuaLoader::new(
            plugin_id.clone(),
            generation_id,
            runtime_path,
            self.diagnostics.clone(),
            manifest.runtime.strict_globals,
        );
        if let Err(e) = lua_loader.install(&lua) {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Error,
                    "host.install_loader_failed",
                    String::new(),
                )
                .with_generation(generation_id)
                .with_mlua_error(&format!("plugins/{}/install_loader", manifest.id), &e),
            );
            self.cleanup_ledger(&ledger);
            return Err(e.into());
        }
        if let Err(e) = lua
            .globals()
            .get::<Table>("leviathan")
            .and_then(|t| install_runtime_module(&lua, &t, lua_loader.clone()))
        {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Error,
                    "host.install_runtime_module_failed",
                    String::new(),
                )
                .with_generation(generation_id)
                .with_mlua_error(
                    &format!("plugins/{}/install_runtime_module", manifest.id),
                    &e,
                ),
            );
            self.cleanup_ledger(&ledger);
            return Err(e.into());
        }

        let chunk_name = format!("plugins/{}/init.lua", manifest.id);
        // Time init.lua against the `Init` budget. We pass
        // a fresh `PluginId` rather than borrowing from the manifest
        // because the closure has to be `'static` over the Lua state.
        let init_pid = PluginId::from(manifest.id.clone());
        let init_outcome = self.budget_tracker.track_call::<(), mlua::Error>(
            CallbackKind::Init,
            &init_pid,
            generation_id,
            "init.lua",
            || lua.load(&init_src).set_name(chunk_name.clone()).exec(),
        );
        if let PerfOutcome::Err(e) = init_outcome {
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Error,
                    "lua.init_failed",
                    String::new(),
                )
                .with_generation(generation_id)
                .with_mlua_error(&chunk_name, &e),
            );
            self.cleanup_ledger(&ledger);
            return Err(PluginLoadError::Plugin(
                git_leviathan_plugin_api::error::PluginError::from_mlua(
                    &manifest.id,
                    "init.lua exec",
                    &e,
                ),
            ));
        }
        // After init.lua succeeds, register the plugin's root and walk
        // after/plugin/*.lua in lexical order. Failures in the
        // after-walk are recorded as diagnostics but don't unload the
        // plugin — after-files are bootstrap helpers, not load gates.
        self.runtime_path_registry
            .register(manifest.id.clone(), plugin_root.clone());
        lua_loader.run_after_plugin(&lua, &plugin_root);

        let (screens, dock_panels, settings_panel, slot_ops, autocmds) = {
            let mut b = build.borrow_mut();
            (
                std::mem::take(&mut b.screens),
                std::mem::take(&mut b.dock_panels),
                b.settings_panel.take(),
                std::mem::take(&mut b.slot_ops),
                std::mem::take(&mut b.autocmds),
            )
        };

        let mut slot_handlers = HashMap::new();
        let mut dynamic_widgets = HashMap::new();
        let mut installed_slot_ops = false;
        for op in slot_ops {
            if !validate_raw_slot_op(
                &self.diagnostics,
                generation_id,
                &self.slot_ops,
                &manifest.id,
                &op,
            ) {
                continue;
            }
            match prepare_op(
                &manifest.id,
                &plugin_root,
                op,
                &ledger,
                &mut slot_handlers,
                &mut dynamic_widgets,
            ) {
                Ok(prepared) => {
                    self.slot_ops.push(prepared);
                    installed_slot_ops = true;
                }
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(manifest.id.clone()),
                            DiagnosticSeverity::Warning,
                            "schema.slot_invalid",
                            format!("slot op ignored: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.ui.slot.add".into(),
                        }),
                    );
                }
            }
        }

        // Resolve plugin-local autocmd-group ids to host-wide
        // GroupIds, applying any `clear = true` requests captured at
        // declaration time. The same translation map lets autocmds
        // reference their group through the host's stable handle.
        //
        // Operations replay in plugin-local declaration sequence so
        // a `clear` issued *after* an `autocmd.create` correctly
        // removes that registration.
        let (raw_groups, raw_clears) = {
            let mut b = build.borrow_mut();
            (
                std::mem::take(&mut b.autocmd_groups),
                std::mem::take(&mut b.autocmd_clears),
            )
        };
        let mut local_to_host: HashMap<u64, GroupId> = HashMap::new();
        let autocmd_ops = MergedAutocmdOps::new(autocmds, raw_groups, raw_clears);
        for op in autocmd_ops {
            match op {
                AutocmdOp::Group(grp) => {
                    let host_id = self
                        .event_bus
                        .group_handle(&manifest.id, &grp.name, grp.clear);
                    local_to_host.insert(grp.local_id, host_id);
                }
                AutocmdOp::Clear(clear) => {
                    if let Some(host_id) = local_to_host.get(&clear.local_id).copied() {
                        self.event_bus.clear_group(&manifest.id, host_id);
                    }
                }
                AutocmdOp::Create(raw) => {
                    self.install_one_autocmd(&manifest.id, generation_id, raw, &local_to_host);
                }
            }
        }

        self.diagnostics.record(
            PluginDiagnostic::new(
                PluginId::from(manifest.id.clone()),
                DiagnosticSeverity::Info,
                "host.plugin_loaded",
                format!("loaded plugin {} ({})", manifest.id, manifest.name),
            )
            .with_generation(generation_id),
        );
        let health_checks: Vec<HealthCheckRegistration> =
            std::mem::take(&mut *health_checks_sink.borrow_mut());

        // Register the plugin's Lua state + capability guard
        // in the dispatcher's plugin map *before* installing any
        // commands so the dispatcher can resolve them. Then install
        // any `RawCommand`s captured during init into the unified
        // registry. Reload uses the same install path inside
        // `commit_staging`.
        self.command_plugin_registry.insert(
            manifest.id.clone(),
            CommandPluginContext {
                lua: Rc::clone(&lua),
                capability_guard: Rc::clone(&guard),
                ui_context: ui_context.clone(),
                generation_id,
            },
        );
        let raw_commands = std::mem::take(&mut build.borrow_mut().commands);
        self.install_raw_commands(&manifest.id, generation_id, raw_commands);

        // keymaps: install captured keymap rows. Apply staged `del`s
        // after the `set`s so a plugin can rebind its own keymaps in
        // one init.lua run.
        let (raw_keymaps, raw_keymap_dels) = {
            let mut b = build.borrow_mut();
            (
                std::mem::take(&mut b.keymaps),
                std::mem::take(&mut b.keymap_dels),
            )
        };
        self.keymap_registry.borrow_mut().install_plugin_raws(
            &manifest.id,
            generation_id,
            raw_keymaps,
            &self.diagnostics,
        );
        self.keymap_registry.borrow_mut().apply_plugin_dels(
            &manifest.id,
            generation_id,
            raw_keymap_dels,
            &self.diagnostics,
        );

        let plugin = LoadedPlugin {
            generation,
            root: plugin_root,
            manifest: manifest.clone(),
            slot_handlers,
            overlay_callbacks,
            dialog_callbacks,
            decoration_provider_callbacks,
            screens,
            dock_panels,
            settings_panel,
            screen_state: HashMap::new(),
            dynamic_widgets,
            chrome_widgets,
            ui_context,
            deferred,
            user_commands,
            health_checks,
            lua_loader,
            job_callbacks,
            timer_callbacks,
            watcher_callbacks,
        };
        self.plugins.insert(manifest.id.clone(), plugin);
        self.register_plugin_dock_panels(&manifest.id);
        self.extension_registry.invalidate_decorations(
            crate::plugin::extensions::DecorationInvalidationReason::PluginState,
        );
        // Remember where this plugin loaded from so a later
        // `Plugin: Enable` (after `Plugin: Disable` unloaded it) can
        // re-load it from the same on-disk path without the user
        // re-supplying the root.
        self.last_plugin_roots
            .insert(manifest.id.clone(), dir.to_path_buf());

        let installed_chrome = !self.plugins[&manifest.id]
            .chrome_widgets
            .borrow()
            .is_empty();
        self.refresh_dynamic_widgets_for_plugin(&manifest.id);
        if installed_slot_ops || installed_chrome {
            self.mark_slot_ops_changed();
        }
        Ok(())
    }

    /// Convert `RawCommand` rows into `CommandDescriptor`s and
    /// register them. Bad descriptors emit `command.invalid_args`
    /// diagnostics and the row is dropped (registration failure
    /// must not block plugin load).
    pub(super) fn install_raw_commands(
        &mut self,
        plugin_id: &str,
        generation_id: GenerationId,
        raws: Vec<RawCommand>,
    ) {
        for raw in raws {
            let name_for_diag = raw.name.clone();
            let source = raw.source_location.clone();
            match commands::build_descriptor(raw, plugin_id.to_string(), Some(generation_id)) {
                Ok(descriptor) => {
                    self.command_registry
                        .borrow_mut()
                        .register(descriptor, &self.diagnostics);
                }
                Err(errors) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Warning,
                            "command.invalid_args",
                            format!(
                                "command `{name_for_diag}` rejected at registration: {}",
                                errors.join("; ")
                            ),
                        )
                        .with_generation(generation_id)
                        .with_source(make_lua_span(plugin_id, source.as_deref()))
                        .with_context(serde_json::json!({
                            "name": name_for_diag,
                            "errors": errors,
                        })),
                    );
                }
            }
        }
    }

    /// Apply every collected `main_bar` op to a `MainBarRegistry`.
    pub fn apply_main_bar_slots(&self, registry: &mut MainBarRegistry) {
        crate::plugin::host::region_mount::REGION_MOUNT_REGISTRY
            .apply_main_bar_slots(self, registry);
    }

    /// Apply every collected `repository` op to a `RepoRegionRegistry`.
    pub fn apply_repo_region_slots(&self, registry: &mut RepoRegionRegistry) {
        crate::plugin::host::region_mount::REGION_MOUNT_REGISTRY
            .apply_repo_region_slots(self, registry);
    }

    /// Apply every collected `tab_bar` op to a `TabBarRegistry`.
    pub fn apply_tab_bar_slots(&self, registry: &mut TabBarRegistry) {
        crate::plugin::host::region_mount::REGION_MOUNT_REGISTRY
            .apply_tab_bar_slots(self, registry);
    }

    pub fn apply_repo_chrome_slots(
        &self,
        registry: &mut crate::widgets::chrome::repo_region::RepoChromeRegistry,
    ) {
        use crate::widgets::chrome::repo_region::RepoChromeSlot;
        let mut plugin_ids: Vec<&String> = self.plugins.keys().collect();
        plugin_ids.sort();
        for plugin_id in plugin_ids {
            let plugin = &self.plugins[plugin_id];
            let chrome_widgets = plugin.chrome_widgets.borrow();
            for chrome in chrome_widgets.values() {
                let cache = Rc::clone(&chrome.cache);
                let plugin_id = plugin.id().to_string();
                let contribution_id = chrome.contribution_id.clone();
                let plugin_root = plugin.root.clone();
                let pane = chrome.pane;
                let priority = chrome.priority;
                let handle = crate::plugin::ui::chrome::ChromeRegistration::handle(
                    pane.point_id(),
                    &contribution_id,
                );
                let slot = RepoChromeSlot {
                    plugin_id: plugin_id.clone(),
                    id: contribution_id.clone(),
                    priority,
                    builder: Box::new(move |_ctx| {
                        let guard = cache.borrow();
                        let ast = guard.as_ref()?;
                        let empty_splits: HashMap<String, Vec<f32>> = HashMap::new();
                        let bc = crate::plugin::bridge::widget_tree::BuildCtx {
                            plugin_id: &plugin_id,
                            scope: crate::plugin::bridge::widget_tree::DispatchScope::Slot {
                                region: "repository",
                                container: pane.as_str(),
                                slot_id: &handle,
                            },
                            plugin_root: plugin_root.as_path(),
                            split_states: &empty_splits,
                            active_drag: None,
                        };
                        Some(crate::plugin::bridge::widget_tree::build(ast, &bc))
                    }),
                };
                registry.insert(pane, slot);
            }
        }
    }

    pub fn chrome_contributions(&self) -> Vec<(String, String, String, i32)> {
        let mut rows = Vec::new();
        for plugin in self.plugins.values() {
            let chrome_widgets = plugin.chrome_widgets.borrow();
            for chrome in chrome_widgets.values() {
                rows.push((
                    plugin.id().to_string(),
                    chrome.pane.point_id().to_string(),
                    chrome.contribution_id.clone(),
                    chrome.priority,
                ));
            }
        }
        rows.sort_by(|a, b| {
            a.3.cmp(&b.3)
                .then(a.0.cmp(&b.0))
                .then(a.1.cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
        rows
    }

    pub fn extension_context_menu_items(
        &self,
        region: &str,
    ) -> Vec<crate::plugin::extensions::ContextMenuItemRecord> {
        self.extension_registry.context_menu_items(region)
    }

    pub fn extension_overlays(&self) -> Vec<crate::plugin::extensions::OverlayRecord> {
        self.extension_registry.overlays()
    }

    pub fn for_each_extension_overlay(
        &self,
        f: impl FnMut(&crate::plugin::extensions::OverlayRecord),
    ) {
        self.extension_registry.for_each_overlay_top_first(f);
    }

    pub fn with_top_extension_overlay<R>(
        &self,
        f: impl FnOnce(&crate::plugin::extensions::OverlayRecord) -> R,
    ) -> Option<R> {
        self.extension_registry.with_top_overlay(f)
    }

    pub fn with_top_dismissible_extension_overlay<R>(
        &self,
        f: impl FnOnce(&crate::plugin::extensions::OverlayRecord) -> R,
    ) -> Option<R> {
        self.extension_registry.with_top_dismissible_overlay(f)
    }

    pub fn extension_graph_decorations_for_commit(
        &self,
        commit_hash: &str,
    ) -> Vec<crate::plugin::extensions::GraphDecorationRecord> {
        let mut rows = self.extension_registry.graph_decorations_for(commit_hash);
        rows.extend(self.dynamic_graph_decorations_for_commit(commit_hash));
        rows.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id).then(a.id.cmp(&b.id)));
        rows
    }

    pub fn extension_diff_decorations_for_line(
        &self,
        file: &str,
        line: u32,
    ) -> Vec<crate::plugin::extensions::DiffDecorationRecord> {
        let mut rows = self
            .extension_registry
            .diff_decorations_for_line(file, line);
        rows.extend(self.dynamic_diff_decorations_for_line(file, line));
        rows.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id).then(a.id.cmp(&b.id)));
        rows
    }

    pub fn discard_extensions_for_plugin(&self, plugin_id: &str) {
        self.extension_registry.clear_for_plugin(plugin_id);
    }

    fn dynamic_graph_decorations_for_commit(
        &self,
        commit_hash: &str,
    ) -> Vec<crate::plugin::extensions::GraphDecorationRecord> {
        let providers = self.extension_registry.decoration_providers(
            crate::plugin::extensions::DecorationProviderTarget::GraphRowBadge,
        );
        let mut rows = Vec::new();
        for provider in providers {
            let Some(plugin) = self.plugins.get(&provider.plugin_id) else {
                continue;
            };
            let Some(func) = self.provider_function(plugin, &provider) else {
                continue;
            };
            let ctx = self.graph_row_context(plugin, commit_hash);
            plugin.ui_context.set(ctx.clone());
            let pid = PluginId::from(provider.plugin_id.clone());
            let cb_id = format!("decoration_provider:{}", provider.handle());
            let outcome = self.budget_tracker.track_call::<LuaValue, mlua::Error>(
                CallbackKind::UiCallback,
                &pid,
                plugin.generation.generation_id,
                &cb_id,
                || {
                    let arg = plugin.lua().to_value(&ctx)?;
                    func.call(arg)
                },
            );
            let Ok(value) = provider_value(outcome) else {
                continue;
            };
            rows.extend(self.decode_graph_provider_rows(plugin, &provider, commit_hash, value));
        }
        rows
    }

    fn dynamic_diff_decorations_for_line(
        &self,
        file: &str,
        line: u32,
    ) -> Vec<crate::plugin::extensions::DiffDecorationRecord> {
        let providers = self.extension_registry.decoration_providers(
            crate::plugin::extensions::DecorationProviderTarget::DiffLineGutter,
        );
        let mut rows = Vec::new();
        for provider in providers {
            let Some(plugin) = self.plugins.get(&provider.plugin_id) else {
                continue;
            };
            let Some(func) = self.provider_function(plugin, &provider) else {
                continue;
            };
            let ctx = self.diff_line_context(plugin, file, line);
            plugin.ui_context.set(ctx.clone());
            let pid = PluginId::from(provider.plugin_id.clone());
            let cb_id = format!("decoration_provider:{}", provider.handle());
            let outcome = self.budget_tracker.track_call::<LuaValue, mlua::Error>(
                CallbackKind::UiCallback,
                &pid,
                plugin.generation.generation_id,
                &cb_id,
                || {
                    let arg = plugin.lua().to_value(&ctx)?;
                    func.call(arg)
                },
            );
            let Ok(value) = provider_value(outcome) else {
                continue;
            };
            rows.extend(self.decode_diff_provider_rows(plugin, &provider, file, line, value));
        }
        rows
    }

    fn provider_function(
        &self,
        plugin: &LoadedPlugin,
        provider: &crate::plugin::extensions::DecorationProviderRecord,
    ) -> Option<Function> {
        let callbacks = plugin.decoration_provider_callbacks.borrow();
        let key = callbacks.get(&provider.handle())?;
        match plugin.lua().registry_value(key) {
            Ok(func) => Some(func),
            Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(provider.plugin_id.clone()),
                        DiagnosticSeverity::Error,
                        "lua.handler_lookup_failed",
                        format!(
                            "decoration provider lookup failed for {}: {e}",
                            provider.handle()
                        ),
                    )
                    .with_generation(plugin.generation.generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("decoration_provider:{}", provider.handle()),
                    }),
                );
                None
            }
        }
    }

    fn graph_row_context(&self, plugin: &LoadedPlugin, commit_hash: &str) -> serde_json::Value {
        let repository = self.repository_context_snapshot();
        let mut ctx = crate::plugin::ui::context::context_for_surface_with_selection(
            plugin.id(),
            plugin.generation.generation_id,
            crate::plugin::ui::context::UiContextSurface::RepositoryGraphRow,
            repository.as_ref(),
            Some(&self.last_selection_snapshot),
            &self.last_tab_snapshot,
            Some(&self.last_focus_snapshot),
        );
        ctx["payload"] = serde_json::json!({
            "graph_row": {
                "commit": crate::plugin::commit_data::CommitData::from_hash(commit_hash),
            },
        });
        ctx
    }

    fn diff_line_context(&self, plugin: &LoadedPlugin, file: &str, line: u32) -> serde_json::Value {
        let repository = self.repository_context_snapshot();
        let mut ctx = crate::plugin::ui::context::context_for_surface_with_selection(
            plugin.id(),
            plugin.generation.generation_id,
            crate::plugin::ui::context::UiContextSurface::RepositoryDiffLine,
            repository.as_ref(),
            Some(&self.last_selection_snapshot),
            &self.last_tab_snapshot,
            Some(&self.last_focus_snapshot),
        );
        ctx["payload"] = serde_json::json!({ "diff_line": { "file": file, "line": line } });
        ctx
    }

    fn decode_graph_provider_rows(
        &self,
        plugin: &LoadedPlugin,
        provider: &crate::plugin::extensions::DecorationProviderRecord,
        commit_hash: &str,
        value: LuaValue,
    ) -> Vec<crate::plugin::extensions::GraphDecorationRecord> {
        let values = match provider_json_values(plugin.lua(), value) {
            Ok(values) => values,
            Err(e) => {
                self.record_provider_decode_error(plugin, provider, e);
                return Vec::new();
            }
        };
        let mut rows = Vec::new();
        for (idx, value) in values.into_iter().enumerate() {
            let id = contribution_id_from_json(&value)
                .unwrap_or_else(|| format!("{}:{}", provider.id, idx + 1));
            match serde_json::from_value::<
                git_leviathan_plugin_api::descriptor::decoration::GraphDecoration,
            >(value)
            {
                Ok(decoration) if decoration.kind() == "badge" => {
                    rows.push(crate::plugin::extensions::GraphDecorationRecord {
                        plugin_id: provider.plugin_id.clone(),
                        id,
                        commit_hash: commit_hash.to_string(),
                        decoration,
                        source_location: provider.source_location.clone(),
                    });
                }
                Ok(_) => {}
                Err(e) => self.record_provider_decode_error(plugin, provider, e.to_string()),
            }
        }
        rows
    }

    fn decode_diff_provider_rows(
        &self,
        plugin: &LoadedPlugin,
        provider: &crate::plugin::extensions::DecorationProviderRecord,
        file: &str,
        line: u32,
        value: LuaValue,
    ) -> Vec<crate::plugin::extensions::DiffDecorationRecord> {
        let values = match provider_json_values(plugin.lua(), value) {
            Ok(values) => values,
            Err(e) => {
                self.record_provider_decode_error(plugin, provider, e);
                return Vec::new();
            }
        };
        let mut rows = Vec::new();
        for (idx, mut value) in values.into_iter().enumerate() {
            inject_line_target(&mut value, file, line);
            let id = contribution_id_from_json(&value)
                .unwrap_or_else(|| format!("{}:{}", provider.id, idx + 1));
            match serde_json::from_value::<
                git_leviathan_plugin_api::descriptor::decoration::DiffDecoration,
            >(value)
            {
                Ok(decoration) if decoration.kind() == "line_gutter" => {
                    rows.push(crate::plugin::extensions::DiffDecorationRecord {
                        plugin_id: provider.plugin_id.clone(),
                        id,
                        decoration,
                        source_location: provider.source_location.clone(),
                    });
                }
                Ok(_) => {}
                Err(e) => self.record_provider_decode_error(plugin, provider, e.to_string()),
            }
        }
        rows
    }

    fn record_provider_decode_error(
        &self,
        plugin: &LoadedPlugin,
        provider: &crate::plugin::extensions::DecorationProviderRecord,
        error: String,
    ) {
        self.diagnostics.record(
            PluginDiagnostic::new(
                PluginId::from(provider.plugin_id.clone()),
                DiagnosticSeverity::Error,
                "decoration.provider_invalid",
                error,
            )
            .with_generation(plugin.generation.generation_id)
            .with_source(PluginSourceSpan::ApiFunction {
                name: format!("decoration_provider:{}", provider.handle()),
            }),
        );
    }

    /// Invoke a plugin's slot-click handler. Handler is called with
    /// `(slot_id, event, value)` so a single slot can receive multiple
    /// distinct widget events (e.g. a `tablist` fires `select` / `close`
    /// / `reorder`). Silently no-ops if the plugin is gone, the slot has
    /// no handler, or the Lua call errors.
    pub fn dispatch_slot_click(
        &mut self,
        plugin_id: &str,
        region: &str,
        container: &str,
        slot_id: &str,
        event: &str,
        value: serde_json::Value,
    ) {
        let handler_key = crate::plugin::slots::SlotAddress::new(
            plugin_id,
            region.to_string(),
            crate::plugin::ui::main_bar_slots::parse_container(container),
            slot_id.to_string(),
        );
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        let mut disable_after_failures = false;
        let effects: Vec<crate::plugin::navigation::PluginNavigationEffect> = {
            let Some(plugin) = self.plugins.get(plugin_id) else {
                return;
            };
            let Some(key) = plugin.slot_handlers.get(&handler_key) else {
                return;
            };
            let generation_id = plugin.generation.generation_id;
            let func: Function = match plugin.lua().registry_value(key) {
                Ok(f) => f,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("slot handler lookup failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("slot:{handler_key}"),
                        }),
                    );
                    return;
                }
            };
            let value_lua = match plugin.lua().to_value(&value) {
                Ok(v) => v,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.value_conversion_failed",
                            format!("slot value conv failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("slot:{handler_key}"),
                        }),
                    );
                    return;
                }
            };
            let cb_id = format!("slot:{handler_key}");
            match self
                .budget_tracker
                .track_call::<Option<Table>, mlua::Error>(
                    CallbackKind::UiCallback,
                    &PluginId::from(plugin_id),
                    generation_id,
                    &cb_id,
                    || func.call((slot_id.to_string(), event.to_string(), value_lua)),
                ) {
                PerfOutcome::Ok(Some(t)) => super::ui_lifecycle::parse_navigation_effects(
                    plugin_id,
                    slot_id,
                    generation_id,
                    "slot.on_event",
                    &t,
                    &self.diagnostics,
                ),
                PerfOutcome::Ok(None) | PerfOutcome::Skipped => Vec::new(),
                PerfOutcome::Err(e) => {
                    disable_after_failures = self.budget_tracker.is_tripped(
                        &PluginId::from(plugin_id),
                        generation_id,
                        &cb_id,
                    );
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            format!("slot handler error in {handler_key}"),
                        )
                        .with_generation(generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                    Vec::new()
                }
            }
        };
        if disable_after_failures {
            self.disable_plugin(plugin_id);
            return;
        }
        for effect in &effects {
            match effect {
                crate::plugin::navigation::PluginNavigationEffect::NavigateRepository => {
                    self.active_screen = None;
                    self.widget_tree = None;
                }
                crate::plugin::navigation::PluginNavigationEffect::OpenScreen {
                    plugin_id,
                    screen_id,
                } => self.open_screen(plugin_id.clone(), screen_id.clone()),
            }
        }
        self.pending_navigation_effects.extend(effects);
        let only = HashSet::from([plugin_id.to_string()]);
        self.invalidate_dynamic_widgets(&[UiInvalidationCause::PluginStateChanged], Some(&only));
        self.drain_lua_command_effects();
    }
}

fn provider_value(value: PerfOutcome<LuaValue, mlua::Error>) -> Result<LuaValue, ()> {
    match value {
        PerfOutcome::Ok(value) => Ok(value),
        PerfOutcome::Skipped | PerfOutcome::Err(_) => Err(()),
    }
}

fn provider_json_values(lua: &Lua, value: LuaValue) -> Result<Vec<serde_json::Value>, String> {
    match value {
        LuaValue::Nil | LuaValue::Boolean(false) => Ok(Vec::new()),
        other => {
            let value: serde_json::Value = lua
                .from_value(other)
                .map_err(|e| format!("provider returned non-serialisable value: {e}"))?;
            Ok(match value {
                serde_json::Value::Null | serde_json::Value::Bool(false) => Vec::new(),
                serde_json::Value::Array(values) => values,
                serde_json::Value::Object(mut object) => {
                    if let Some(serde_json::Value::Array(values)) = object.remove("decorations") {
                        values
                    } else {
                        vec![serde_json::Value::Object(object)]
                    }
                }
                other => return Err(format!("provider returned unsupported value: {other}")),
            })
        }
    }
}

fn contribution_id_from_json(value: &serde_json::Value) -> Option<String> {
    value
        .get("id")
        .and_then(|id| id.as_str())
        .map(ToString::to_string)
}

fn inject_line_target(value: &mut serde_json::Value, file: &str, line: u32) {
    let serde_json::Value::Object(map) = value else {
        return;
    };
    map.entry("file")
        .or_insert_with(|| serde_json::Value::String(file.to_string()));
    map.entry("line")
        .or_insert_with(|| serde_json::Value::Number(line.into()));
}

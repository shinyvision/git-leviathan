use super::*;

impl PluginHost {
    pub fn current_ui_context_snapshot(&self) -> serde_json::Value {
        crate::plugin::ui::context::context_for_surface_with_selection_and_dialog(
            crate::plugin::ui::context::SurfaceContextRequest {
                plugin_id: crate::plugin::commands::HOST_COMMAND_PLUGIN_ID,
                generation_id: GenerationId::new(0),
                surface: crate::plugin::ui::context::UiContextSurface::Screen,
                repository: self.repository_context_snapshot().as_ref(),
                selection: Some(&self.last_selection_snapshot),
                dialog: Some(&self.last_toolbar_dialog_snapshot),
                tabs: &self.last_tab_snapshot,
                focus: Some(&self.last_focus_snapshot),
            },
        )
    }

    pub(super) fn refresh_command_active_context(&self) {
        *self.command_active_context.borrow_mut() = self.current_ui_context_snapshot();
    }

    pub(super) fn repository_context_snapshot(
        &self,
    ) -> Option<crate::plugin::ui::context::RepositoryContextSnapshot> {
        self.last_repository_shape.as_ref().map(|facts| {
            crate::plugin::ui::context::RepositoryContextSnapshot {
                name: facts.repo_name.clone(),
                workdir_path: facts.workdir.to_string_lossy().into_owned(),
                current_branch_name: facts.current_branch.clone(),
                head_hash: facts.head_hash.clone(),
                default_remote_name: facts.default_remote.clone(),
                has_remote: facts.has_remote,
            }
        })
    }

    fn custom_settings_view(
        &self,
        plugin: &LoadedPlugin,
        schema: &serde_json::Value,
        values: &serde_json::Value,
    ) -> Option<crate::plugin::ui::widget_ast::WidgetAst> {
        let panel = plugin.settings_panel.as_ref()?;
        let view = plugin.lua().registry_value::<Function>(&panel.view).ok()?;
        let ctx = serde_json::json!({
            "plugin_id": plugin.id(),
            "schema": schema,
            "values": values,
        });
        let arg = plugin.lua().to_value(&ctx).ok()?;
        let value: LuaValue = view.call(arg).ok()?;
        let json: serde_json::Value = plugin.lua().from_value(value).ok()?;
        crate::plugin::ui::widget_ast::decode(&json).ok()
    }

    fn collect_dock_panel_summaries(&self) -> Vec<crate::plugin::devtools::DockPanelSummary> {
        use crate::plugin::devtools::DockPanelSummary;
        self.dock_panels()
            .into_iter()
            .map(|panel| DockPanelSummary {
                plugin_id: panel.plugin_id,
                generation_id: panel.generation_id,
                id: panel.id,
                key: panel.key,
                title: panel.title,
                area: panel.area,
                open: panel.open,
                order: panel.order,
                selected: panel.selected,
                state: panel.state,
                source_location: panel.source_location,
            })
            .collect()
    }

    fn collect_slot_summaries(&self) -> Vec<crate::plugin::devtools::SlotSummary> {
        use crate::plugin::devtools::SlotSummary;
        let mut slot_map: std::collections::BTreeMap<
            crate::plugin::slots::SlotAddress,
            SlotSummary,
        > = std::collections::BTreeMap::new();
        for builtin in crate::plugin::slots::builtins::BUILTIN_SLOTS {
            let address = builtin.address();
            slot_map.insert(
                address.clone(),
                SlotSummary {
                    region: builtin.region.to_string(),
                    container: builtin.container.to_string(),
                    id: builtin.id.to_string(),
                    priority: builtin.priority,
                    hidden: self.contribution_overrides.is_hidden(&address),
                    effective_priority: self
                        .contribution_overrides
                        .priority(&address)
                        .unwrap_or(builtin.priority),
                    owner_plugin_id: crate::plugin::slots::BUILTIN_PLUGIN_ID.to_string(),
                    replace_capability: Some(builtin.replace_capability.to_string()),
                    remove_capability: Some(builtin.remove_capability.to_string()),
                },
            );
        }
        for op in &self.slot_ops {
            match op {
                PreparedSlotOp::Add(p) => {
                    let builtin = crate::plugin::slots::builtins::get(
                        p.region.as_str(),
                        p.container.key().as_str(),
                        p.id.as_str(),
                    );
                    slot_map.insert(
                        p.mount_address.clone(),
                        SlotSummary {
                            region: p.region.clone(),
                            container: p.container.key(),
                            id: p.id.clone(),
                            priority: p.priority,
                            hidden: self.contribution_overrides.is_hidden(&p.mount_address),
                            effective_priority: self
                                .contribution_overrides
                                .priority(&p.mount_address)
                                .unwrap_or(p.priority),
                            owner_plugin_id: p.plugin_id.clone(),
                            replace_capability: builtin.map(|b| b.replace_capability.to_string()),
                            remove_capability: builtin.map(|b| b.remove_capability.to_string()),
                        },
                    );
                }
                PreparedSlotOp::Replace { target, spec, .. } => {
                    let builtin = crate::plugin::slots::builtins::get(
                        target.region().as_str(),
                        target.container().key().as_str(),
                        target.slot_id().as_str(),
                    );
                    slot_map.insert(
                        target.clone(),
                        SlotSummary {
                            region: target.region().as_str().to_string(),
                            container: target.container().key(),
                            id: target.slot_id().as_str().to_string(),
                            priority: spec.priority,
                            hidden: self.contribution_overrides.is_hidden(target),
                            effective_priority: self
                                .contribution_overrides
                                .priority(target)
                                .unwrap_or(spec.priority),
                            owner_plugin_id: spec.plugin_id.clone(),
                            replace_capability: builtin.map(|b| b.replace_capability.to_string()),
                            remove_capability: builtin.map(|b| b.remove_capability.to_string()),
                        },
                    );
                }
                PreparedSlotOp::Remove { target, .. } => {
                    slot_map.remove(target);
                }
            }
        }
        let mut slots: Vec<SlotSummary> = slot_map.into_values().collect();
        slots.sort_by(|a, b| {
            a.region
                .cmp(&b.region)
                .then(a.container.cmp(&b.container))
                .then(a.id.cmp(&b.id))
                .then(a.owner_plugin_id.cmp(&b.owner_plugin_id))
        });
        slots
    }

    /// Point-in-time devtools snapshot: loaded plugins, currently-owned
    /// slots, registered services, and the tail of the capability audit
    /// log. Cheap to call (clones strings; ~O(plugins + slot_ops +
    /// services + audit)). Consumed by the in-app inspector and tests.
    pub fn introspect(&self) -> crate::plugin::devtools::InspectorSnapshot {
        use crate::plugin::devtools::{
            AutocmdSummary, CommandSummaryRow, ContributionOverrideSummary,
            DecorationInvalidationSummary, DependencySummaryRow, DiagnosticSummary,
            ExtensionContributionSummary, InspectorSnapshot, KeymapConflictRef, KeymapSummaryRow,
            LoadedModuleSummary, PluginSummary, ResourceSummary, RuntimePathSummary, SecretSummary,
            ServiceCallTraceSummary, ServiceGraphEdge, ServiceSummary, SettingsSummary,
            StorageSurfaceSummary, UiRenderDiagnosticSummary,
        };
        let dock_layout = self.dock_layout_json();
        let mut snap = InspectorSnapshot {
            extension_points: collect_extension_points(),
            dock_panels: self.collect_dock_panel_summaries(),
            dock_layout,
            ..Default::default()
        };

        for (id, plugin) in &self.plugins {
            let m = &plugin.manifest;
            snap.plugins.push(PluginSummary {
                id: id.clone(),
                name: m.name.clone(),
                version: m.version.to_string(),
                api_version: format!("{}.{}", m.api_version.major, m.api_version.minor),
                debug: m.debug,
                last_reload_error: self.last_reload_errors.get(id).cloned(),
                provides_services: m
                    .provides_services
                    .iter()
                    .map(|d| format!("{}@{}", d.name, d.version))
                    .collect(),
                consumes_services: m
                    .consumes_services
                    .iter()
                    .map(|d| format!("{}@{}", d.name, d.version))
                    .collect(),
                capabilities: m
                    .capabilities
                    .iter()
                    .map(|c| String::from(c.clone()))
                    .collect(),
            });
            for resource in plugin.generation.ledger.records() {
                debug_assert_eq!(resource.plugin_id, plugin.generation.plugin_id);
                debug_assert_eq!(resource.generation_id, plugin.generation.generation_id);
                let created_at_unix_ms = resource.created_at_unix_ms();
                snap.resources.push(ResourceSummary {
                    resource_id: resource.resource_id.get(),
                    plugin_id: resource.plugin_id.to_string(),
                    generation_id: resource.generation_id.get(),
                    kind: resource.kind.as_str().to_string(),
                    handle: resource.handle,
                    source_location: resource.source_location,
                    created_at_unix_ms,
                });
            }
            for entry in plugin.lua_loader.runtime_path().entries() {
                snap.runtime_paths.push(RuntimePathSummary {
                    plugin_id: id.clone(),
                    generation_id: plugin.generation.generation_id.get(),
                    entry_plugin_id: entry.plugin_id.clone(),
                    kind: entry.kind.as_str().to_string(),
                    root: entry.lua_root.display().to_string(),
                });
            }
            for record in plugin.lua_loader.module_records() {
                snap.loaded_modules.push(LoadedModuleSummary {
                    plugin_id: id.clone(),
                    generation_id: plugin.generation.generation_id.get(),
                    module_name: record.module_name.clone(),
                    source_plugin_id: record.plugin_id.clone(),
                    source_path: record.source_path.display().to_string(),
                    kind: record.kind.as_str().to_string(),
                });
            }
            let storage = self.storage_paths(id, plugin.root.clone());
            for surface in StorageSurface::devtools_surfaces() {
                let path = match surface {
                    StorageSurface::Settings => storage.settings_path(),
                    _ => storage.surface_dir(surface),
                };
                let meta = crate::plugin::storage::surface_metadata(id, surface, &path);
                snap.storage.push(StorageSurfaceSummary {
                    plugin_id: meta.plugin_id,
                    surface: meta.surface,
                    path: meta.path,
                    exists: meta.exists,
                    file_count: meta.file_count,
                    byte_count: meta.byte_count,
                    corrupt_files: meta.corrupt_files,
                });
            }
            let settings_meta = crate::plugin::settings::metadata(&storage.settings_path());
            let generated_form = crate::plugin::settings::generated_form_widget(
                &settings_meta.schema,
                &settings_meta.values,
            );
            let custom_view =
                self.custom_settings_view(plugin, &settings_meta.schema, &settings_meta.values);
            let custom_view_source = plugin
                .settings_panel
                .as_ref()
                .and_then(|panel| panel.source_location.clone());
            snap.settings.push(SettingsSummary {
                plugin_id: id.clone(),
                path: settings_meta.path,
                generated_form,
                custom_view,
                custom_view_source,
                schema_keys: settings_meta.schema_keys,
                value_keys: settings_meta.value_keys,
                valid: settings_meta.valid,
                errors: settings_meta.errors,
            });
            let secret_meta = crate::plugin::secrets::metadata(&storage.secrets_dir);
            snap.secrets.push(SecretSummary {
                plugin_id: id.clone(),
                path: secret_meta.path,
                key_count: secret_meta.key_count,
                keys: secret_meta.keys,
            });
            for (address, dynamic) in &plugin.dynamic_widgets {
                let telemetry = dynamic.telemetry.borrow().clone();
                snap.ui_render_diagnostics.push(UiRenderDiagnosticSummary {
                    plugin_id: id.clone(),
                    generation_id: plugin.generation.generation_id.get(),
                    region: address.region().as_str().to_string(),
                    container: address.container().key(),
                    slot_id: address.slot_id().as_str().to_string(),
                    dependencies: dynamic
                        .dependencies
                        .iter()
                        .map(|dep| dep.as_str().to_string())
                        .collect(),
                    last_refresh_causes: telemetry.last_causes,
                    refresh_count: telemetry.refresh_count,
                    skipped_count: telemetry.skipped_count,
                    stale_count: telemetry.stale_count,
                    last_duration_ms: telemetry.last_duration_ms,
                    last_status: telemetry.last_status,
                    last_error: telemetry.last_error,
                    diagnostic_badge: telemetry.diagnostic_badge,
                });
            }
        }
        snap.runtime_paths
            .sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        snap.loaded_modules.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.module_name.cmp(&b.module_name))
        });
        snap.plugins.sort_by(|a, b| a.id.cmp(&b.id));
        snap.resources.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.generation_id.cmp(&b.generation_id))
                .then(a.resource_id.cmp(&b.resource_id))
        });
        snap.storage.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.surface.cmp(&b.surface))
        });
        snap.settings.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        snap.secrets.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        snap.ui_render_diagnostics.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.region.cmp(&b.region))
                .then(a.container.cmp(&b.container))
                .then(a.slot_id.cmp(&b.slot_id))
        });

        snap.slots = self.collect_slot_summaries();
        snap.contribution_overrides = self
            .contribution_overrides
            .summaries()
            .into_iter()
            .map(|row| ContributionOverrideSummary {
                plugin_id: row.plugin_id,
                region: row.region,
                container: row.container,
                id: row.id,
                hidden: row.hidden,
                priority: row.priority,
            })
            .collect();

        {
            let registry = self.service_registry.borrow();
            for h in registry.handles_iter() {
                let mut methods: Vec<String> = h.methods.keys().cloned().collect();
                methods.sort();
                snap.services.push(ServiceSummary {
                    key: ServiceRegistry::key(&h.decl),
                    publisher_plugin_id: h.plugin_id.clone(),
                    methods,
                });
            }
            for (id, plugin) in &self.plugins {
                for status in dependency_statuses(
                    id,
                    &plugin.manifest.provides_services,
                    &plugin.manifest.consumes_services,
                    &registry,
                ) {
                    let edge_status = match (status.required, status.satisfied) {
                        (_, true) => "connected",
                        (true, false) => "missing_required",
                        (false, false) => "missing_optional",
                    };
                    snap.service_graph.push(ServiceGraphEdge {
                        consumer_plugin_id: status.consumer_plugin_id,
                        provider_plugin_id: status.provider_plugin_id,
                        service_key: status.service_key,
                        required: status.required,
                        status: edge_status.to_string(),
                    });
                }
            }
            for trace in registry.traces() {
                snap.service_call_traces.push(ServiceCallTraceSummary {
                    caller_plugin_id: trace.caller_plugin_id,
                    provider_plugin_id: trace.provider_plugin_id,
                    service_key: trace.service_key,
                    method: trace.method,
                    success: trace.success,
                    error: trace.error,
                    duration_ms: trace.duration_ms,
                    timestamp_unix_ms: trace.timestamp_unix_ms,
                });
            }
        }
        snap.services.sort_by(|a, b| a.key.cmp(&b.key));
        snap.service_graph.sort_by(|a, b| {
            a.consumer_plugin_id
                .cmp(&b.consumer_plugin_id)
                .then(a.service_key.cmp(&b.service_key))
        });

        // dependency graph projection. The host stores the
        // resolver's last graph as live state so devtools mirror what
        // resolution actually produced (including blocked plugins).
        snap.dependency_graph = self
            .dependency_graph
            .iter()
            .map(|d| DependencySummaryRow {
                consumer_plugin_id: d.consumer_plugin_id.clone(),
                dependency_id: d.dependency_id.clone(),
                requirement: d.requirement.clone(),
                resolved_version: d.resolved_version.clone(),
                kind: d.kind.to_string(),
                status: d.status.to_string(),
            })
            .collect();
        snap.dependency_graph.sort_by(|a, b| {
            a.consumer_plugin_id
                .cmp(&b.consumer_plugin_id)
                .then(a.dependency_id.cmp(&b.dependency_id))
        });

        let entries = self.audit_log.entries();
        let n = entries.len();
        let start = n.saturating_sub(100);
        snap.audit_recent = entries[start..].to_vec();

        snap.diagnostics = self
            .diagnostics
            .tail(100)
            .into_iter()
            .map(|d| DiagnosticSummary {
                plugin_id: d.plugin_id.to_string(),
                generation_id: d.generation_id.map(|g| g.get()),
                severity: d.severity.to_string(),
                code: d.code.clone(),
                message: d.message.clone(),
                source: d.source_string(),
                context: d.context.clone(),
                timestamp_unix_ms: d.timestamp_unix_ms(),
            })
            .collect();

        // autocmd rows. Project every entry into a stable
        // summary; sort by (plugin_id, generation_id, autocmd_id).
        for entry in self.event_bus.entries() {
            snap.autocmds.push(AutocmdSummary {
                id: entry.id.get(),
                plugin_id: entry.plugin_id.clone(),
                generation_id: entry.generation_id.get(),
                group_id: entry.group.map(|g| g.get()),
                event: entry.canonical_event.to_string(),
                subscribed_event: entry.event.to_string(),
                pattern: entry.options.pattern.clone(),
                debounce_ms: entry.options.debounce_ms,
                priority: entry.options.priority,
                once: entry.options.once,
                source_location: entry.options.source_location.clone(),
                fires: entry.runtime.fires,
                failures: entry.runtime.failures,
                consecutive_failures: entry.runtime.consecutive_failures,
                disabled: entry.runtime.disabled,
            });
        }
        snap.autocmds.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.generation_id.cmp(&b.generation_id))
                .then(a.id.cmp(&b.id))
        });

        // command rows: project the unified registry into
        // sorted devtools rows. Host commands sit under `<host>`.
        let registry = self.command_registry.borrow();
        for entry in registry.entries() {
            let desc = &entry.descriptor;
            snap.commands.push(CommandSummaryRow {
                name: desc.name.clone(),
                title: desc.title.clone(),
                description: desc.description.clone(),
                plugin_id: desc.plugin_id.clone(),
                generation_id: desc.generation_id.map(|g| g.get()),
                context: desc.context.clone(),
                destructive: desc.destructive,
                capabilities: desc.capabilities.clone(),
                plugin_invocation_capabilities: desc.plugin_invocation_capabilities.clone(),
                enabled: desc.availability.enabled,
                disabled_reason: desc.availability.reason.clone(),
                keymap_eligible: desc.keymap_eligible,
                palette_visible: desc.palette_visible,
                hook_before: desc.hooks.before,
                hook_after: desc.hooks.after,
                hook_veto: desc.hooks.veto,
                replaceable: desc.hooks.replace,
                fires: entry.runtime.fires,
                failures: entry.runtime.failures,
                last_outcome: entry.runtime.last_outcome.clone(),
                last_duration_ms: entry.runtime.last_duration_ms,
            });
        }
        drop(registry);
        snap.commands
            .sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id).then(a.name.cmp(&b.name)));

        // keymap rows: project the registry's already-sorted
        // summaries straight through.
        let keymap_summaries = self.keymap_registry.borrow().summaries();
        for summary in keymap_summaries {
            snap.keymaps.push(KeymapSummaryRow {
                context: summary.context,
                key: summary.key,
                command: summary.command,
                plugin_id: summary.plugin_id,
                generation_id: summary.generation_id,
                source: summary.source,
                status: summary.status,
                description: summary.description,
                conflict_with: summary.conflict_with.map(|c| KeymapConflictRef {
                    plugin_id: c.plugin_id,
                    source: c.source,
                }),
            });
        }

        // capability grant snapshot: cheap-clone every row + every
        // open prompt. Inspectors / tests use these to drive the
        // grant lifecycle without poking the store directly.
        snap.capability_grants = self
            .grant_store
            .rows()
            .into_iter()
            .map(CapabilityGrantSummary::from)
            .collect();
        let mut prompts: Vec<PendingPromptSummary> = self
            .grant_store
            .pending_prompts()
            .iter()
            .filter_map(PendingPromptSummary::from_prompt)
            .collect();
        prompts.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.plugin_version.cmp(&b.plugin_version))
        });
        snap.pending_capability_prompts = prompts;

        // Surface every recent / in-flight git write.
        snap.pending_git_writes = self.pending_git_writes.entries();

        // async runtime: project async-runtime registries into the snapshot.
        snap.async_jobs = self
            .async_jobs
            .summaries()
            .into_iter()
            .map(|s| crate::plugin::devtools::AsyncJobSummary {
                plugin_id: s.plugin_id,
                generation_id: s.generation_id,
                job_id: s.job_id,
                started_at_unix_ms: s.started_at_unix_ms,
                status: s.status,
            })
            .collect();
        snap.timers = self
            .timers
            .summaries()
            .into_iter()
            .map(|s| crate::plugin::devtools::TimerSummary {
                plugin_id: s.plugin_id,
                generation_id: s.generation_id,
                timer_id: s.timer_id,
                kind: s.kind,
                interval_ms: s.interval_ms,
                fires: s.fires,
            })
            .collect();
        snap.file_watchers = self
            .watchers
            .summaries()
            .into_iter()
            .map(|s| crate::plugin::devtools::WatcherSummary {
                plugin_id: s.plugin_id,
                generation_id: s.generation_id,
                watch_id: s.watch_id,
                path: s.path,
                recursive: s.recursive,
            })
            .collect();

        // lazy-plugin projection. Builds the inspector row
        // directly from the registry (sorted by plugin_id for
        // stable rendering) and renders trigger descriptors inline
        // — the registry does not own a projection helper, since
        // this is the single consumer.
        let mut lazy_rows: Vec<crate::plugin::devtools::LazyPluginSummary> = self
            .lazy_registry
            .entries()
            .iter()
            .map(|e| {
                let mut triggers: Vec<String> = Vec::new();
                for c in &e.commands {
                    triggers.push(format!("command:{c}"));
                }
                for k in &e.keymaps {
                    triggers.push(format!("keymap:{}:{}", k.context, k.key));
                }
                for ev in &e.events {
                    triggers.push(format!("event:{ev}"));
                }
                for r in &e.regions {
                    triggers.push(format!("region:{r}"));
                }
                for f in &e.files {
                    triggers.push(format!("file:{}", f.display()));
                }
                if e.repository_shape.is_some() {
                    triggers.push("repository_shape".to_string());
                }
                if e.manual {
                    triggers.push("manual".to_string());
                }
                triggers.sort();
                crate::plugin::devtools::LazyPluginSummary {
                    plugin_id: e.plugin_id.clone(),
                    triggers,
                    status: e.status.as_str().to_string(),
                    activations: e.activations,
                    last_activation_unix_ms: e.last_activation_unix_ms,
                    last_activation_trigger: e.last_activation_trigger.clone(),
                    last_error: e.last_error.clone(),
                }
            })
            .collect();
        lazy_rows.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        snap.lazy_plugins = lazy_rows;

        // extension-point projections. The registry already
        // sorts each surface for us (see `ExtensionRegistry`), so the
        // snapshots flow through verbatim.
        snap.overlays = self
            .extension_registry
            .overlays()
            .into_iter()
            .map(|o| crate::plugin::devtools::OverlaySummary {
                plugin_id: o.plugin_id,
                id: o.id,
                priority: o.priority,
                dismissible: o.dismissible,
                key_events: o.key_events,
                widget: o.widget,
                source_location: o.source_location,
            })
            .collect();
        snap.context_menu_items = self
            .extension_registry
            .all_context_menu_items()
            .into_iter()
            .map(|i| crate::plugin::devtools::ContextMenuItemSummary {
                plugin_id: i.plugin_id,
                region: i.region,
                id: i.id,
                label: i.label,
                command: i.command,
                priority: i.priority,
                condition_capability: i.condition_capability,
                source_location: i.source_location,
            })
            .collect();
        snap.graph_decorations = self
            .extension_registry
            .all_graph_decorations()
            .into_iter()
            .map(|d| {
                let kind = d.decoration.kind().to_string();
                let decoration =
                    serde_json::to_value(&d.decoration).unwrap_or(serde_json::Value::Null);
                crate::plugin::devtools::GraphDecorationSummary {
                    plugin_id: d.plugin_id,
                    id: d.id,
                    commit_hash: d.commit_hash,
                    kind,
                    decoration,
                    source_location: d.source_location,
                }
            })
            .collect();
        snap.diff_decorations = self
            .extension_registry
            .all_diff_decorations()
            .into_iter()
            .map(|d| {
                let kind = d.decoration.kind().to_string();
                let decoration =
                    serde_json::to_value(&d.decoration).unwrap_or(serde_json::Value::Null);
                crate::plugin::devtools::DiffDecorationSummary {
                    plugin_id: d.plugin_id,
                    id: d.id,
                    kind,
                    decoration,
                    source_location: d.source_location,
                }
            })
            .collect();
        snap.decoration_revision = self.extension_registry.decoration_revision();
        snap.decoration_invalidations = self
            .extension_registry
            .decoration_invalidations()
            .into_iter()
            .map(|row| DecorationInvalidationSummary {
                revision: row.revision,
                reason: row.reason,
            })
            .collect();
        snap.extension_contributions = self
            .extension_registry
            .all_contributions()
            .into_iter()
            .map(|row| ExtensionContributionSummary {
                plugin_id: row.plugin_id,
                point_id: row.point_id,
                kind: row.kind,
                id: row.id,
                resource_kind: row.resource_kind,
                owner_key: row.owner_key,
                dynamic: row.dynamic,
                source_location: row.source_location,
            })
            .collect();
        for slot in &snap.slots {
            snap.extension_contributions
                .push(ExtensionContributionSummary {
                    plugin_id: slot.owner_plugin_id.clone(),
                    point_id: format!("{}.{}", slot.region, slot.container),
                    kind: "Slot".into(),
                    id: slot.id.clone(),
                    resource_kind: "slot".into(),
                    owner_key: format!(
                        "{}:{}:{}:{}",
                        slot.owner_plugin_id, slot.region, slot.container, slot.id
                    ),
                    dynamic: false,
                    source_location: None,
                });
        }
        for command in &snap.commands {
            snap.extension_contributions
                .push(ExtensionContributionSummary {
                    plugin_id: command.plugin_id.clone(),
                    point_id: "commands".into(),
                    kind: "Command".into(),
                    id: command.name.clone(),
                    resource_kind: "command".into(),
                    owner_key: format!("{}:{}", command.plugin_id, command.name),
                    dynamic: false,
                    source_location: None,
                });
        }
        for (plugin_id, plugin) in &self.plugins {
            for screen_id in plugin.screens.keys() {
                snap.extension_contributions
                    .push(ExtensionContributionSummary {
                        plugin_id: plugin_id.clone(),
                        point_id: "screens".into(),
                        kind: "Screen".into(),
                        id: screen_id.clone(),
                        resource_kind: "screen".into(),
                        owner_key: format!("{plugin_id}:{screen_id}"),
                        dynamic: true,
                        source_location: None,
                    });
            }
        }
        for settings in &snap.settings {
            if !settings.schema_keys.is_empty() {
                snap.extension_contributions
                    .push(ExtensionContributionSummary {
                        plugin_id: settings.plugin_id.clone(),
                        point_id: "settings.panel".into(),
                        kind: "SettingsPanel".into(),
                        id: "settings".into(),
                        resource_kind: "settings".into(),
                        owner_key: settings.plugin_id.clone(),
                        dynamic: false,
                        source_location: None,
                    });
            }
        }
        for panel in &snap.dock_panels {
            snap.extension_contributions
                .push(ExtensionContributionSummary {
                    plugin_id: panel.plugin_id.clone(),
                    point_id: "dock.pane".into(),
                    kind: "DockPane".into(),
                    id: panel.id.clone(),
                    resource_kind: "dock_panel".into(),
                    owner_key: panel.key.clone(),
                    dynamic: true,
                    source_location: panel.source_location.clone(),
                });
        }
        snap.extension_contributions.sort_by(|a, b| {
            a.point_id
                .cmp(&b.point_id)
                .then(a.plugin_id.cmp(&b.plugin_id))
                .then(a.id.cmp(&b.id))
        });

        // performance traces + circuit-breaker rows. Both
        // are cheap clones from the tracker; sort here so the
        // snapshot is deterministic.
        let mut traces: Vec<crate::plugin::devtools::PerformanceTraceSummary> = self
            .budget_tracker
            .traces()
            .into_iter()
            .map(|t| crate::plugin::devtools::PerformanceTraceSummary {
                plugin_id: t.plugin_id,
                generation_id: t.generation_id,
                callback_id: t.callback_id,
                kind: t.kind.as_str().to_string(),
                duration_ms: t.duration_ms,
                ok: t.ok,
                timestamp_unix_ms: t.timestamp_unix_ms,
            })
            .collect();
        traces.sort_by(|a, b| {
            a.timestamp_unix_ms
                .cmp(&b.timestamp_unix_ms)
                .then(a.plugin_id.cmp(&b.plugin_id))
                .then(a.callback_id.cmp(&b.callback_id))
        });
        snap.performance_traces = traces;

        let mut breakers: Vec<crate::plugin::devtools::CircuitBreakerSummary> = self
            .budget_tracker
            .breaker_summaries()
            .into_iter()
            .map(|s| crate::plugin::devtools::CircuitBreakerSummary {
                plugin_id: s.plugin_id,
                generation_id: s.generation_id,
                callback_id: s.callback_id,
                kind: s.kind,
                state: s.state,
                consecutive_failures: s.consecutive_failures,
                count: s.count,
                ok_count: s.ok_count,
                err_count: s.err_count,
                p50_ms: s.p50_ms,
                p95_ms: s.p95_ms,
                last_failure: s.last_failure,
            })
            .collect();
        breakers.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.generation_id.cmp(&b.generation_id))
                .then(a.callback_id.cmp(&b.callback_id))
        });
        snap.circuit_breakers = breakers;

        // Reload history is naturally per-plugin in the store; flatten
        // here in (plugin_id, timestamp) order so external inspectors
        // can group as they please.
        let mut history_keys: Vec<&String> = self.reload_history.keys().collect();
        history_keys.sort();
        for key in history_keys {
            if let Some(bucket) = self.reload_history.get(key) {
                for entry in bucket.iter() {
                    snap.reload_history.push(entry.clone());
                }
            }
        }

        snap.current_context = self.current_ui_context_snapshot();
        populate_authoring_trees(&mut snap);

        snap
    }

    /// Cheap-cloned reload history for `plugin_id`, oldest first.
    /// Empty when the plugin has never been reloaded since load.
    pub fn reload_history(&self, plugin_id: &str) -> Vec<ReloadEventSummary> {
        self.reload_history
            .get(plugin_id)
            .map(|bucket| bucket.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Number of suspended coroutines parked in this plugin's queue.
    /// Used by tests to drive a coroutine to completion via repeated
    /// `tick()` calls.
    pub fn coroutine_count(&self, plugin_id: &str) -> usize {
        self.plugins
            .get(plugin_id)
            .map(|p| p.deferred.borrow().coroutines.len())
            .unwrap_or(0)
    }

    /// Re-invoke every dynamic widget fn the plugin registered and push
    /// the resulting tree into its shared cache cell. Read by the slot
    /// builder on the next render. Errors are logged; the previous
    /// cached value is left in place so a transient Lua error doesn't
    /// blank out the bar.
    /// Resolve and install one autocmd registration into the live
    /// `EventBus`. Records `autocmd.invalid_event` /
    /// `autocmd.invalid_pattern` diagnostics for shape errors and
    /// drops the row when the event name is unknown.
    pub(super) fn install_one_autocmd(
        &mut self,
        plugin_id: &str,
        generation_id: GenerationId,
        raw: api::RawAutocmd,
        local_to_host: &HashMap<u64, GroupId>,
    ) {
        let (canonical_name, subscribed_name) = match events::resolve_event(&raw.event) {
            Ok((descriptor, alias)) => {
                let subscribed = alias.unwrap_or(descriptor.name);
                (descriptor.name, subscribed)
            }
            Err(name) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Warning,
                        "autocmd.invalid_event",
                        format!("unknown event `{name}` ignored"),
                    )
                    .with_generation(generation_id)
                    .with_source(make_lua_span(plugin_id, raw.source_location.as_deref())),
                );
                return;
            }
        };
        if let Some(pat) = raw.pattern.as_deref() {
            if pat.is_empty() {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Warning,
                        "autocmd.invalid_pattern",
                        "empty pattern ignored",
                    )
                    .with_generation(generation_id)
                    .with_source(make_lua_span(plugin_id, raw.source_location.as_deref())),
                );
            }
        }
        let options = api::event::build_options(&raw, local_to_host);
        self.event_bus.register(
            plugin_id.to_string(),
            generation_id,
            subscribed_name,
            canonical_name,
            raw.callback,
            options,
        );
    }
}

fn collect_extension_points() -> Vec<crate::plugin::devtools::ExtensionPointSummary> {
    use crate::plugin::devtools::ExtensionPointSummary;
    git_leviathan_plugin_api::descriptor::extension_point::EXTENSION_POINTS
        .iter()
        .map(|point| ExtensionPointSummary {
            id: point.id.to_string(),
            kind: format!("{:?}", point.kind),
            context_schema: point.context_schema.to_string(),
            contribution_schema: point.contribution_schema.to_string(),
            ordering_model: point.ordering_model.to_string(),
            ownership_rules: point.ownership_rules.to_string(),
            capabilities: point
                .capabilities
                .iter()
                .map(|cap| cap.to_string())
                .collect(),
            render_mount_handler: point.render_mount_handler.to_string(),
        })
        .collect()
}

fn populate_authoring_trees(snap: &mut crate::plugin::devtools::InspectorSnapshot) {
    let diagnostic_codes_by_plugin = {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for diagnostic in &snap.diagnostics {
            map.entry(diagnostic.plugin_id.clone())
                .or_default()
                .push(diagnostic.code.clone());
        }
        for render in &snap.ui_render_diagnostics {
            if render.diagnostic_badge {
                map.entry(render.plugin_id.clone())
                    .or_default()
                    .push(format!("render:{}", render.last_status));
            }
        }
        map
    };

    snap.plugin_tree = snap
        .plugins
        .iter()
        .map(|plugin| {
            let children = [
                format!("contributions:{}", plugin.id),
                format!("commands:{}", plugin.id),
                format!("autocmds:{}", plugin.id),
                format!("capabilities:{}", plugin.id),
                format!("diagnostics:{}", plugin.id),
            ]
            .into_iter()
            .collect();
            crate::plugin::devtools::PluginTreeNode {
                id: plugin.id.clone(),
                label: plugin.name.clone(),
                version: plugin.version.clone(),
                children,
            }
        })
        .collect();

    let hidden = snap
        .contribution_overrides
        .iter()
        .filter(|row| row.hidden)
        .map(|row| {
            format!(
                "{}:{}:{}:{}",
                row.plugin_id, row.region, row.container, row.id
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    let slot_visibility = snap
        .slots
        .iter()
        .map(|slot| {
            let key = format!(
                "{}:{}:{}:{}",
                slot.owner_plugin_id, slot.region, slot.container, slot.id
            );
            (key.clone(), !slot.hidden && !hidden.contains(&key))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut contribution_tree = Vec::new();
    for slot in &snap.slots {
        let key = format!(
            "{}:{}:{}:{}",
            slot.owner_plugin_id, slot.region, slot.container, slot.id
        );
        contribution_tree.push(crate::plugin::devtools::ContributionTreeNode {
            id: slot.id.clone(),
            plugin_id: slot.owner_plugin_id.clone(),
            kind: "slot".into(),
            location: format!("{}:{}", slot.region, slot.container),
            visible: !slot.hidden && !hidden.contains(&key),
            diagnostics: diagnostic_codes_by_plugin
                .get(&slot.owner_plugin_id)
                .cloned()
                .unwrap_or_default(),
        });
    }
    for contribution in &snap.extension_contributions {
        if contribution.resource_kind == "slot" {
            continue;
        }
        contribution_tree.push(crate::plugin::devtools::ContributionTreeNode {
            id: contribution.id.clone(),
            plugin_id: contribution.plugin_id.clone(),
            kind: contribution.kind.clone(),
            location: contribution.point_id.clone(),
            visible: slot_visibility
                .get(&contribution.owner_key)
                .copied()
                .unwrap_or(true),
            diagnostics: diagnostic_codes_by_plugin
                .get(&contribution.plugin_id)
                .cloned()
                .unwrap_or_default(),
        });
    }
    for panel in &snap.dock_panels {
        contribution_tree.push(crate::plugin::devtools::ContributionTreeNode {
            id: panel.id.clone(),
            plugin_id: panel.plugin_id.clone(),
            kind: "dock_panel".into(),
            location: panel.area.clone(),
            visible: panel.open,
            diagnostics: diagnostic_codes_by_plugin
                .get(&panel.plugin_id)
                .cloned()
                .unwrap_or_default(),
        });
    }
    contribution_tree.sort_by(|a, b| {
        a.plugin_id
            .cmp(&b.plugin_id)
            .then(a.kind.cmp(&b.kind))
            .then(a.id.cmp(&b.id))
    });
    snap.contribution_tree = contribution_tree;

    snap.command_tree = snap
        .commands
        .iter()
        .map(|command| {
            let mut children = vec![format!("context:{}", command.context)];
            if !command.capabilities.is_empty() {
                children.push(format!("capabilities:{}", command.capabilities.join(",")));
            }
            if !command.plugin_invocation_capabilities.is_empty() {
                children.push(format!(
                    "plugin_invocation_capabilities:{}",
                    command.plugin_invocation_capabilities.join(",")
                ));
            }
            crate::plugin::devtools::CommandTreeNode {
                id: command.name.clone(),
                plugin_id: command.plugin_id.clone(),
                title: command.title.clone(),
                enabled: command.enabled,
                children,
            }
        })
        .collect();

    snap.autocmd_tree = snap
        .autocmds
        .iter()
        .map(|autocmd| {
            let status = if autocmd.disabled {
                "disabled"
            } else if autocmd.consecutive_failures > 0 {
                "failing"
            } else {
                "active"
            };
            crate::plugin::devtools::AutocmdTreeNode {
                id: autocmd.id.to_string(),
                plugin_id: autocmd.plugin_id.clone(),
                event: autocmd.event.clone(),
                status: status.into(),
                children: vec![
                    format!("fires:{}", autocmd.fires),
                    format!("failures:{}", autocmd.failures),
                ],
            }
        })
        .collect();
}

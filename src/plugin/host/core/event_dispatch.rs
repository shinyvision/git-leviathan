use super::*;

impl PluginHost {
    /// Fire a host-side event with no payload.
    /// Equivalent to `fire_event_typed(event, {})`.
    pub fn fire_event(&mut self, event: &str) {
        self.fire_event_typed(event, EventPayload::new());
    }

    /// Fire a host-side typed event. The funnel:
    /// 1. resolves the event name against the descriptor table;
    /// 2. validates the payload shape against the descriptor (records
    ///    `autocmd.payload_mismatch` for shape errors but proceeds);
    /// 3. invokes every matching autocmd in (priority desc, id asc)
    ///    order, honouring `pattern`, `debounce_ms`, `once`, and the
    ///    consecutive-failure disable threshold;
    /// 4. refreshes dynamic widgets for every plugin whose callbacks
    ///    actually ran.
    pub fn fire_event_typed(&mut self, event: &str, payload: EventPayload) {
        // lazy loading: probe the lazy registry for a plugin that
        // declared this event as an activation trigger. Activation
        // re-fires the event through this same funnel so the now-live
        // autocmds observe it.
        if let Ok((canonical, _)) = events::resolve_event(event) {
            let canonical_name = canonical.name;
            let activate = self
                .lazy_registry
                .match_event(canonical_name)
                .or_else(|| self.lazy_registry.match_event(event))
                .map(|e| e.plugin_id.clone());
            if let Some(plugin_id) = activate {
                if self
                    .activate_now(&plugin_id, "event", format!("event:{canonical_name}"))
                    .is_ok()
                {
                    // Continue the dispatch — we want the freshly
                    // installed autocmds to observe this same event.
                }
            }
        }
        let (canonical, _alias_used) = match events::resolve_event(event) {
            Ok(pair) => pair,
            Err(_) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from("<host>"),
                        DiagnosticSeverity::Warning,
                        "autocmd.invalid_event",
                        format!("fire_event_typed: unknown event `{event}`"),
                    )
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("event:{event}"),
                    }),
                );
                return;
            }
        };
        let _validated = events::validate_payload(canonical, &payload, &self.diagnostics);
        self.invalidate_decorations_for_event(canonical.name);

        let mut affected: HashSet<String> = HashSet::new();
        self.dispatch_for_name(canonical, None, &payload, &mut affected);

        let plugin_state_changed = !affected.is_empty();
        let event_cause = ui_cause_for_event(canonical.name);
        let mut causes = Vec::new();
        if let Some(cause) = event_cause {
            causes.push(cause);
        }
        if plugin_state_changed {
            let mut targeted = HashSet::new();
            for pid in &affected {
                targeted.insert(pid.clone());
            }
            if !causes.is_empty() {
                let other_plugins: HashSet<String> = self
                    .plugins
                    .keys()
                    .filter(|pid| !targeted.contains(*pid))
                    .cloned()
                    .collect();
                self.invalidate_dynamic_widgets(&causes, Some(&other_plugins));
            }
            let mut targeted_causes = causes;
            targeted_causes.push(UiInvalidationCause::PluginStateChanged);
            self.invalidate_dynamic_widgets(&targeted_causes, Some(&targeted));
        } else if !causes.is_empty() {
            self.invalidate_dynamic_widgets(&causes, None);
        }
        if plugin_state_changed {
            self.extension_registry.invalidate_decorations(
                crate::plugin::extensions::DecorationInvalidationReason::PluginState,
            );
        }
    }

    fn invalidate_decorations_for_event(&self, event: &str) {
        use crate::plugin::extensions::DecorationInvalidationReason as Reason;
        let reason = match event {
            "CommitSelected" => Some(Reason::Selection),
            "DiffLoaded" => Some(Reason::DiffLoaded),
            "RefsChanged" | "BranchChanged" | "HeadChanged" | "RepositoryChanged" => {
                Some(Reason::RefsChanged)
            }
            _ => None,
        };
        if let Some(reason) = reason {
            self.extension_registry.invalidate_decorations(reason);
        }
    }

    pub fn dispatch_test_event(&mut self, event: &str, payload: serde_json::Value) {
        let map = match payload {
            serde_json::Value::Object(m) => m,
            serde_json::Value::Null => EventPayload::new(),
            other => {
                let mut m = EventPayload::new();
                m.insert("value".into(), other);
                m
            }
        };
        self.fire_event_typed(event, map);
    }

    /// Advance the host's virtual debounce clock. Tests use this to
    /// drive `debounce_ms` deterministically.
    pub fn advance_event_clock(&mut self, delta_ms: u64) {
        self.event_bus.advance_clock(delta_ms);
    }

    pub(super) fn dispatch_for_name(
        &mut self,
        canonical: &'static event_descriptor::ApiEvent,
        alias_name: Option<&'static str>,
        payload: &EventPayload,
        affected: &mut HashSet<String>,
    ) {
        // Look up the dispatch name (canonical or alias) and snapshot
        // the iteration order so concurrent removals from `once` /
        // disable don't shift the iteration index.
        let dispatch_name = alias_name.unwrap_or(canonical.name);
        let entries = self.event_bus.entries();
        let order: Vec<usize> = events::dispatch_order(entries)
            .into_iter()
            .filter(|i| entries[*i].event == dispatch_name)
            .collect();
        if order.is_empty() {
            return;
        }
        let now_ms = self.event_bus.clock_ms();
        let mut to_remove: Vec<u64> = Vec::new();
        for index in order {
            // Re-fetch the entry by index because earlier callbacks
            // may have mutated `entries` (e.g. on a recursive fire).
            let snapshot = {
                let entries = self.event_bus.entries();
                if index >= entries.len() {
                    continue;
                }
                let entry = &entries[index];
                EntrySnapshot {
                    id: entry.id.get(),
                    plugin_id: entry.plugin_id.clone(),
                    generation_id: entry.generation_id,
                    once: entry.options.once,
                    debounce_ms: entry.options.debounce_ms,
                    pattern_present: entry.options.pattern.is_some(),
                    matches_pattern: events::pattern_matches(entry, payload),
                    last_fire_clock_ms: entry.runtime.last_fire_clock_ms,
                    disabled: entry.runtime.disabled,
                }
            };
            if snapshot.disabled {
                continue;
            }
            if snapshot.pattern_present && !snapshot.matches_pattern {
                continue;
            }
            if snapshot.debounce_ms > 0 {
                if let Some(prev) = snapshot.last_fire_clock_ms {
                    if now_ms.saturating_sub(prev) < snapshot.debounce_ms {
                        continue;
                    }
                }
            }
            let outcome = self.invoke_one_callback(
                &snapshot.plugin_id,
                snapshot.generation_id,
                snapshot.id,
                canonical,
                alias_name,
                payload,
            );
            // Bookkeeping (counters, last_fire, once removal,
            // disable on threshold). Done after the callback returns
            // so the snapshot reflects post-call state.
            self.update_runtime(snapshot.id, &outcome, now_ms);
            if matches!(outcome, DispatchOutcome::Ok | DispatchOutcome::Error) {
                affected.insert(snapshot.plugin_id);
                if snapshot.once {
                    to_remove.push(snapshot.id);
                }
            }
        }
        for id in to_remove {
            self.event_bus
                .entries_mut()
                .retain(|entry| entry.id.get() != id);
        }
    }

    pub(super) fn invoke_one_callback(
        &self,
        plugin_id: &str,
        generation_id: GenerationId,
        autocmd_id: u64,
        canonical: &'static event_descriptor::ApiEvent,
        alias_used: Option<&'static str>,
        payload: &EventPayload,
    ) -> DispatchOutcome {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return DispatchOutcome::Skipped;
        };
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        // Resolve the callback through the entry's RegistryKey via the
        // entries vector — the snapshot only carries ids, so dive
        // back in here.
        let key_ptr: *const RegistryKey = {
            let entries = self.event_bus.entries();
            match entries.iter().find(|e| e.id.get() == autocmd_id) {
                Some(entry) => &entry.callback as *const RegistryKey,
                None => return DispatchOutcome::Skipped,
            }
        };
        // SAFETY: the EventBus owns the RegistryKey for the lifetime
        // of this call (we never remove during `invoke_one_callback`,
        // only in `dispatch_for_name` after we return). Borrowing as
        // a `*const` lets us avoid holding a borrow on `self.event_bus`
        // while we call into Lua.
        let key: &RegistryKey = unsafe { &*key_ptr };
        let func: Function = match plugin.lua().registry_value(key) {
            Ok(f) => f,
            Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "lua.handler_lookup_failed",
                        format!("autocmd handler lookup failed for {}: {e}", canonical.name),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("autocmd:{}", canonical.name),
                    }),
                );
                return DispatchOutcome::Error;
            }
        };
        let payload_table = match build_payload_table(plugin.lua(), canonical, alias_used, payload)
        {
            Ok(t) => t,
            Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "autocmd.payload_mismatch",
                        format!("could not build typed payload: {e}"),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("autocmd:{}", canonical.name),
                    }),
                );
                return DispatchOutcome::Error;
            }
        };
        let pid = PluginId::from(plugin_id);
        let callback_id = format!("autocmd:{}", canonical.name);
        let perf_outcome = self.budget_tracker.track_call::<(), mlua::Error>(
            CallbackKind::EventCallback,
            &pid,
            generation_id,
            &callback_id,
            || func.call::<()>(payload_table),
        );
        match perf_outcome {
            PerfOutcome::Skipped => DispatchOutcome::Skipped,
            PerfOutcome::Ok(()) => DispatchOutcome::Ok,
            PerfOutcome::Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "autocmd.callback_failed",
                        format!("autocmd handler error for {}", canonical.name),
                    )
                    .with_generation(generation_id)
                    .with_mlua_error(&chunk_name, &e)
                    .with_context(serde_json::json!({
                        "event": canonical.name,
                        "alias_used": alias_used,
                        "autocmd_id": autocmd_id,
                    })),
                );
                DispatchOutcome::Error
            }
        }
    }

    pub(super) fn update_runtime(
        &mut self,
        autocmd_id: u64,
        outcome: &DispatchOutcome,
        now_ms: u64,
    ) {
        let entries = self.event_bus.entries_mut();
        let Some(entry) = entries.iter_mut().find(|e| e.id.get() == autocmd_id) else {
            return;
        };
        match outcome {
            DispatchOutcome::Ok => {
                entry.runtime.fires += 1;
                entry.runtime.consecutive_failures = 0;
                entry.runtime.last_fire_clock_ms = Some(now_ms);
            }
            DispatchOutcome::Error => {
                entry.runtime.fires += 1;
                entry.runtime.failures += 1;
                entry.runtime.consecutive_failures += 1;
                entry.runtime.last_fire_clock_ms = Some(now_ms);
                if entry.runtime.consecutive_failures >= MAX_CONSECUTIVE_FAILURES
                    && !entry.runtime.disabled
                {
                    entry.runtime.disabled = true;
                    let plugin_id = entry.plugin_id.clone();
                    let generation_id = entry.generation_id;
                    let event_name = entry.event;
                    let consecutive = entry.runtime.consecutive_failures;
                    let source_location = entry.options.source_location.clone();
                    // Drop the &mut entry borrow before recording
                    // the diagnostic so the diagnostic store call
                    // doesn't reborrow self.event_bus.
                    let _ = entry;
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Error,
                            "autocmd.disabled_after_failures",
                            format!(
                                "autocmd for {event_name} disabled after {consecutive} \
                                 consecutive failures"
                            ),
                        )
                        .with_generation(generation_id)
                        .with_source(make_lua_span(&plugin_id, source_location.as_deref()))
                        .with_context(serde_json::json!({
                            "event": event_name,
                            "consecutive_failures": consecutive,
                            "autocmd_id": autocmd_id,
                        })),
                    );
                }
            }
            DispatchOutcome::Skipped => {}
        }
    }

    /// Push the active repository identity and latest branch refs into every
    /// plugin's `leviathan.repository`,
    /// invoke `BranchChanged` autocmd subscribers, and refresh every
    /// plugin's dynamic main-bar widgets.
    ///
    /// Widgets are refreshed unconditionally — not only for subscribers —
    /// because `leviathan.repository` is a host-owned global that any
    /// widget fn might read. Requiring an opt-in autocmd just to trigger
    /// a widget refresh would force plugins to declare an empty callback
    /// as a refresh hint. Refresh is cheap (one Lua call per dynamic
    /// slot), diffing by `last_repository_hash` keeps the common unchanged
    /// path free; the explicit `BranchChanged` event is still useful for
    /// plugins that want to react imperatively (toasts, logs, etc.).
    ///
    /// Cheap no-op when the repository snapshot hash matches the last sync — callers can
    /// invoke liberally from app-level update hooks without tracking
    /// change detection themselves.
    pub fn sync_repository(
        &mut self,
        repo_name: &str,
        workdir_path: &str,
        current_branch_name: &str,
        head_hash: &str,
        default_remote_name: &str,
        refs: &[RepoRef],
    ) {
        let hash = compute_repo_hash(
            repo_name,
            workdir_path,
            current_branch_name,
            head_hash,
            default_remote_name,
            refs,
        );
        if self.last_repository_hash == Some(hash) {
            return;
        }
        self.last_repository_hash = Some(hash);

        for plugin in self.plugins.values() {
            let plugin_id = plugin.id().to_string();
            let generation_id = plugin.generation.generation_id;
            let table = match api::repository::build_table(
                plugin.lua(),
                repo_name,
                workdir_path,
                current_branch_name,
                head_hash,
                default_remote_name,
                refs,
            ) {
                Ok(t) => t,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Error,
                            "host.repository_table_build_failed",
                            format!("build leviathan.repository failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan.repository (sync)".into(),
                        }),
                    );
                    continue;
                }
            };
            let leviathan: Table = match plugin.lua().globals().get("leviathan") {
                Ok(t) => t,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Error,
                            "host.leviathan_global_missing",
                            format!("`leviathan` global missing: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan (global)".into(),
                        }),
                    );
                    continue;
                }
            };
            if let Err(e) = leviathan.set("repository", table) {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "host.repository_table_set_failed",
                        format!("set leviathan.repository failed: {e}"),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: "leviathan.repository (sync)".into(),
                    }),
                );
            }
        }

        let has_remote = !default_remote_name.is_empty();
        let workdir_buf = PathBuf::from(workdir_path);
        self.last_repository_shape = Some(RepositoryShapeFacts {
            repo_name: repo_name.to_string(),
            current_branch: current_branch_name.to_string(),
            head_hash: head_hash.to_string(),
            default_remote: default_remote_name.to_string(),
            has_remote,
            workdir: workdir_buf,
        });
        self.refresh_command_active_context();

        // Run BranchChanged callbacks first so any Lua-side state they
        // mutate is fresh before widgets re-read the globals. The
        // payload mirrors the new typed `BranchChanged` schema
        // (`name`, `head_hash`).
        let mut payload = EventPayload::new();
        payload.insert(
            "name".into(),
            serde_json::Value::String(current_branch_name.to_string()),
        );
        payload.insert(
            "head_hash".into(),
            serde_json::Value::String(head_hash.to_string()),
        );
        self.fire_event_typed("BranchChanged", payload);

        self.probe_lazy_repository_triggers();
    }

    /// lazy loading: walk the lazy registry's repository-shape and
    /// file-presence triggers against the cached facts and activate
    /// matching plugins. Called after every `sync_repository`.
    pub(super) fn probe_lazy_repository_triggers(&mut self) {
        let Some(facts) = self.last_repository_shape.clone() else {
            return;
        };
        // Repository shape predicates.
        loop {
            let next = self
                .lazy_registry
                .match_repo_shape(&facts.current_branch, facts.has_remote)
                .map(|e| e.plugin_id.clone());
            match next {
                Some(plugin_id) => {
                    let _ = self.activate_now(
                        &plugin_id,
                        "repository_shape",
                        "repository_shape".to_string(),
                    );
                }
                None => break,
            }
        }
        // File presence: collect file lists snapshot first, then
        // iterate so we can mutate the registry inside the loop.
        let candidates: Vec<(String, Vec<PathBuf>)> = self
            .lazy_registry
            .entries()
            .iter()
            .filter(|e| e.status == crate::plugin::activation::LazyStatus::Lazy)
            .map(|e| (e.plugin_id.clone(), e.files.clone()))
            .collect();
        for (plugin_id, files) in candidates {
            for rel in files {
                let abs = facts.workdir.join(&rel);
                if abs.exists() {
                    let _ =
                        self.activate_now(&plugin_id, "file", format!("file:{}", rel.display()));
                    break;
                }
            }
        }
    }

    /// Drain the queue of `tab_registry.{add,remove,select}` ops Lua
    /// pushed since the last call. App applies them through `TabManager`.
    pub fn take_pending_tab_ops(&mut self) -> Vec<TabRegistryOp> {
        std::mem::take(&mut *self.pending_tab_ops.borrow_mut())
    }

    /// Push a fresh tabs snapshot into every plugin's
    /// `leviathan.tab_registry.{list, current}`. Cheap no-op when the
    /// snapshot hash matches the last sync, mirroring `sync_repository`.
    /// Does not fire any tab-lifecycle events — those are explicit at the
    /// app's tab-mutation sites.
    ///
    /// Refreshes every plugin's dynamic widgets after the table is set.
    /// Same rationale as `sync_repository`: `leviathan.tab_registry` is a
    /// host-owned global that any `widget = function() ... end` may
    /// read, even from a plugin that didn't subscribe to a tab event.
    pub fn sync_tab_registry(&mut self, snapshot: &TabsSnapshot) -> Option<TabChange> {
        if &self.last_tab_snapshot == snapshot {
            return None;
        }
        let change = TabChange::diff(&self.last_tab_snapshot, snapshot);
        self.last_tab_snapshot = snapshot.clone();
        self.refresh_command_active_context();

        for plugin in self.plugins.values() {
            let plugin_id = plugin.id().to_string();
            let generation_id = plugin.generation.generation_id;
            let leviathan: Table = match plugin.lua().globals().get("leviathan") {
                Ok(t) => t,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Error,
                            "host.leviathan_global_missing",
                            format!("`leviathan` global missing: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: "leviathan (global)".into(),
                        }),
                    );
                    continue;
                }
            };
            if let Err(e) = api::tab_registry::refresh(plugin.lua(), &leviathan, snapshot) {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "host.tab_registry_refresh_failed",
                        format!("tab_registry refresh failed: {e}"),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: "leviathan.tab_registry (sync)".into(),
                    }),
                );
            }
        }

        self.invalidate_dynamic_widgets(&[UiInvalidationCause::TabChanged], None);

        Some(change)
    }

    pub fn sync_selection(
        &mut self,
        snapshot: crate::plugin::ui::context::SelectionContextSnapshot,
    ) -> bool {
        if self.last_selection_snapshot == snapshot {
            return false;
        }
        self.last_selection_snapshot = snapshot;
        self.refresh_command_active_context();
        self.invalidate_dynamic_widgets(&[UiInvalidationCause::SelectionChanged], None);
        true
    }

    pub fn take_pending_ui_scrolls(&mut self) -> Vec<crate::plugin::ui::effects::ScrollToRequest> {
        self.pending_ui_effects.take_scroll_to()
    }

    pub fn tab_snapshot(&self) -> &TabsSnapshot {
        &self.last_tab_snapshot
    }

    pub fn sync_focus(&mut self, snapshot: crate::plugin::ui::focus::FocusSnapshot) -> bool {
        if self.last_focus_snapshot == snapshot {
            return false;
        }
        let prev = std::mem::replace(&mut self.last_focus_snapshot, snapshot);
        let next = self.last_focus_snapshot.clone();
        self.refresh_command_active_context();
        self.invalidate_dynamic_widgets(&[UiInvalidationCause::FocusChanged], None);
        let payload = crate::plugin::ui::focus::focus_event_payload(&prev, &next);
        self.fire_event_typed("FocusChanged", payload);
        true
    }

    pub fn last_focus_snapshot(&self) -> &crate::plugin::ui::focus::FocusSnapshot {
        &self.last_focus_snapshot
    }
}

fn ui_cause_for_event(event: &str) -> Option<UiInvalidationCause> {
    match event {
        "ThemeChanged" => Some(UiInvalidationCause::ThemeChanged),
        "CommitSelected" => Some(UiInvalidationCause::SelectionChanged),
        "DiffLoaded" => Some(UiInvalidationCause::DiffLoaded),
        "LayoutChanged" => Some(UiInvalidationCause::LayoutChanged),
        "RefsChanged" | "BranchChanged" | "HeadChanged" | "RepositoryChanged" => {
            Some(UiInvalidationCause::RepositoryChanged)
        }
        _ => None,
    }
}

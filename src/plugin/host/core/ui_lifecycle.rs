use super::*;

// Extra accessor/dispatch methods. Kept in a second `impl` block to
// isolate the new hook-system additions above from the original API.
impl PluginHost {
    /// Sandbox root for a plugin's assets. Returns `None` if the
    /// plugin id is unknown.
    pub fn plugin_root(&self, plugin_id: &str) -> Option<&Path> {
        self.plugins.get(plugin_id).map(|p| p.root.as_path())
    }

    pub fn active_screen(&self) -> Option<(&str, &str)> {
        self.active_screen
            .as_ref()
            .map(|(a, b)| (a.as_str(), b.as_str()))
    }

    pub fn clear_active_screen(&mut self) {
        self.active_screen = None;
        self.widget_tree = None;
    }

    pub fn widget_tree(&self) -> Option<&WidgetAst> {
        self.widget_tree.as_ref()
    }

    pub fn screen_exists(&self, plugin_id: &str, screen_id: &str) -> bool {
        self.plugins
            .get(plugin_id)
            .is_some_and(|p| p.screens.contains_key(screen_id))
    }

    pub fn screen_summary(
        &self,
        plugin_id: &str,
        screen_id: &str,
    ) -> Option<crate::screens::plugin::PluginScreenSummary> {
        let screen = self.plugins.get(plugin_id)?.screens.get(screen_id)?;
        Some(crate::screens::plugin::PluginScreenSummary {
            plugin_id: plugin_id.to_string(),
            screen_id: screen_id.to_string(),
            title: screen.title.clone(),
            breadcrumbs: screen.breadcrumbs.clone(),
            bind_repository: screen.bind_repository,
        })
    }

    pub fn take_pending_navigation_effects(
        &mut self,
    ) -> Vec<crate::plugin::navigation::PluginNavigationEffect> {
        std::mem::take(&mut self.pending_navigation_effects)
    }

    pub fn split_sizes(&self) -> &HashMap<String, Vec<f32>> {
        &self.split_sizes
    }

    pub fn dispatch_event(
        &mut self,
        plugin_id: &str,
        screen_id: &str,
        event: &str,
        value: serde_json::Value,
    ) {
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        let mut pending_diagnostics: Vec<PluginDiagnostic> = Vec::new();
        let mut disable_after_failures = false;
        let mut effects = Vec::new();
        let tabs = self.last_tab_snapshot.clone();
        let focus = self.last_focus_snapshot.clone();
        // A screen event can mutate plugin/tab state, so mark activity for the
        // persistence gate — before taking the mutable plugin borrow below.
        self.bump_lua_activity();
        {
            let Some(plugin) = self.plugins.get_mut(plugin_id) else {
                return;
            };
            let Some(screen_def) = plugin.screens.get(screen_id) else {
                return;
            };
            let generation_id = plugin.generation.generation_id;
            let update_fn: Function = match plugin.lua().registry_value(&screen_def.update) {
                Ok(f) => f,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("screen update lookup failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("screen:{screen_id}.update"),
                        }),
                    );
                    return;
                }
            };
            let state_val: LuaValue = match plugin.screen_state.get(screen_id) {
                Some(k) => plugin.lua().registry_value(k).unwrap_or(LuaValue::Nil),
                None => LuaValue::Nil,
            };
            let value_lua = match plugin.lua().to_value(&value) {
                Ok(v) => v,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.value_conversion_failed",
                            format!("event value conv failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("screen:{screen_id}.update"),
                        }),
                    );
                    return;
                }
            };
            let ctx_lua = match screen_context_lua(plugin, &tabs, Some(&focus)) {
                Ok(v) => v,
                Err(e) => {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.value_conversion_failed",
                            format!("screen context conversion failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("screen:{screen_id}.update"),
                        }),
                    );
                    return;
                }
            };
            let cb_id = format!("screen:{screen_id}.update");
            let result = self.budget_tracker.track_call::<Table, mlua::Error>(
                CallbackKind::UiCallback,
                &PluginId::from(plugin_id),
                generation_id,
                &cb_id,
                || update_fn.call((state_val, event.to_string(), value_lua, ctx_lua)),
            );
            match result {
                PerfOutcome::Ok(action) => {
                    if let Ok(new_state) = action.get::<LuaValue>("state") {
                        if !matches!(new_state, LuaValue::Nil) {
                            if let Ok(key) = plugin.lua().create_registry_value(new_state) {
                                record_screen_state_resource(&plugin.ledger(), screen_id, None);
                                plugin.screen_state.insert(screen_id.to_string(), key);
                            }
                        }
                    }
                    effects = parse_navigation_effects(
                        plugin_id,
                        screen_id,
                        generation_id,
                        "screen.update",
                        &action,
                        &self.diagnostics,
                    );
                }
                PerfOutcome::Skipped => {
                    // The circuit breaker already tripped and the body never
                    // ran — a transient skip, not a failure. Disabling the
                    // whole plugin here (and making `reset_breaker` useless for
                    // screen callbacks) is wrong; no-op like dispatch_slot_click.
                }
                PerfOutcome::Err(e) => {
                    disable_after_failures = self.budget_tracker.is_tripped(
                        &PluginId::from(plugin_id),
                        generation_id,
                        &cb_id,
                    );
                    pending_diagnostics.push(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            format!("screen update error in {screen_id}"),
                        )
                        .with_generation(generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                }
            }
        }
        for diag in pending_diagnostics {
            self.diagnostics.record(diag);
        }
        if disable_after_failures {
            self.disable_plugin(plugin_id);
            return;
        }
        self.apply_navigation_effects(&effects);
        self.pending_navigation_effects.extend(effects);
        self.refresh_active_widget_tree();
        self.drain_lua_command_effects();
    }

    pub fn dispatch_overlay_event(
        &mut self,
        plugin_id: &str,
        overlay_id: &str,
        event: &str,
        value: serde_json::Value,
    ) {
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        let mut disable_after_failures = false;
        let effects: Vec<crate::plugin::navigation::PluginNavigationEffect> = {
            let Some(plugin) = self.plugins.get(plugin_id) else {
                return;
            };
            let generation_id = plugin.generation.generation_id;
            let func: Function = {
                let callbacks = plugin.overlay_callbacks.borrow();
                let Some(key) = callbacks.get(overlay_id) else {
                    return;
                };
                match plugin.lua().registry_value(key) {
                    Ok(f) => f,
                    Err(e) => {
                        self.diagnostics.record(
                            PluginDiagnostic::new(
                                PluginId::from(plugin_id),
                                DiagnosticSeverity::Error,
                                "lua.handler_lookup_failed",
                                format!("overlay handler lookup failed: {e}"),
                            )
                            .with_generation(generation_id)
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: format!("overlay:{overlay_id}.on_event"),
                                },
                            ),
                        );
                        return;
                    }
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
                            format!("overlay value conv failed: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("overlay:{overlay_id}.on_event"),
                        }),
                    );
                    return;
                }
            };
            let cb_id = format!("overlay:{overlay_id}.on_event");
            self.bump_lua_activity();
            match self
                .budget_tracker
                .track_call::<Option<Table>, mlua::Error>(
                    CallbackKind::UiCallback,
                    &PluginId::from(plugin_id),
                    generation_id,
                    &cb_id,
                    || func.call((overlay_id.to_string(), event.to_string(), value_lua)),
                ) {
                PerfOutcome::Ok(Some(t)) => parse_navigation_effects(
                    plugin_id,
                    overlay_id,
                    generation_id,
                    "overlay.on_event",
                    &t,
                    &self.diagnostics,
                ),
                PerfOutcome::Ok(None) => Vec::new(),
                PerfOutcome::Skipped => {
                    disable_after_failures = true;
                    Vec::new()
                }
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
                            format!("overlay handler error in {overlay_id}"),
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
        self.apply_navigation_effects(&effects);
        self.pending_navigation_effects.extend(effects);
        self.drain_lua_command_effects();
    }

    pub fn dispatch_dialog_button_callback(
        &mut self,
        plugin_id: &str,
        dialog_id: &str,
        button_id: &str,
    ) {
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        let mut disable_after_failures = false;
        let effects: Vec<crate::plugin::navigation::PluginNavigationEffect> = {
            let Some(plugin) = self.plugins.get(plugin_id) else {
                return;
            };
            let generation_id = plugin.generation.generation_id;
            let func: Function = {
                let callbacks = plugin.dialog_callbacks.borrow();
                let Some(key) = callbacks.get(plugin_id, dialog_id, button_id) else {
                    return;
                };
                match plugin.lua().registry_value(key) {
                    Ok(f) => f,
                    Err(e) => {
                        self.diagnostics.record(
                            PluginDiagnostic::new(
                                PluginId::from(plugin_id),
                                DiagnosticSeverity::Error,
                                "lua.handler_lookup_failed",
                                format!("dialog button callback lookup failed: {e}"),
                            )
                            .with_generation(generation_id)
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: format!("dialog:{dialog_id}.button:{button_id}.on_click"),
                                },
                            ),
                        );
                        return;
                    }
                }
            };
            let cb_id = format!("dialog:{dialog_id}.button:{button_id}.on_click");
            self.bump_lua_activity();
            match self
                .budget_tracker
                .track_call::<Option<Table>, mlua::Error>(
                    CallbackKind::UiCallback,
                    &PluginId::from(plugin_id),
                    generation_id,
                    &cb_id,
                    || func.call(button_id.to_string()),
                ) {
                PerfOutcome::Ok(Some(t)) => parse_navigation_effects(
                    plugin_id,
                    dialog_id,
                    generation_id,
                    "dialog.button.on_click",
                    &t,
                    &self.diagnostics,
                ),
                PerfOutcome::Ok(None) => Vec::new(),
                PerfOutcome::Skipped => {
                    disable_after_failures = true;
                    Vec::new()
                }
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
                            format!("dialog button callback error in {dialog_id}.{button_id}"),
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
        self.apply_navigation_effects(&effects);
        self.pending_navigation_effects.extend(effects);
        let only = HashSet::from([plugin_id.to_string()]);
        self.invalidate_dynamic_widgets(&[UiInvalidationCause::PluginStateChanged], Some(&only));
        self.drain_lua_command_effects();
    }

    pub fn autofocus_overlay_text_input_id(&self) -> Option<iced::widget::Id> {
        for overlay in self.extension_registry.overlays() {
            let Some(node_id) = autofocus_text_input_node_id(&overlay.widget) else {
                continue;
            };
            let scope_key = format!("overlay:{}", overlay.id);
            return Some(plugin_text_input_id(
                &overlay.plugin_id,
                &scope_key,
                node_id,
            ));
        }
        None
    }

    /// Snapshot of a screen's current Lua state as JSON. Returns `None`
    /// when the plugin is unloaded, the screen has no state yet, or the
    /// state value can't be serialised.
    pub fn screen_state_json(&self, plugin_id: &str, screen_id: &str) -> Option<serde_json::Value> {
        let plugin = self.plugins.get(plugin_id)?;
        let key = plugin.screen_state.get(screen_id)?;
        let v: LuaValue = plugin.lua().registry_value(key).ok()?;
        plugin.lua().from_value(v).ok()
    }

    pub fn serialize_screen_state(
        &self,
        plugin_id: &str,
        screen_id: &str,
    ) -> Option<serde_json::Value> {
        let plugin = self.plugins.get(plugin_id)?;
        let screen = plugin.screens.get(screen_id)?;
        let key = plugin.screen_state.get(screen_id)?;
        let state: LuaValue = plugin.lua().registry_value(key).ok()?;
        let out = if let Some(ser_key) = &screen.serialize {
            let ser: Function = plugin.lua().registry_value(ser_key).ok()?;
            ser.call::<LuaValue>(state).ok()?
        } else {
            state
        };
        plugin.lua().from_value(out).ok()
    }

    pub fn deserialize_screen_state(
        &mut self,
        plugin_id: &str,
        screen_id: &str,
        value: serde_json::Value,
    ) -> bool {
        let tabs = self.last_tab_snapshot.clone();
        let focus = self.last_focus_snapshot.clone();
        let Some(plugin) = self.plugins.get_mut(plugin_id) else {
            return false;
        };
        let Some(screen) = plugin.screens.get(screen_id) else {
            return false;
        };
        let input = match plugin.lua().to_value(&value) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let state = if let Some(de_key) = &screen.deserialize {
            let Ok(de_fn) = plugin.lua().registry_value::<Function>(de_key) else {
                return false;
            };
            let Ok(ctx_lua) = screen_context_lua(plugin, &tabs, Some(&focus)) else {
                return false;
            };
            match de_fn.call::<LuaValue>((input, ctx_lua)) {
                Ok(v) => v,
                Err(_) => return false,
            }
        } else {
            input
        };
        match plugin.lua().create_registry_value(state) {
            Ok(key) => {
                record_screen_state_resource(&plugin.ledger(), screen_id, None);
                plugin.screen_state.insert(screen_id.to_string(), key);
                true
            }
            Err(_) => false,
        }
    }

    pub fn can_close_screen(&self, plugin_id: &str, screen_id: &str) -> bool {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return true;
        };
        let Some(screen) = plugin.screens.get(screen_id) else {
            return true;
        };
        let Some(can_close_key) = &screen.can_close else {
            return true;
        };
        let Ok(func) = plugin.lua().registry_value::<Function>(can_close_key) else {
            return true;
        };
        let state_val: LuaValue = match plugin.screen_state.get(screen_id) {
            Some(k) => plugin.lua().registry_value(k).unwrap_or(LuaValue::Nil),
            None => LuaValue::Nil,
        };
        let Ok(ctx_lua) = screen_context_lua(
            plugin,
            &self.last_tab_snapshot,
            Some(&self.last_focus_snapshot),
        ) else {
            return true;
        };
        func.call::<bool>((state_val, ctx_lua)).unwrap_or(true)
    }

    /// Read a top-level Lua global from `plugin_id`'s VM as `i64`. Used
    /// by tests to observe side-effects of plugin code (e.g. results of
    /// `leviathan.services.get(...).method(...)`). Returns `None` when
    /// the plugin is unknown, the global doesn't exist, or the value is
    /// neither integer nor number.
    pub fn plugin_global_i64(&self, plugin_id: &str, name: &str) -> Option<i64> {
        let plugin = self.plugins.get(plugin_id)?;
        let v: LuaValue = plugin.lua().globals().get(name).ok()?;
        match v {
            LuaValue::Integer(i) => Some(i),
            LuaValue::Number(n) => Some(n as i64),
            _ => None,
        }
    }

    /// String counterpart of `plugin_global_i64`. Used by plugin package layout
    /// tests that read order-tracking strings out of `_G`.
    pub fn plugin_global_string(&self, plugin_id: &str, name: &str) -> Option<String> {
        let plugin = self.plugins.get(plugin_id)?;
        let v: LuaValue = plugin.lua().globals().get(name).ok()?;
        match v {
            LuaValue::String(s) => s.to_str().ok().map(|c| c.to_string()),
            _ => None,
        }
    }

    /// Returns the last error from a failed `reload_plugin` for this
    /// plugin id, if any. Cleared by a subsequent successful reload.
    pub fn last_reload_error(&self, plugin_id: &str) -> Option<&str> {
        self.last_reload_errors.get(plugin_id).map(String::as_str)
    }

    /// True when the plugin currently owns a slot at
    /// `(region, container, slot_id)`. Walks `slot_ops` in reverse so a
    /// later `Remove` shadows an earlier `Add`.
    pub fn has_slot(&self, plugin_id: &str, region: &str, container: &str, slot_id: &str) -> bool {
        for op in self.slot_ops.iter().rev() {
            match op {
                PreparedSlotOp::Add(p)
                    if p.plugin_id == plugin_id
                        && p.region == region
                        && p.container.key() == container
                        && p.id == slot_id =>
                {
                    return true;
                }
                PreparedSlotOp::Replace { target, spec, .. }
                    if target.region().as_str() == region
                        && target.container().key() == container
                        && target.slot_id().as_str() == slot_id =>
                {
                    return spec.plugin_id == plugin_id;
                }
                PreparedSlotOp::Remove { target, .. }
                    if target.region().as_str() == region
                        && target.container().key() == container
                        && target.slot_id().as_str() == slot_id
                        && (target.plugin_id().as_str() == plugin_id || target.is_builtin()) =>
                {
                    return false;
                }
                _ => {}
            }
        }
        false
    }

    pub fn unload_plugin(
        &mut self,
        plugin_id: &str,
    ) -> Result<(), git_leviathan_plugin_api::error::PluginError> {
        // lazy loading: a plugin may live in the lazy registry without
        // a Lua state. Reap its stub ledger and remove the entry.
        let in_lazy = self
            .lazy_registry
            .entries()
            .iter()
            .any(|e| e.plugin_id == plugin_id);
        if !self.plugins.contains_key(plugin_id) && in_lazy {
            if let Some(ledger) = self.lazy_ledgers.remove(plugin_id) {
                self.cleanup_ledger(&ledger);
            }
            self.lazy_registry
                .entries_mut()
                .retain(|e| e.plugin_id != plugin_id);
            self.diagnostics.clear_plugin_debug(plugin_id);
            return Ok(());
        }
        let mut plugin = self.plugins.remove(plugin_id).ok_or_else(|| {
            git_leviathan_plugin_api::error::PluginError::new(
                plugin_id,
                "host.unload_plugin",
                "plugin not loaded",
            )
        })?;
        // If the plugin had been activated lazily, drop the active
        // entry too so a later `load_plugin` doesn't see a stale
        // `Active` row.
        self.lazy_registry
            .entries_mut()
            .retain(|e| e.plugin_id != plugin_id);
        plugin.generation.mark_unloading();
        // async runtime: cancel every async resource owned by this plugin's
        // current generation. Worker threads observe their cancellation
        // tokens; timer / watcher records drop immediately so no further
        // tick fires their callbacks.
        let pid_typed = plugin.generation.plugin_id.clone();
        let gen_id = plugin.generation.generation_id;
        self.async_jobs.cancel_for_generation(&pid_typed, gen_id);
        self.timers.cancel_for_generation(&pid_typed, gen_id);
        self.watchers.cancel_for_generation(&pid_typed, gen_id);
        crate::plugin::terminal::registry().close_for_generation(&pid_typed, gen_id);
        // Drop every command this plugin owned (across all
        // generations — defensive, mirroring `EventBus::drop_for_plugin`)
        // and release the Lua callback registry keys back to the
        // owning state before the state itself is dropped.
        let removed = self
            .command_registry
            .borrow_mut()
            .drop_for_plugin(plugin_id);
        commands::release_lua_keys(removed, plugin.lua());
        self.command_plugin_registry.remove(plugin_id);
        // keymaps: drop every keymap this plugin owned. The resolver
        // re-runs inside `drop_for_plugin`, so any conflict-loser
        // bindings from peers automatically reactivate.
        self.keymap_registry.borrow_mut().drop_for_plugin(plugin_id);
        let had_chrome = !plugin.chrome_widgets.borrow().is_empty();
        self.cleanup_ledger(&plugin.generation.ledger);
        // Remove ops are owned host state, but they are not slot
        // resources. Drop them explicitly so peer/built-in slots can
        // reappear when registries are replayed after this plugin unloads.
        let slot_ops_len_before = self.slot_ops.len();
        self.slot_ops.retain(|op| !op_belongs_to(op, plugin_id));
        if self.slot_ops.len() != slot_ops_len_before || had_chrome {
            self.mark_slot_ops_changed();
        }
        // extension points: drop overlays / context-menu items / decorations
        // owned by this plugin. Records came from a non-Slot ledger
        // path so the cleaner above didn't touch the registry.
        self.extension_registry.clear_for_plugin(plugin_id);
        self.dock_manager.remove_plugin(plugin_id);
        self.budget_tracker.drop_for_plugin(plugin_id);
        if self
            .active_screen
            .as_ref()
            .map(|(pid, _)| pid == plugin_id)
            .unwrap_or(false)
        {
            self.active_screen = None;
            self.widget_tree = None;
        }
        let split_prefix = format!("{plugin_id}:");
        self.split_sizes
            .retain(|key, _| !key.starts_with(&split_prefix));
        if self
            .split_drag
            .as_ref()
            .map(|drag| drag.split_key.starts_with(&split_prefix))
            .unwrap_or(false)
        {
            self.split_drag = None;
        }
        self.last_reload_errors.remove(plugin_id);
        self.runtime_path_registry.unregister(plugin_id);
        self.diagnostics.clear_plugin_debug(plugin_id);
        crate::plugin::bridge::widget_tree::clear_image_cache_for_root(&plugin.root);
        drop(plugin);
        Ok(())
    }

    /// Builds a new generation in isolation,
    /// validates it, runs the optional `M.reload(old_state)` migration
    /// hook, and only then atomically swaps it for the previous
    /// generation. Any failure between stages aborts cleanly: the
    /// previous generation stays live, a structured `reload.failed`
    /// diagnostic carries the failing stage and cause, and the staging
    /// generation is dropped in full (its ledger, lua state, after-
    /// files, services, and runtime-path entry).
    ///
    /// On success the previous generation's ledger is drained through
    /// the host's standard `cleanup_ledger` path so every resource keyed
    /// to the old `(plugin_id, generation_id)` pair is gone.
    pub fn reload_plugin(
        &mut self,
        plugin_id: &str,
    ) -> Result<(), git_leviathan_plugin_api::error::PluginError> {
        let plugin = self.plugins.get(plugin_id).ok_or_else(|| {
            git_leviathan_plugin_api::error::PluginError::new(
                plugin_id,
                "host.reload_plugin",
                "plugin not loaded",
            )
        })?;
        let plugin_id_owned = plugin_id.to_string();
        let dir = plugin.root.clone();
        let plugin_root = plugin.root.clone();
        let previous_generation_id = plugin.generation.generation_id;
        let previous_capabilities: Vec<String> = plugin
            .manifest
            .capabilities
            .iter()
            .map(|c| String::from(c.clone()))
            .collect();
        let previous_screen_ids: Vec<String> = plugin.screen_state.keys().cloned().collect();
        let auto_grant_capabilities = self.auto_grant_policy.is_auto_granted(&dir);

        let previous_screens = snapshot_previous_screens(
            plugin_id,
            plugin.lua(),
            &plugin.screens,
            &plugin.screen_state,
        );
        let live_slot_ops = self
            .slot_ops
            .iter()
            .filter(|op| !op_belongs_to(op, plugin_id))
            .cloned()
            .collect();

        let plugin_id_typed = PluginId::from(plugin_id);
        let staging_generation_id = self.allocate_generation_id(plugin_id);

        let stage_inputs = StageInputs {
            plugin_dir: dir,
            plugin_id: plugin_id_typed.clone(),
            generation_id: staging_generation_id,
            diagnostics: self.diagnostics.clone(),
            audit_log: self.audit_log.clone(),
            runtime_path_registry: &self.runtime_path_registry,
            previous_generation_id,
            previous_screens,
            previous_capabilities,
            previous_screen_ids,
            pending_tab_ops: Rc::clone(&self.pending_tab_ops),
            command_dispatch: self.command_dispatch_env(),
            keymap_registry: Rc::clone(&self.keymap_registry),
            grant_store: self.grant_store.clone(),
            auto_grant_capabilities,
            git_ctx: self.git_ops_context(),
            pending_git_events: self.pending_git_events.clone(),
            pending_ui_effects: self.pending_ui_effects.clone(),
            async_jobs: self.async_jobs.clone(),
            timers: self.timers.clone(),
            watchers: self.watchers.clone(),
            storage_roots: self.storage_roots.clone(),
            service_registry: Rc::clone(&self.service_registry),
            extension_registry: crate::plugin::extensions::ExtensionRegistry::new(),
            live_slot_ops,
        };

        match stage_reload(stage_inputs) {
            Ok(artifacts) => {
                let duration_ms = artifacts.started_at.elapsed().as_millis();
                self.commit_staging(artifacts);
                crate::plugin::bridge::widget_tree::clear_image_cache_for_root(&plugin_root);
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        plugin_id_typed.clone(),
                        DiagnosticSeverity::Info,
                        "reload.completed",
                        format!(
                            "reload completed: gen {} -> {}",
                            previous_generation_id, staging_generation_id
                        ),
                    )
                    .with_generation(staging_generation_id)
                    .with_context(serde_json::json!({
                        "duration_ms": duration_ms,
                        "previous_generation_id": previous_generation_id.get(),
                    })),
                );
                self.last_reload_errors.remove(plugin_id);
                self.record_reload_event(ReloadEventSummary {
                    plugin_id: plugin_id_owned,
                    from_generation_id: previous_generation_id.get(),
                    to_generation_id: Some(staging_generation_id.get()),
                    outcome: ReloadOutcome::Succeeded,
                    duration_ms,
                    stage_reached: ReloadStage::Swap.as_str().to_string(),
                    error_code: None,
                    error_message: None,
                    timestamp_unix_ms: now_unix_ms(),
                });
                // Staged chrome/dynamic widgets were pre-rendered with an empty
                // context during staging; render them now with the live
                // repository context (cold-load does the same via
                // `refresh_dynamic_widgets_for_plugin`). Clearing
                // `last_repository_hash` also forces the next `sync_repository`
                // to re-invalidate rather than short-circuit on the stale hash.
                self.last_repository_hash = None;
                self.refresh_dynamic_widgets_for_plugin(plugin_id);
                self.refresh_active_widget_tree();
                Ok(())
            }
            Err(failure) => {
                let StagingFailure {
                    stage,
                    cause,
                    message,
                } = failure;
                let summary_message = message.clone();
                let summary_cause = cause.clone();
                // The failed init.lua ran during staging and may have
                // registered timers/watchers/async-jobs into the shared
                // registries under `staging_generation_id`. Cancel them, or
                // watcher inotify handles leak and timer records get re-armed
                // every tick forever.
                self.async_jobs
                    .cancel_for_generation(&plugin_id_typed, staging_generation_id);
                self.timers
                    .cancel_for_generation(&plugin_id_typed, staging_generation_id);
                self.watchers
                    .cancel_for_generation(&plugin_id_typed, staging_generation_id);
                crate::plugin::terminal::registry()
                    .close_for_generation(&plugin_id_typed, staging_generation_id);
                self.last_reload_errors
                    .insert(plugin_id_owned.clone(), summary_message.clone());
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        plugin_id_typed,
                        DiagnosticSeverity::Error,
                        "reload.failed",
                        format!(
                            "reload failed at stage {}; previous generation kept active",
                            stage.as_str()
                        ),
                    )
                    .with_generation(previous_generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: "host.reload_plugin".into(),
                    })
                    .with_context(serde_json::json!({
                        "stage": stage.as_str(),
                        "cause": summary_cause,
                        "message": summary_message,
                        "staging_generation_id": staging_generation_id.get(),
                    })),
                );
                self.record_reload_event(ReloadEventSummary {
                    plugin_id: plugin_id_owned.clone(),
                    from_generation_id: previous_generation_id.get(),
                    to_generation_id: None,
                    outcome: ReloadOutcome::Failed,
                    duration_ms: 0,
                    stage_reached: stage.as_str().to_string(),
                    error_code: Some(summary_cause),
                    error_message: Some(summary_message.clone()),
                    timestamp_unix_ms: now_unix_ms(),
                });
                Err(git_leviathan_plugin_api::error::PluginError::new(
                    plugin_id,
                    "host.reload_plugin",
                    format!("reload failed at {}: {}", stage.as_str(), summary_message),
                ))
            }
        }
    }

    /// Atomic swap. Drains the previous generation's resources (slot
    /// ops, autocmds, services, runtime-path entry, ledger) and
    /// installs the staged generation in their place. Called only by
    /// `reload_plugin` on a successful staging build; never invoked
    /// when staging failed (the artefacts drop and clean up on their
    /// own).
    pub(super) fn commit_staging(&mut self, artifacts: StagingArtifacts) {
        let StagingArtifacts {
            plugin_id,
            generation_id,
            plugin_root,
            manifest,
            generation,
            slot_handlers,
            overlay_callbacks,
            dialog_callbacks,
            decoration_provider_callbacks,
            screens,
            dock_panels,
            settings_panel,
            screen_state,
            dynamic_widgets,
            ui_context,
            deferred,
            user_commands,
            health_checks,
            lua_loader,
            slot_ops: staged_slot_ops,
            autocmds: staged_autocmds,
            autocmd_groups: staged_autocmd_groups,
            autocmd_clears: staged_autocmd_clears,
            staged_services,
            started_at: _,
            commands: staged_commands,
            capability_guard: staged_capability_guard,
            keymaps: staged_keymaps,
            keymap_dels: staged_keymap_dels,
            job_callbacks: staged_job_callbacks,
            timer_callbacks: staged_timer_callbacks,
            watcher_callbacks: staged_watcher_callbacks,
            extension_registry: staged_extension_registry,
            chrome_widgets: staged_chrome_widgets,
        } = artifacts;

        let plugin_id_str = plugin_id.as_str().to_string();

        // Remove the previous generation from every host-level table
        // before splicing the new one in. RegistryKeys do not implement
        // Clone, so we drain entries by index.
        if let Some(mut previous) = self.plugins.remove(&plugin_id_str) {
            previous.generation.mark_unloading();
            // async runtime: cancel previous generation's async-runtime
            // resources so old-gen jobs/timers/watchers don't keep
            // firing against the soon-to-be-dropped Lua state.
            let prev_pid = previous.generation.plugin_id.clone();
            let prev_gen = previous.generation.generation_id;
            self.async_jobs.cancel_for_generation(&prev_pid, prev_gen);
            self.timers.cancel_for_generation(&prev_pid, prev_gen);
            self.watchers.cancel_for_generation(&prev_pid, prev_gen);
            crate::plugin::terminal::registry().close_for_generation(&prev_pid, prev_gen);
            // Drop every command this plugin owned (across
            // all prior generations) and release the Lua callback
            // keys against the *previous* state — the staged
            // generation will install fresh keys on the new state
            // below.
            let removed = self
                .command_registry
                .borrow_mut()
                .drop_for_plugin(&plugin_id_str);
            commands::release_lua_keys(removed, previous.lua());
            self.command_plugin_registry.remove(&plugin_id_str);
            // keymaps: drop every keymap the previous generation
            // owned. The staged generation's `set` rows install
            // below.
            self.keymap_registry
                .borrow_mut()
                .drop_for_plugin(&plugin_id_str);
            self.cleanup_ledger(&previous.generation.ledger);
            self.dock_manager
                .drop_registrations_for_plugin(&plugin_id_str);
            // Clear breaker state for the previous generation
            // so a fixed plugin starts clean. Earlier generations'
            // traces stay visible in devtools because we key by
            // generation id; only the previous-gen rows are dropped.
            self.budget_tracker
                .drop_for_generation(&plugin_id_str, prev_gen);
            // After cleanup_ledger ran, the ledger is empty and host
            // tables (slot_ops, event_bus, services) have already had
            // every resource keyed to the previous gen removed (the
            // ledger drives that). Drop the rest of the previous
            // plugin: dynamic widget caches, screen state keys, etc.
            drop(previous);
        }
        // Defensive sweep: remove any straggler slot_ops/autocmds for
        // this plugin id that the ledger walk did not own (e.g.
        // plugin-owned `Remove` ops, which are not ledger-tracked).
        // Safe even when the cleanup already emptied them.
        let slot_ops_len_before = self.slot_ops.len();
        self.slot_ops
            .retain(|op| !op_belongs_to(op, &plugin_id_str));
        if self.slot_ops.len() != slot_ops_len_before {
            self.mark_slot_ops_changed();
        }
        // Drop *every* prior autocmd row for this plugin id regardless
        // of generation; the ledger walk already removed the previous
        // gen's, this catches any residue.
        let _ = self.event_bus.drop_for_plugin(&plugin_id_str);
        self.extension_registry
            .replace_plugin_records_from(&plugin_id_str, &staged_extension_registry);

        // Splice staged ops into the live host tables.
        let installed_slot_ops = !staged_slot_ops.is_empty();
        for op in staged_slot_ops {
            self.slot_ops.push(op);
        }
        if installed_slot_ops {
            self.mark_slot_ops_changed();
        }
        // Splice staged autocmd group handles + clears + entries into
        // the live `EventBus` in plugin-local declaration order.
        // Group handles are minted per-plugin so a fresh generation
        // always sees a fresh handle namespace.
        let mut local_to_host: HashMap<u64, GroupId> = HashMap::new();
        let staged_walk = MergedStagedAutocmdOps::new(
            staged_autocmds,
            staged_autocmd_groups,
            staged_autocmd_clears,
        );
        for op in staged_walk {
            match op {
                StagedAutocmdOp::Group(grp) => {
                    let host_id = self
                        .event_bus
                        .group_handle(&plugin_id_str, &grp.name, grp.clear);
                    local_to_host.insert(grp.local_id, host_id);
                }
                StagedAutocmdOp::Clear(clear) => {
                    if let Some(host_id) = local_to_host.get(&clear.local_id).copied() {
                        self.event_bus.clear_group(&plugin_id_str, host_id);
                    }
                }
                StagedAutocmdOp::Create(staged) => {
                    let options = AutocmdOptions {
                        group: staged
                            .options_local_group
                            .and_then(|local| local_to_host.get(&local).copied()),
                        once: staged.once,
                        pattern: staged.pattern,
                        debounce_ms: staged.debounce_ms,
                        priority: staged.priority,
                        source_location: staged.source_location,
                    };
                    self.event_bus.register(
                        plugin_id_str.clone(),
                        generation_id,
                        staged.subscribed_event,
                        staged.canonical_event,
                        staged.callback,
                        options,
                    );
                }
            }
        }
        // `register` initialises `AutocmdRuntime::default()`, so the
        // staged autocmds always start with clean failure counters.
        // The previous generation's disabled rows were dropped by
        // `cleanup_ledger` + the defensive `drop_for_plugin` sweep
        // above, satisfying autocmd registry's "re-enabled on plugin reload"
        // requirement.

        // Transfer staged services into the live registry. Old-gen
        // service handles were removed by `cleanup_ledger` above.
        let drained = staged_reload::drain_staged_services(&staged_services, &plugin_id_str);
        self.service_registry.borrow_mut().restore_handles(drained);

        // Insert the new generation's lua + capabilities into
        // the dispatcher's plugin registry, then install the staged
        // commands. Any command rejected by `build_descriptor` lands
        // as a `command.invalid_args` diagnostic the same way it
        // would on cold load — the swap itself doesn't fail because
        // of a single bad command (mirrors the autocmd path).
        self.command_plugin_registry.insert(
            plugin_id_str.clone(),
            CommandPluginContext {
                lua: Rc::clone(&generation.lua),
                capability_guard: staged_capability_guard,
                ui_context: ui_context.clone(),
                generation_id,
            },
        );
        self.install_raw_commands(&plugin_id_str, generation_id, staged_commands);

        // keymaps: install staged keymaps. Bad rows surface as
        // `keymap.invalid_key` diagnostics — the swap itself doesn't
        // fail because of one bad binding (mirrors commands /
        // autocmds). Apply staged `del`s after the `set`s so a plugin
        // can rebind its own keymaps in one init.lua run.
        self.keymap_registry.borrow_mut().install_plugin_raws(
            &plugin_id_str,
            generation_id,
            staged_keymaps,
            &self.diagnostics,
        );
        self.keymap_registry.borrow_mut().apply_plugin_dels(
            &plugin_id_str,
            generation_id,
            staged_keymap_dels,
            &self.diagnostics,
        );

        // Now expose the new plugin's root in the runtime-path
        // registry so dependents resolve `require("<id>.module")`
        // against this generation's `lua/` directory.
        self.runtime_path_registry
            .register(plugin_id_str.clone(), plugin_root.clone());

        // If the active screen belonged to this plugin, drop it when
        // the screen no longer exists (the staged generation may have
        // removed or renamed it). Otherwise the existing
        // `(plugin_id, screen_id)` is still valid because the staged
        // gen carries its `screens` map with the same id.
        if let Some((active_pid, active_sid)) = self.active_screen.clone() {
            if active_pid == plugin_id_str && !screens.contains_key(&active_sid) {
                self.active_screen = None;
                self.widget_tree = None;
            }
        }

        let plugin = LoadedPlugin {
            generation,
            root: plugin_root,
            manifest,
            slot_handlers,
            overlay_callbacks,
            dialog_callbacks,
            decoration_provider_callbacks,
            screens,
            dock_panels,
            settings_panel,
            screen_state,
            dynamic_widgets,
            chrome_widgets: staged_chrome_widgets,
            ui_context,
            deferred,
            user_commands,
            health_checks,
            lua_loader,
            job_callbacks: staged_job_callbacks,
            timer_callbacks: staged_timer_callbacks,
            watcher_callbacks: staged_watcher_callbacks,
        };
        self.plugins.insert(plugin_id_str.clone(), plugin);
        self.register_plugin_dock_panels(&plugin_id_str);
        self.extension_registry.invalidate_decorations(
            crate::plugin::extensions::DecorationInvalidationReason::PluginState,
        );
        let _ = generation_id; // recorded on the inserted plugin's ledger
    }

    pub fn open_screen(&mut self, plugin_id: String, screen_id: String) {
        let tabs = self.last_tab_snapshot.clone();
        let focus = self.last_focus_snapshot.clone();
        let needs_init = self
            .plugins
            .get(&plugin_id)
            .map(|p| p.screens.contains_key(&screen_id) && !p.screen_state.contains_key(&screen_id))
            .unwrap_or(false);
        if needs_init {
            let chunk_name = format!("plugins/{plugin_id}/init.lua");
            let mut pending: Vec<PluginDiagnostic> = Vec::new();
            if let Some(plugin) = self.plugins.get_mut(&plugin_id) {
                if let Some(screen_def) = plugin.screens.get(&screen_id) {
                    let generation_id = plugin.generation.generation_id;
                    match plugin.lua().registry_value::<Function>(&screen_def.init) {
                        Ok(init_fn) => {
                            let ctx_lua = match screen_context_lua(plugin, &tabs, Some(&focus)) {
                                Ok(ctx) => ctx,
                                Err(e) => {
                                    pending.push(
                                        PluginDiagnostic::new(
                                            PluginId::from(plugin_id.clone()),
                                            DiagnosticSeverity::Error,
                                            "lua.value_conversion_failed",
                                            format!("screen init context conversion failed: {e}"),
                                        )
                                        .with_generation(generation_id)
                                        .with_source(
                                            PluginSourceSpan::ApiFunction {
                                                name: format!("screen:{screen_id}.init"),
                                            },
                                        ),
                                    );
                                    LuaValue::Nil
                                }
                            };
                            match init_fn.call::<LuaValue>(ctx_lua) {
                                Ok(state) => {
                                    if let Ok(key) = plugin.lua().create_registry_value(state) {
                                        record_screen_state_resource(
                                            &plugin.ledger(),
                                            &screen_id,
                                            None,
                                        );
                                        plugin.screen_state.insert(screen_id.clone(), key);
                                    }
                                }
                                Err(e) => pending.push(
                                    PluginDiagnostic::new(
                                        PluginId::from(plugin_id.clone()),
                                        DiagnosticSeverity::Error,
                                        "lua.callback_error",
                                        format!("screen init error in {screen_id}"),
                                    )
                                    .with_generation(generation_id)
                                    .with_mlua_error(&chunk_name, &e),
                                ),
                            }
                        }
                        Err(e) => pending.push(
                            PluginDiagnostic::new(
                                PluginId::from(plugin_id.clone()),
                                DiagnosticSeverity::Error,
                                "lua.handler_lookup_failed",
                                format!("screen init lookup failed: {e}"),
                            )
                            .with_generation(generation_id)
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: format!("screen:{screen_id}.init"),
                                },
                            ),
                        ),
                    }
                }
            }
            for diag in pending {
                self.diagnostics.record(diag);
            }
        }
        self.active_screen = Some((plugin_id, screen_id));
        self.refresh_active_widget_tree();
    }

    pub(super) fn refresh_active_widget_tree(&mut self) {
        let Some((plugin_id, screen_id)) = self.active_screen.clone() else {
            self.widget_tree = None;
            return;
        };
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        let mut pending: Vec<PluginDiagnostic> = Vec::new();
        let mut ok_tree: Option<WidgetAst> = None;
        let mut disable_after_failures = false;
        let tabs = self.last_tab_snapshot.clone();
        let focus = self.last_focus_snapshot.clone();
        {
            let Some(plugin) = self.plugins.get(&plugin_id) else {
                self.widget_tree = None;
                return;
            };
            let Some(screen_def) = plugin.screens.get(&screen_id) else {
                self.widget_tree = None;
                return;
            };
            let generation_id = plugin.generation.generation_id;
            let widget_path_root = format!("screen.{screen_id}.view");
            match plugin.lua().registry_value::<Function>(&screen_def.view) {
                Ok(view_fn) => {
                    let state_val: LuaValue = match plugin.screen_state.get(&screen_id) {
                        Some(k) => plugin.lua().registry_value(k).unwrap_or(LuaValue::Nil),
                        None => LuaValue::Nil,
                    };
                    let ctx_lua = match screen_context_lua(plugin, &tabs, Some(&focus)) {
                        Ok(ctx) => ctx,
                        Err(e) => {
                            pending.push(
                                PluginDiagnostic::new(
                                    PluginId::from(plugin_id.clone()),
                                    DiagnosticSeverity::Error,
                                    "lua.value_conversion_failed",
                                    format!("screen view context conversion failed: {e}"),
                                )
                                .with_generation(generation_id)
                                .with_source(
                                    PluginSourceSpan::ApiFunction {
                                        name: format!("screen:{screen_id}.view"),
                                    },
                                ),
                            );
                            LuaValue::Nil
                        }
                    };
                    let cb_id = format!("screen:{screen_id}.view");
                    let result = self.budget_tracker.track_call::<LuaValue, mlua::Error>(
                        CallbackKind::UiCallback,
                        &PluginId::from(plugin_id.clone()),
                        generation_id,
                        &cb_id,
                        || view_fn.call((state_val, ctx_lua)),
                    );
                    match result {
                        PerfOutcome::Ok(v) => {
                            let json: Result<serde_json::Value, mlua::Error> =
                                plugin.lua().from_value(v);
                            match json {
                                Ok(tree) => match widget_ast::decode(&tree) {
                                    Ok(ast) => ok_tree = Some(ast),
                                    Err(decode_err) => pending.push(widget_decode_diagnostic(
                                        &plugin_id,
                                        generation_id,
                                        &widget_path_root,
                                        &decode_err,
                                    )),
                                },
                                Err(e) => pending.push(
                                    PluginDiagnostic::new(
                                        PluginId::from(plugin_id.clone()),
                                        DiagnosticSeverity::Error,
                                        "widget.invalid_tree",
                                        format!("view returned non-serialisable tree: {e}"),
                                    )
                                    .with_generation(generation_id)
                                    .with_source(
                                        PluginSourceSpan::Widget {
                                            path: widget_path_root.clone(),
                                        },
                                    ),
                                ),
                            }
                        }
                        PerfOutcome::Skipped => {
                            // Breaker already tripped, body never ran — a
                            // transient skip, not a failure. Don't disable the
                            // plugin (consistent with dispatch_slot_click).
                        }
                        PerfOutcome::Err(e) => {
                            disable_after_failures = self.budget_tracker.is_tripped(
                                &PluginId::from(plugin_id.clone()),
                                generation_id,
                                &cb_id,
                            );
                            pending.push(
                                PluginDiagnostic::new(
                                    PluginId::from(plugin_id.clone()),
                                    DiagnosticSeverity::Error,
                                    "lua.callback_error",
                                    format!("view call failed for screen {screen_id}"),
                                )
                                .with_generation(generation_id)
                                .with_mlua_error(&chunk_name, &e),
                            );
                        }
                    }
                }
                Err(e) => pending.push(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id.clone()),
                        DiagnosticSeverity::Error,
                        "lua.handler_lookup_failed",
                        format!("view lookup failed: {e}"),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("screen:{screen_id}.view"),
                    }),
                ),
            }
        }
        for diag in pending {
            self.diagnostics.record(diag);
        }
        if disable_after_failures {
            self.disable_plugin(&plugin_id);
            self.widget_tree = None;
            return;
        }
        match ok_tree {
            Some(tree) => self.widget_tree = Some(tree),
            None => self.widget_tree = None,
        }
    }

    pub fn split_drag_begin(
        &mut self,
        split_key: &str,
        divider_index: usize,
        child_count: usize,
        is_vertical: bool,
        limits: Vec<(f32, f32)>,
    ) {
        // Sizes now tracked one per child (not n-1) so the last panel also
        // clamps via its own limits.
        let default_len = child_count.max(1);
        let sizes = self
            .split_sizes
            .get(split_key)
            .cloned()
            .unwrap_or_else(|| vec![split::DEFAULT_PANEL_SIZE; default_len]);
        self.split_drag = Some(SplitDragInfo {
            split_key: split_key.to_string(),
            divider_index,
            initial_sizes: sizes.clone(),
            initial_pointer: None,
            is_vertical,
            limits,
        });
        self.split_sizes
            .entry(split_key.to_string())
            .or_insert(sizes);
    }

    pub fn split_drag_moved(&mut self, x: f32, y: f32) {
        let (key, divider_index, initial_sizes, delta, limits) = {
            let Some(drag) = self.split_drag.as_mut() else {
                return;
            };
            let pointer_pos = if drag.is_vertical { y } else { x };
            let initial_pointer = *drag.initial_pointer.get_or_insert(pointer_pos);
            let delta = pointer_pos - initial_pointer;
            (
                drag.split_key.clone(),
                drag.divider_index,
                drag.initial_sizes.clone(),
                delta,
                drag.limits.clone(),
            )
        };
        let new_sizes = split::apply_drag_delta(&initial_sizes, divider_index, delta, &limits);
        self.split_sizes.insert(key, new_sizes);
    }

    pub fn split_drag_released(&mut self) {
        self.split_drag = None;
    }

    pub fn is_dragging_split(&self) -> bool {
        self.split_drag.is_some()
    }

    pub fn active_drag(&self) -> Option<(&str, usize)> {
        self.split_drag
            .as_ref()
            .map(|d| (d.split_key.as_str(), d.divider_index))
    }
}

fn autofocus_text_input_node_id(ast: &WidgetAst) -> Option<&str> {
    match &ast.node {
        widget_ast::WidgetNode::TextInput(node) if node.autofocus => Some(&ast.node_id.value),
        widget_ast::WidgetNode::Terminal(_) => None,
        widget_ast::WidgetNode::Button(node) => {
            node.child.as_deref().and_then(autofocus_text_input_node_id)
        }
        widget_ast::WidgetNode::Row(node) => {
            node.children.iter().find_map(autofocus_text_input_node_id)
        }
        widget_ast::WidgetNode::Column(node) => {
            node.children.iter().find_map(autofocus_text_input_node_id)
        }
        widget_ast::WidgetNode::Container(node) => {
            node.child.as_deref().and_then(autofocus_text_input_node_id)
        }
        widget_ast::WidgetNode::Padding(node) => {
            node.child.as_deref().and_then(autofocus_text_input_node_id)
        }
        widget_ast::WidgetNode::Scrollable(node) => {
            node.child.as_deref().and_then(autofocus_text_input_node_id)
        }
        widget_ast::WidgetNode::MouseArea(node) => {
            node.child.as_deref().and_then(autofocus_text_input_node_id)
        }
        widget_ast::WidgetNode::ResizableSplit(node) => {
            node.children.iter().find_map(autofocus_text_input_node_id)
        }
        widget_ast::WidgetNode::Semantic(node) => node
            .child
            .as_deref()
            .and_then(autofocus_text_input_node_id)
            .or_else(|| node.children.iter().find_map(autofocus_text_input_node_id))
            .or_else(|| {
                node.items
                    .iter()
                    .filter_map(|item| item.child.as_deref())
                    .find_map(autofocus_text_input_node_id)
            }),
        widget_ast::WidgetNode::Layout(node) => node
            .child
            .as_deref()
            .and_then(autofocus_text_input_node_id)
            .or_else(|| node.children.iter().find_map(autofocus_text_input_node_id))
            .or_else(|| {
                node.tabs
                    .iter()
                    .filter_map(|tab| tab.child.as_deref())
                    .find_map(autofocus_text_input_node_id)
            }),
        _ => None,
    }
}

impl PluginHost {
    fn apply_navigation_effects(
        &mut self,
        effects: &[crate::plugin::navigation::PluginNavigationEffect],
    ) {
        for effect in effects {
            match effect {
                crate::plugin::navigation::PluginNavigationEffect::NavigateRepository => {
                    self.active_screen = None;
                    self.widget_tree = None;
                }
                crate::plugin::navigation::PluginNavigationEffect::OpenScreen {
                    plugin_id,
                    screen_id,
                } => {
                    self.open_screen(plugin_id.clone(), screen_id.clone());
                }
            }
        }
    }
}

fn screen_context_lua(
    plugin: &LoadedPlugin,
    tabs: &TabsSnapshot,
    focus: Option<&crate::plugin::ui::focus::FocusSnapshot>,
) -> mlua::Result<LuaValue> {
    let ctx = crate::plugin::ui::context::context_for_surface(
        plugin.id(),
        plugin.generation.generation_id,
        crate::plugin::ui::context::UiContextSurface::Screen,
        None,
        tabs,
        focus,
    );
    plugin.ui_context.set(ctx.clone());
    plugin.lua().to_value(&ctx)
}

pub(super) fn parse_navigation_effects(
    plugin_id: &str,
    screen_id: &str,
    generation_id: GenerationId,
    callback: &str,
    action: &Table,
    diagnostics: &DiagnosticStore,
) -> Vec<crate::plugin::navigation::PluginNavigationEffect> {
    if matches!(action.get::<LuaValue>("navigate"), Ok(v) if !matches!(v, LuaValue::Nil)) {
        diagnostics.record(navigation_diagnostic(
            plugin_id,
            generation_id,
            callback,
            "`navigate` string returns are invalid; return effects = { { kind = ... } }",
        ));
    }

    let Ok(effects_value) = action.get::<LuaValue>("effects") else {
        return Vec::new();
    };
    let LuaValue::Table(effects_table) = effects_value else {
        if !matches!(effects_value, LuaValue::Nil) {
            diagnostics.record(navigation_diagnostic(
                plugin_id,
                generation_id,
                callback,
                "`effects` must be an array of navigation effect tables",
            ));
        }
        return Vec::new();
    };

    let mut out = Vec::new();
    for value in effects_table.sequence_values::<LuaValue>() {
        let Ok(LuaValue::Table(effect)) = value else {
            diagnostics.record(navigation_diagnostic(
                plugin_id,
                generation_id,
                callback,
                "navigation effect must be a table",
            ));
            continue;
        };
        let Ok(kind) = effect.get::<String>("kind") else {
            diagnostics.record(navigation_diagnostic(
                plugin_id,
                generation_id,
                callback,
                "navigation effect requires string field `kind`",
            ));
            continue;
        };
        match kind.as_str() {
            "navigate" => match effect.get::<Table>("target") {
                Ok(target) => match target.get::<String>("kind").ok().as_deref() {
                    Some("repository") | Some("back") => out.push(
                        crate::plugin::navigation::PluginNavigationEffect::NavigateRepository,
                    ),
                    _ => diagnostics.record(navigation_diagnostic(
                        plugin_id,
                        generation_id,
                        callback,
                        "navigate target.kind must be `repository`",
                    )),
                },
                Err(_) => diagnostics.record(navigation_diagnostic(
                    plugin_id,
                    generation_id,
                    callback,
                    "navigate effect requires target table",
                )),
            },
            "open_screen" => {
                let target_plugin = effect
                    .get::<String>("plugin")
                    .unwrap_or_else(|_| plugin_id.to_string());
                match effect.get::<String>("screen") {
                    Ok(target_screen) if !target_screen.trim().is_empty() => out.push(
                        crate::plugin::navigation::PluginNavigationEffect::OpenScreen {
                            plugin_id: target_plugin,
                            screen_id: target_screen,
                        },
                    ),
                    _ => diagnostics.record(navigation_diagnostic(
                        plugin_id,
                        generation_id,
                        callback,
                        "open_screen effect requires non-empty `screen`",
                    )),
                }
            }
            _ => diagnostics.record(navigation_diagnostic(
                plugin_id,
                generation_id,
                callback,
                format!("unknown navigation effect kind `{kind}`"),
            )),
        }
    }
    let _ = screen_id;
    out
}

fn navigation_diagnostic(
    plugin_id: &str,
    generation_id: GenerationId,
    callback: &str,
    message: impl Into<String>,
) -> PluginDiagnostic {
    PluginDiagnostic::new(
        PluginId::from(plugin_id),
        DiagnosticSeverity::Error,
        "navigation.invalid_effect",
        message.into(),
    )
    .with_generation(generation_id)
    .with_source(PluginSourceSpan::ApiFunction {
        name: callback.to_string(),
    })
}

use super::*;

impl PluginHost {
    pub(super) fn refresh_dynamic_widgets_for_plugin(&self, plugin_id: &str) {
        self.budget_tracker.begin_render_frame();
        self.refresh_dynamic_widgets_for_plugin_caused(
            plugin_id,
            &[UiInvalidationCause::InitialLoad],
        );
    }

    pub(super) fn invalidate_dynamic_widgets(
        &self,
        causes: &[UiInvalidationCause],
        only_plugins: Option<&HashSet<String>>,
    ) {
        if causes.is_empty() {
            return;
        }
        self.budget_tracker.begin_render_frame();
        match only_plugins {
            Some(set) => {
                let mut ids: Vec<&str> = set
                    .iter()
                    .filter(|id| self.plugins.contains_key(*id))
                    .map(String::as_str)
                    .collect();
                ids.sort_unstable();
                for plugin_id in ids {
                    self.refresh_dynamic_widgets_for_plugin_caused(plugin_id, causes);
                }
            }
            None => {
                let mut ids: Vec<&str> = self.plugins.keys().map(String::as_str).collect();
                ids.sort_unstable();
                for plugin_id in ids {
                    self.refresh_dynamic_widgets_for_plugin_caused(plugin_id, causes);
                }
            }
        }
    }

    pub(super) fn refresh_dynamic_widgets_for_plugin_caused(
        &self,
        plugin_id: &str,
        causes: &[UiInvalidationCause],
    ) {
        self.refresh_chrome_widgets_for_plugin_caused(plugin_id, causes);
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return;
        };
        let generation_id = plugin.generation.generation_id;
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        let repository = self.repository_context_snapshot();
        let cause_labels = crate::plugin::ui::invalidation::cause_labels(causes);
        for (address, dynamic) in &plugin.dynamic_widgets {
            if !crate::plugin::ui::invalidation::dependencies_match(&dynamic.dependencies, causes) {
                continue;
            }
            let region = address.region().as_str();
            let pid = PluginId::from(plugin_id);
            let cb_id = format!("dynamic_widget:{address}");
            if let Some(skip) = self
                .budget_tracker
                .render_budget_skip(&pid, generation_id, region)
            {
                let reason = skip.reason();
                {
                    let mut telemetry = dynamic.telemetry.borrow_mut();
                    telemetry.skipped_count = telemetry.skipped_count.saturating_add(1);
                    telemetry.last_causes = cause_labels.clone();
                    telemetry.last_status = "skipped".to_string();
                    telemetry.last_error = Some(reason.clone());
                    telemetry.diagnostic_badge = true;
                }
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        pid,
                        DiagnosticSeverity::Warning,
                        "performance.render_budget_skipped",
                        reason,
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::Widget {
                        path: format!("slot:{address}"),
                    }),
                );
                continue;
            }
            let func: Function = match plugin.lua().registry_value(&dynamic.key) {
                Ok(f) => f,
                Err(e) => {
                    record_dynamic_error(
                        dynamic,
                        &cause_labels,
                        format!("handler lookup failed: {e}"),
                    );
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("dynamic widget fn lookup failed for {address}: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("dynamic_widget:{address}"),
                        }),
                    );
                    continue;
                }
            };
            let ctx = crate::plugin::ui::context::context_for_slot_with_selection(
                plugin_id,
                generation_id,
                address,
                repository.as_ref(),
                Some(&self.last_selection_snapshot),
                &self.last_tab_snapshot,
                Some(&self.last_focus_snapshot),
            );
            plugin.ui_context.set(ctx.clone());
            let started = std::time::Instant::now();
            let perf_outcome = self.budget_tracker.track_call::<LuaValue, mlua::Error>(
                CallbackKind::UiCallback,
                &pid,
                generation_id,
                &cb_id,
                || {
                    let arg = plugin.lua().to_value(&ctx)?;
                    func.call(arg)
                },
            );
            let duration_ms = started.elapsed().as_millis();
            self.budget_tracker
                .record_render_frame_cost(&pid, generation_id, region, duration_ms);
            let lua_val: LuaValue = match perf_outcome {
                PerfOutcome::Ok(v) => v,
                PerfOutcome::Skipped => {
                    record_dynamic_skip(dynamic, &cause_labels, "callback circuit breaker tripped");
                    continue;
                }
                PerfOutcome::Err(e) => {
                    record_dynamic_error(dynamic, &cause_labels, e.to_string());
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            format!("dynamic widget fn error for {address}"),
                        )
                        .with_generation(generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                    continue;
                }
            };
            let json: serde_json::Value = match plugin.lua().from_value(lua_val) {
                Ok(v) => v,
                Err(e) => {
                    record_dynamic_error(dynamic, &cause_labels, e.to_string());
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "widget.invalid_tree",
                            format!("dynamic widget returned non-serialisable value: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::Widget {
                            path: format!("slot:{address}"),
                        }),
                    );
                    continue;
                }
            };
            match widget_ast::decode(&json) {
                Ok(ast) => {
                    *dynamic.cache.borrow_mut() = Some(ast);
                    let mut telemetry = dynamic.telemetry.borrow_mut();
                    telemetry.refresh_count = telemetry.refresh_count.saturating_add(1);
                    telemetry.last_duration_ms = duration_ms;
                    telemetry.last_causes = cause_labels.clone();
                    telemetry.last_status = "ok".to_string();
                    telemetry.last_error = None;
                    telemetry.diagnostic_badge = false;
                }
                Err(decode_err) => {
                    record_dynamic_error(dynamic, &cause_labels, decode_err.message.clone());
                    self.diagnostics.record(widget_decode_diagnostic(
                        plugin_id,
                        generation_id,
                        &format!("slot:{address}"),
                        &decode_err,
                    ));
                }
            }
        }
    }

    pub(super) fn refresh_chrome_widgets_for_plugin_caused(
        &self,
        plugin_id: &str,
        causes: &[UiInvalidationCause],
    ) {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return;
        };
        let generation_id = plugin.generation.generation_id;
        let chunk_name = format!("plugins/{plugin_id}/init.lua");
        let repository = self.repository_context_snapshot();
        let chrome_widgets = Rc::clone(&plugin.chrome_widgets);
        let chrome_widgets = chrome_widgets.borrow();
        let cause_labels = crate::plugin::ui::invalidation::cause_labels(causes);
        for (handle, chrome) in chrome_widgets.iter() {
            if !crate::plugin::ui::invalidation::dependencies_match(&chrome.dependencies, causes) {
                continue;
            }
            let pid = PluginId::from(plugin_id);
            let cb_id = format!("chrome:{handle}");
            let func: Function = match plugin.lua().registry_value(&chrome.key) {
                Ok(f) => f,
                Err(e) => {
                    record_chrome_error(
                        chrome,
                        &cause_labels,
                        format!("handler lookup failed: {e}"),
                    );
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            pid,
                            DiagnosticSeverity::Error,
                            "lua.handler_lookup_failed",
                            format!("chrome widget fn lookup failed for {handle}: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("chrome:{handle}"),
                        }),
                    );
                    continue;
                }
            };
            let surface = chrome_pane_surface(chrome.pane);
            let ctx = crate::plugin::ui::context::context_for_surface_with_selection(
                plugin_id,
                generation_id,
                surface,
                repository.as_ref(),
                Some(&self.last_selection_snapshot),
                &self.last_tab_snapshot,
                Some(&self.last_focus_snapshot),
            );
            plugin.ui_context.set(ctx.clone());
            let started = std::time::Instant::now();
            let perf_outcome = self.budget_tracker.track_call::<LuaValue, mlua::Error>(
                CallbackKind::UiCallback,
                &pid,
                generation_id,
                &cb_id,
                || {
                    let arg = plugin.lua().to_value(&ctx)?;
                    func.call(arg)
                },
            );
            let duration_ms = started.elapsed().as_millis();
            let lua_val: LuaValue = match perf_outcome {
                PerfOutcome::Ok(v) => v,
                PerfOutcome::Skipped => {
                    let mut telemetry = chrome.telemetry.borrow_mut();
                    telemetry.skipped_count = telemetry.skipped_count.saturating_add(1);
                    telemetry.last_causes = cause_labels.clone();
                    telemetry.last_status = "skipped".to_string();
                    telemetry.last_error = Some("callback circuit breaker tripped".to_string());
                    telemetry.diagnostic_badge = true;
                    continue;
                }
                PerfOutcome::Err(e) => {
                    record_chrome_error(chrome, &cause_labels, e.to_string());
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            format!("chrome widget fn error for {handle}"),
                        )
                        .with_generation(generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                    continue;
                }
            };
            if matches!(lua_val, LuaValue::Nil | LuaValue::Boolean(false)) {
                *chrome.cache.borrow_mut() = None;
                let mut telemetry = chrome.telemetry.borrow_mut();
                telemetry.refresh_count = telemetry.refresh_count.saturating_add(1);
                telemetry.last_duration_ms = duration_ms;
                telemetry.last_causes = cause_labels.clone();
                telemetry.last_status = "ok".to_string();
                telemetry.last_error = None;
                telemetry.diagnostic_badge = false;
                continue;
            }
            let json: serde_json::Value = match plugin.lua().from_value(lua_val) {
                Ok(v) => v,
                Err(e) => {
                    record_chrome_error(chrome, &cause_labels, e.to_string());
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id),
                            DiagnosticSeverity::Error,
                            "widget.invalid_tree",
                            format!("chrome widget returned non-serialisable value: {e}"),
                        )
                        .with_generation(generation_id)
                        .with_source(PluginSourceSpan::Widget {
                            path: format!("chrome:{handle}"),
                        }),
                    );
                    continue;
                }
            };
            match widget_ast::decode(&json) {
                Ok(ast) => {
                    *chrome.cache.borrow_mut() = Some(ast);
                    let mut telemetry = chrome.telemetry.borrow_mut();
                    telemetry.refresh_count = telemetry.refresh_count.saturating_add(1);
                    telemetry.last_duration_ms = duration_ms;
                    telemetry.last_causes = cause_labels.clone();
                    telemetry.last_status = "ok".to_string();
                    telemetry.last_error = None;
                    telemetry.diagnostic_badge = false;
                }
                Err(decode_err) => {
                    record_chrome_error(chrome, &cause_labels, decode_err.message.clone());
                    self.diagnostics.record(widget_decode_diagnostic(
                        plugin_id,
                        generation_id,
                        &format!("chrome:{handle}"),
                        &decode_err,
                    ));
                }
            }
        }
    }
}

fn chrome_pane_surface(
    pane: crate::widgets::chrome::repo_region::ChromePane,
) -> crate::plugin::ui::context::UiContextSurface {
    use crate::plugin::ui::context::UiContextSurface;
    use crate::widgets::chrome::repo_region::ChromePane;
    match pane {
        ChromePane::Sidebar => UiContextSurface::RepositorySidebar {
            section: "chrome".into(),
        },
        ChromePane::Graph => UiContextSurface::RepositoryGraph {
            section: "chrome".into(),
        },
        ChromePane::Details => UiContextSurface::RepositoryDetails {
            section: "chrome".into(),
        },
        ChromePane::Diff => UiContextSurface::RepositoryDiff,
    }
}

fn record_dynamic_error(
    dynamic: &crate::plugin::ui::main_bar_slots::DynamicWidgetRegistration,
    causes: &[String],
    error: String,
) {
    let mut telemetry = dynamic.telemetry.borrow_mut();
    telemetry.last_causes = causes.to_vec();
    telemetry.last_status = if dynamic.cache.borrow().is_some() {
        telemetry.stale_count = telemetry.stale_count.saturating_add(1);
        "stale".to_string()
    } else {
        "error".to_string()
    };
    telemetry.last_error = Some(error);
    telemetry.diagnostic_badge = true;
}

fn record_dynamic_skip(
    dynamic: &crate::plugin::ui::main_bar_slots::DynamicWidgetRegistration,
    causes: &[String],
    reason: &str,
) {
    let mut telemetry = dynamic.telemetry.borrow_mut();
    telemetry.skipped_count = telemetry.skipped_count.saturating_add(1);
    telemetry.last_causes = causes.to_vec();
    telemetry.last_status = "skipped".to_string();
    telemetry.last_error = Some(reason.to_string());
    telemetry.diagnostic_badge = true;
}

fn record_chrome_error(
    chrome: &crate::plugin::ui::chrome::ChromeRegistration,
    causes: &[String],
    error: String,
) {
    let mut telemetry = chrome.telemetry.borrow_mut();
    telemetry.last_causes = causes.to_vec();
    telemetry.last_status = if chrome.cache.borrow().is_some() {
        telemetry.stale_count = telemetry.stale_count.saturating_add(1);
        "stale".to_string()
    } else {
        "error".to_string()
    };
    telemetry.last_error = Some(error);
    telemetry.diagnostic_badge = true;
}

pub(super) fn widget_decode_diagnostic(
    plugin_id: &str,
    generation_id: GenerationId,
    path_root: &str,
    err: &widget_ast::WidgetDecodeError,
) -> PluginDiagnostic {
    // The decoder paths are rooted at "root"; strip that and re-root
    // under the host-side path so the diagnostic carries an absolute
    // location.
    let suffix = err.path.strip_prefix("root").unwrap_or(err.path.as_str());
    let full_path = if suffix.is_empty() {
        path_root.to_string()
    } else {
        format!("{path_root}{suffix}")
    };
    PluginDiagnostic::new(
        PluginId::from(plugin_id),
        DiagnosticSeverity::Error,
        err.code,
        err.message.clone(),
    )
    .with_generation(generation_id)
    .with_source(PluginSourceSpan::Widget { path: full_path })
}

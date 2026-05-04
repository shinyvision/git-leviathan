use super::*;

impl PluginHost {
    pub(super) fn register_plugin_dock_panels(&self, plugin_id: &str) {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return;
        };
        for panel in plugin.dock_panels.values() {
            self.dock_manager
                .register(crate::plugin::dock::DockPanelRegistration {
                    plugin_id: plugin_id.to_string(),
                    generation_id: plugin.generation.generation_id.get(),
                    id: panel.id.clone(),
                    title: panel.title.clone(),
                    area: panel.area,
                    default_open: panel.default_open,
                    source_location: panel.source_location.clone(),
                });
        }
    }

    pub fn dock_panels(&self) -> Vec<crate::plugin::dock::DockPanelSummary> {
        self.dock_manager.summaries()
    }

    pub fn dock_layout_json(&self) -> serde_json::Value {
        self.dock_manager.layout_json()
    }

    pub fn open_dock_panel(&self, plugin_id: &str, id: &str) -> Result<(), String> {
        self.dock_manager.open_panel(plugin_id, id)
    }

    pub fn close_dock_panel(&self, plugin_id: &str, id: &str) -> Result<(), String> {
        self.dock_manager.close_panel(plugin_id, id)
    }

    pub fn move_dock_panel(
        &self,
        plugin_id: &str,
        id: &str,
        area: &str,
        order: Option<usize>,
    ) -> Result<(), String> {
        self.dock_manager.move_panel(
            plugin_id,
            id,
            crate::plugin::dock::DockArea::parse(area)?,
            order,
        )
    }

    pub fn reset_dock_layout(&self) {
        self.dock_manager.reset_layout();
    }

    pub fn render_dock_panel(
        &self,
        plugin_id: &str,
        id: &str,
    ) -> Option<crate::plugin::ui::widget_ast::WidgetAst> {
        let plugin = self.plugins.get(plugin_id)?;
        let panel = plugin.dock_panels.get(id)?;
        let func: Function = match plugin.lua().registry_value(&panel.view) {
            Ok(func) => func,
            Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "lua.handler_lookup_failed",
                        format!("dock panel view lookup failed for {id}: {e}"),
                    )
                    .with_generation(plugin.generation.generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("dock:{id}:view"),
                    }),
                );
                return None;
            }
        };
        let ctx = self.dock_context(plugin_id, id)?;
        plugin.ui_context.set(ctx.clone());
        let pid = PluginId::from(plugin_id);
        let generation_id = plugin.generation.generation_id;
        let callback_id = format!("dock:{id}:view");
        let lua_val = match self.budget_tracker.track_call::<LuaValue, mlua::Error>(
            CallbackKind::UiCallback,
            &pid,
            generation_id,
            &callback_id,
            || {
                let arg = plugin.lua().to_value(&ctx)?;
                func.call(arg)
            },
        ) {
            PerfOutcome::Ok(value) => value,
            PerfOutcome::Skipped => return None,
            PerfOutcome::Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "lua.callback_error",
                        format!("dock panel view error for {id}"),
                    )
                    .with_generation(generation_id)
                    .with_mlua_error(&format!("plugins/{plugin_id}/init.lua"), &e),
                );
                return None;
            }
        };
        let json: serde_json::Value = match plugin.lua().from_value(lua_val) {
            Ok(value) => value,
            Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "widget.invalid_tree",
                        format!("dock panel returned non-serialisable value: {e}"),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::Widget {
                        path: format!("dock:{id}"),
                    }),
                );
                return None;
            }
        };
        match widget_ast::decode(&json) {
            Ok(ast) => Some(ast),
            Err(err) => {
                self.diagnostics.record(widget_decode_diagnostic(
                    plugin_id,
                    generation_id,
                    &format!("dock:{id}"),
                    &err,
                ));
                None
            }
        }
    }

    pub fn dispatch_dock_event(
        &self,
        plugin_id: &str,
        id: &str,
        event: &str,
        value: serde_json::Value,
    ) {
        let Some(plugin) = self.plugins.get(plugin_id) else {
            return;
        };
        let Some(panel) = plugin.dock_panels.get(id) else {
            return;
        };
        let Some(update_key) = panel.update.as_ref() else {
            return;
        };
        let func: Function = match plugin.lua().registry_value(update_key) {
            Ok(func) => func,
            Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "lua.handler_lookup_failed",
                        format!("dock panel update lookup failed for {id}: {e}"),
                    )
                    .with_generation(plugin.generation.generation_id)
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("dock:{id}:update"),
                    }),
                );
                return;
            }
        };
        let state = self.dock_manager.state(plugin_id, id);
        let state_lua = match plugin.lua().to_value(&state) {
            Ok(value) => value,
            Err(e) => {
                self.record_dock_value_error(plugin_id, id, e);
                return;
            }
        };
        let event_value = match plugin.lua().to_value(&value) {
            Ok(value) => value,
            Err(e) => {
                self.record_dock_value_error(plugin_id, id, e);
                return;
            }
        };
        let pid = PluginId::from(plugin_id);
        let generation_id = plugin.generation.generation_id;
        let callback_id = format!("dock:{id}:update");
        let result = self.budget_tracker.track_call::<LuaValue, mlua::Error>(
            CallbackKind::UiCallback,
            &pid,
            generation_id,
            &callback_id,
            || func.call((state_lua, event.to_string(), event_value)),
        );
        let lua_val = match result {
            PerfOutcome::Ok(value) => value,
            PerfOutcome::Skipped => return,
            PerfOutcome::Err(e) => {
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(plugin_id),
                        DiagnosticSeverity::Error,
                        "lua.callback_error",
                        format!("dock panel update error for {id}"),
                    )
                    .with_generation(generation_id)
                    .with_mlua_error(&format!("plugins/{plugin_id}/init.lua"), &e),
                );
                return;
            }
        };
        let json: serde_json::Value = match plugin.lua().from_value(lua_val) {
            Ok(value) => value,
            Err(e) => {
                self.record_dock_value_error(plugin_id, id, e);
                return;
            }
        };
        if let Some(state) = json.get("state").cloned() {
            self.dock_manager.set_state(plugin_id, id, state);
        }
    }

    fn dock_context(&self, plugin_id: &str, id: &str) -> Option<serde_json::Value> {
        let panel = self
            .dock_manager
            .summaries()
            .into_iter()
            .find(|panel| panel.plugin_id == plugin_id && panel.id == id)?;
        Some(crate::plugin::ui::context::context_for_dock_panel(
            plugin_id,
            GenerationId::new(panel.generation_id),
            &panel,
            self.repository_context_snapshot().as_ref(),
            &self.last_tab_snapshot,
        ))
    }

    fn record_dock_value_error(&self, plugin_id: &str, id: &str, error: mlua::Error) {
        self.diagnostics.record(
            PluginDiagnostic::new(
                PluginId::from(plugin_id),
                DiagnosticSeverity::Error,
                "lua.value_conversion_failed",
                format!("dock panel value conversion failed for {id}: {error}"),
            )
            .with_source(PluginSourceSpan::ApiFunction {
                name: format!("dock:{id}"),
            }),
        );
    }
}

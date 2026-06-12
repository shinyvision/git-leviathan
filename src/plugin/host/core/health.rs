use super::*;

impl PluginHost {
    /// Run every plugin's registered health checks and return an aggregated
    /// report. Plugins that didn't register a check (or whose checks
    /// produced no items) are absent from the report. Errors from
    /// individual callbacks are logged; partial item lists are kept.
    pub fn run_health_checks(&self) -> HealthReport {
        let mut report = HealthReport::default();
        for (plugin_id, plugin) in &self.plugins {
            let generation_id = plugin.generation.generation_id;
            let chunk_name = format!("plugins/{plugin_id}/init.lua");
            let mut items: Vec<HealthItem> = Vec::new();
            for check in &plugin.health_checks {
                let func: Function = match plugin.lua().registry_value(&check.callback) {
                    Ok(f) => f,
                    Err(e) => {
                        self.diagnostics.record(
                            PluginDiagnostic::new(
                                PluginId::from(plugin_id.clone()),
                                DiagnosticSeverity::Error,
                                "lua.handler_lookup_failed",
                                format!("health callback lookup failed: {e}"),
                            )
                            .with_generation(generation_id)
                            .with_source(
                                PluginSourceSpan::ApiFunction {
                                    name: "leviathan.health.register".into(),
                                },
                            ),
                        );
                        continue;
                    }
                };
                let bucket: Rc<RefCell<Vec<HealthItem>>> = Rc::new(RefCell::new(Vec::new()));
                let ctx = HealthContext {
                    items: Rc::clone(&bucket),
                };
                if let Err(e) = func.call::<()>(ctx) {
                    self.diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from(plugin_id.clone()),
                            DiagnosticSeverity::Error,
                            "lua.callback_error",
                            "health callback error".to_string(),
                        )
                        .with_generation(generation_id)
                        .with_mlua_error(&chunk_name, &e),
                    );
                }
                items.extend(bucket.borrow().iter().cloned());
            }
            if !items.is_empty() {
                report.plugins.push(PluginHealth {
                    plugin_id: plugin_id.clone(),
                    items,
                });
            }
        }
        report
    }
}

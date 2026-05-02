//! Devtools introspection. `PluginHost::introspect()` returns a
//! point-in-time snapshot of the host's state for in-app inspectors,
//! tests, and external tools.

use crate::plugin::audit::AuditEntry;

#[derive(Debug, Clone)]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub last_reload_error: Option<String>,
    pub provides_services: Vec<String>,
    pub consumes_services: Vec<String>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SlotSummary {
    pub region: String,
    pub container: String,
    pub id: String,
    pub priority: i32,
    pub owner_plugin_id: String,
}

#[derive(Debug, Clone)]
pub struct ServiceSummary {
    pub key: String,
    pub publisher_plugin_id: String,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct InspectorSnapshot {
    pub plugins: Vec<PluginSummary>,
    pub slots: Vec<SlotSummary>,
    pub services: Vec<ServiceSummary>,
    pub audit_recent: Vec<AuditEntry>,
}

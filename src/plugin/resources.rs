use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use mlua::Lua;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PluginId(String);

impl PluginId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for PluginId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for PluginId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GenerationId(u64);

impl GenerationId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for GenerationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceId(u64);

impl ResourceId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PluginResourceKind {
    LuaRegistryKey,
    Slot,
    Screen,
    Overlay,
    Command,
    Keymap,
    Autocmd,
    Timer,
    AsyncJob,
    FileWatcher,
    ServiceRegistration,
    DynamicWidgetCache,
    PersistedScreenState,
    HealthCheck,
    /// lazy loading: a placeholder registration the host installs on
    /// behalf of a lazy plugin (a stub command, keymap, event
    /// subscription, region slot, or file-presence watcher) so the
    /// plugin can be activated lazily on first trigger. The
    /// activation resolver owns the lifecycle; the ledger ensures
    /// stubs disappear on unload, just like real registrations.
    ActivationStub,
    /// extension points: one item contributed at a `*.context_menu`
    /// extension point.
    ContextMenuItem,
    /// extension points: one decoration attached to a commit row in the
    /// graph view.
    GraphDecoration,
    /// extension points: one decoration attached to a diff line / hunk.
    DiffDecoration,
    AssetHandle,
    DockPanel,
}

impl PluginResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LuaRegistryKey => "lua_registry_key",
            Self::Slot => "slot",
            Self::Screen => "screen",
            Self::Overlay => "overlay_placeholder",
            Self::Command => "command",
            Self::Keymap => "keymap_placeholder",
            Self::Autocmd => "autocmd",
            Self::Timer => "timer_placeholder",
            Self::AsyncJob => "async_job_placeholder",
            Self::FileWatcher => "file_watcher_placeholder",
            Self::ServiceRegistration => "service_registration",
            Self::DynamicWidgetCache => "dynamic_widget_cache",
            Self::PersistedScreenState => "persisted_screen_state",
            Self::HealthCheck => "health_check",
            Self::ActivationStub => "activation_stub",
            Self::ContextMenuItem => "context_menu_item",
            Self::GraphDecoration => "graph_decoration",
            Self::DiffDecoration => "diff_decoration",
            Self::AssetHandle => "asset_handle",
            Self::DockPanel => "dock_panel",
        }
    }

    pub fn all_known() -> &'static [PluginResourceKind] {
        &[
            Self::LuaRegistryKey,
            Self::Slot,
            Self::Screen,
            Self::Overlay,
            Self::Command,
            Self::Keymap,
            Self::Autocmd,
            Self::Timer,
            Self::AsyncJob,
            Self::FileWatcher,
            Self::ServiceRegistration,
            Self::DynamicWidgetCache,
            Self::PersistedScreenState,
            Self::HealthCheck,
            Self::ActivationStub,
            Self::ContextMenuItem,
            Self::GraphDecoration,
            Self::DiffDecoration,
            Self::AssetHandle,
            Self::DockPanel,
        ]
    }
}

impl fmt::Display for PluginResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct ResourceRecord {
    pub resource_id: ResourceId,
    pub plugin_id: PluginId,
    pub generation_id: GenerationId,
    pub kind: PluginResourceKind,
    pub handle: String,
    pub source_location: Option<String>,
    pub created_at: SystemTime,
}

impl ResourceRecord {
    pub fn created_at_unix_ms(&self) -> u128 {
        self.created_at
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0)
    }
}

#[derive(Debug)]
struct ResourceLedgerInner {
    plugin_id: PluginId,
    generation_id: GenerationId,
    next_resource_id: u64,
    records: Vec<ResourceRecord>,
}

#[derive(Debug, Clone)]
pub struct ResourceLedger {
    inner: Rc<RefCell<ResourceLedgerInner>>,
}

impl ResourceLedger {
    pub fn new(plugin_id: PluginId, generation_id: GenerationId) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ResourceLedgerInner {
                plugin_id,
                generation_id,
                next_resource_id: 1,
                records: Vec::new(),
            })),
        }
    }

    pub fn plugin_id(&self) -> PluginId {
        self.inner.borrow().plugin_id.clone()
    }

    pub fn generation_id(&self) -> GenerationId {
        self.inner.borrow().generation_id
    }

    pub fn record(
        &self,
        kind: PluginResourceKind,
        handle: impl Into<String>,
        source_location: Option<String>,
    ) -> ResourceId {
        let mut inner = self.inner.borrow_mut();
        let resource_id = ResourceId::new(inner.next_resource_id);
        inner.next_resource_id += 1;
        let record = ResourceRecord {
            resource_id,
            plugin_id: inner.plugin_id.clone(),
            generation_id: inner.generation_id,
            kind,
            handle: handle.into(),
            source_location,
            created_at: SystemTime::now(),
        };
        inner.records.push(record);
        resource_id
    }

    pub fn remove_resource(&self, resource_id: ResourceId) {
        self.inner
            .borrow_mut()
            .records
            .retain(|record| record.resource_id != resource_id);
    }

    pub fn remove_by_kind_handle(&self, kind: PluginResourceKind, handle: &str) {
        self.inner
            .borrow_mut()
            .records
            .retain(|record| !(record.kind == kind && record.handle == handle));
    }

    pub fn contains_kind_handle(&self, kind: PluginResourceKind, handle: &str) -> bool {
        self.inner
            .borrow()
            .records
            .iter()
            .any(|record| record.kind == kind && record.handle == handle)
    }

    pub fn remove_by_handle_prefix(&self, prefix: &str) {
        self.inner
            .borrow_mut()
            .records
            .retain(|record| !record.handle.starts_with(prefix));
    }

    pub fn len(&self) -> usize {
        self.inner.borrow().records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn records(&self) -> Vec<ResourceRecord> {
        self.inner.borrow().records.clone()
    }

    pub fn cleanup_all(&self, cleaner: &mut impl ResourceCleaner) -> ResourceCleanupReport {
        let mut report = ResourceCleanupReport::default();
        let records = std::mem::take(&mut self.inner.borrow_mut().records);
        for record in records.into_iter().rev() {
            if let Err(error) = cleaner.cleanup_resource(&record) {
                report.errors.push(ResourceCleanupError {
                    resource: record,
                    error,
                });
            } else {
                report.cleaned += 1;
            }
        }
        report
    }

    pub fn source_location(lua: &Lua) -> Option<String> {
        let debug = lua.inspect_stack(1)?;
        let source = debug.source();
        let short_src = source.short_src.as_deref().or(source.source.as_deref())?;
        let line = debug.curr_line();
        if line > 0 {
            Some(format!("{short_src}:{line}"))
        } else {
            Some(short_src.to_string())
        }
    }
}

pub trait ResourceCleaner {
    fn cleanup_resource(&mut self, resource: &ResourceRecord) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct ResourceCleanupReport {
    pub cleaned: usize,
    pub errors: Vec<ResourceCleanupError>,
}

impl ResourceCleanupReport {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

#[derive(Debug)]
pub struct ResourceCleanupError {
    pub resource: ResourceRecord,
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingCleaner;

    impl ResourceCleaner for FailingCleaner {
        fn cleanup_resource(&mut self, resource: &ResourceRecord) -> Result<(), String> {
            if resource.kind == PluginResourceKind::ServiceRegistration {
                Err("simulated cleanup failure".to_string())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn cleanup_drains_records_even_when_a_cleaner_fails() {
        let ledger = ResourceLedger::new(PluginId::from("p"), GenerationId::new(1));
        assert_eq!(ledger.plugin_id().as_str(), "p");
        assert_eq!(ledger.generation_id().get(), 1);
        ledger.record(PluginResourceKind::Slot, "main_bar:left:x", None);
        ledger.record(PluginResourceKind::ServiceRegistration, "math@1", None);

        let report = ledger.cleanup_all(&mut FailingCleaner);

        assert_eq!(report.cleaned, 1);
        assert_eq!(report.errors.len(), 1);
        assert!(ledger.is_empty());
    }

    #[test]
    fn known_kinds_include_resource_lifecycle_placeholders() {
        let kinds = PluginResourceKind::all_known();
        assert!(kinds.contains(&PluginResourceKind::Overlay));
        assert!(kinds.contains(&PluginResourceKind::Keymap));
        assert!(kinds.contains(&PluginResourceKind::Timer));
        assert!(kinds.contains(&PluginResourceKind::AsyncJob));
        assert!(kinds.contains(&PluginResourceKind::FileWatcher));
    }
}

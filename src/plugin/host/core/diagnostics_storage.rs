use super::*;

impl PluginHost {
    /// Replace the diagnostic store. Tests pass a store wired to
    /// `NullSink` so test output stays quiet while every emission path
    /// still records a real diagnostic. The performance budgets tracker is
    /// rebuilt against the new store so its breach / trip diagnostics
    /// land in the same buffer the rest of the host uses.
    pub fn set_diagnostic_store(&mut self, store: DiagnosticStore) {
        self.diagnostics = store.clone();
        self.budget_tracker = BudgetTracker::new(store);
    }

    /// Cheap-clone handle to the budget tracker. Tests use
    /// this to drive the breaker (`reset_breaker`) and project
    /// summaries directly. Production code goes through
    /// [`Self::reset_breaker`].
    pub fn budget_tracker(&self) -> BudgetTracker {
        self.budget_tracker.clone()
    }

    /// Replace the budget tracker. performance budget tests use this to inject
    /// a `MockClock`-backed tracker so timing assertions stay
    /// deterministic.
    pub fn set_budget_tracker(&mut self, tracker: BudgetTracker) {
        self.budget_tracker = tracker;
    }

    /// User-facing breaker reset. Walks every generation
    /// under `plugin_id` and emits a `performance.reset` info
    /// diagnostic.
    pub fn reset_breaker(&self, plugin_id: &str, callback_id: &str) {
        self.budget_tracker
            .reset_breaker(&PluginId::from(plugin_id), callback_id);
    }

    /// Query whether a callback's circuit breaker is
    /// tripped. The app-level startup pass calls this with sentinel
    /// values to keep the lookup path warm; tests use it through
    /// `MockHost::budget_tracker()` for assertions.
    pub fn is_breaker_tripped(
        &self,
        plugin_id: &str,
        generation_id: u64,
        callback_id: &str,
    ) -> bool {
        self.budget_tracker.is_tripped(
            &PluginId::from(plugin_id),
            GenerationId::new(generation_id),
            callback_id,
        )
    }

    /// Drop every breaker / trace row for `plugin_id`.
    /// Called from `unload_plugin`; the startup pass uses a sentinel
    /// id so production builds always have a live entry into this
    /// path.
    pub fn drop_breaker_state_for_plugin(&self, plugin_id: &str) {
        self.budget_tracker.drop_for_plugin(plugin_id);
    }

    /// Drop breaker / trace rows tied to one specific
    /// generation. Called from the reload-cleanup walk so the new
    /// generation starts with a clean slate.
    pub fn drop_breaker_state_for_generation(&self, plugin_id: &str, generation_id: u64) {
        self.budget_tracker
            .drop_for_generation(plugin_id, GenerationId::new(generation_id));
    }

    /// Cheap-clone handle to the host diagnostic store. Read by the
    /// devtools panel and tests.
    pub fn diagnostics(&self) -> DiagnosticStore {
        self.diagnostics.clone()
    }

    pub fn set_plugin_storage_base(&mut self, base: impl AsRef<Path>) {
        self.storage_roots = PluginStorageRoots::under_base(base);
    }

    /// Test-only accessor that exposes the per-plugin storage paths
    /// so harness code can seed / inspect a plugin's surfaces. Same
    /// path layout the production code uses; cheap to call.
    pub fn storage_paths_for_test(
        &self,
        plugin_id: &str,
        plugin_root: &Path,
    ) -> PluginStoragePaths {
        self.storage_paths(plugin_id, plugin_root.to_path_buf())
    }

    pub(super) fn storage_paths(
        &self,
        plugin_id: &str,
        plugin_root: impl Into<PathBuf>,
    ) -> PluginStoragePaths {
        self.storage_roots.for_plugin(plugin_id, plugin_root)
    }

    pub fn reset_plugin_storage(&mut self, plugin_id: &str, surface: &str) -> Result<(), String> {
        let plugin = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| format!("plugin `{plugin_id}` not loaded"))?;
        let surface = StorageSurface::parse(surface)?;
        let storage = self.storage_paths(plugin_id, plugin.root.clone());
        let path = match surface {
            StorageSurface::Settings => storage.settings_path(),
            _ => storage.surface_dir(surface),
        };
        if !path.exists() {
            return Ok(());
        }
        if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|e| e.to_string())
        } else {
            std::fs::remove_file(&path).map_err(|e| e.to_string())
        }
    }

    pub(super) fn allocate_generation_id(&mut self, plugin_id: &str) -> GenerationId {
        let next = self
            .next_generation_ids
            .entry(plugin_id.to_string())
            .or_insert(1);
        let id = *next;
        *next += 1;
        GenerationId::new(id)
    }

    pub(super) fn validate_service_dependencies(
        &self,
        manifest: &PluginManifest,
        generation_id: GenerationId,
    ) -> Result<(), PluginLoadError> {
        let statuses = {
            let registry = self.service_registry.borrow();
            dependency_statuses(
                &manifest.id,
                &manifest.provides_services,
                &manifest.consumes_services,
                &registry,
            )
        };
        for status in statuses {
            if status.satisfied {
                continue;
            }
            if status.required {
                let message = format!(
                    "required service `{}` is not registered before `{}` loads",
                    status.service_key, manifest.id
                );
                self.diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from(manifest.id.clone()),
                        DiagnosticSeverity::Error,
                        "services.required_missing",
                        message.clone(),
                    )
                    .with_generation(generation_id)
                    .with_source(PluginSourceSpan::Manifest {
                        path: format!("plugins/{}/plugin.toml", manifest.id),
                        key: Some("consumes_services".to_string()),
                    })
                    .with_context(serde_json::json!({
                        "service": status.service_key,
                        "required": true,
                    })),
                );
                return Err(PluginLoadError::BadManifest(message));
            }
            self.diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(manifest.id.clone()),
                    DiagnosticSeverity::Warning,
                    "services.optional_missing",
                    format!(
                        "optional service `{}` is not registered; consumer will receive nil",
                        status.service_key
                    ),
                )
                .with_generation(generation_id)
                .with_source(PluginSourceSpan::Manifest {
                    path: format!("plugins/{}/plugin.toml", manifest.id),
                    key: Some("consumes_services".to_string()),
                })
                .with_context(serde_json::json!({
                    "service": status.service_key,
                    "required": false,
                })),
            );
        }
        Ok(())
    }

    pub(super) fn cleanup_ledger(&mut self, ledger: &ResourceLedger) {
        let slot_ops_len_before = self.slot_ops.len();
        let report = {
            let mut cleaner = HostResourceCleaner {
                slot_ops: &mut self.slot_ops,
                event_bus: &mut self.event_bus,
                service_registry: Rc::clone(&self.service_registry),
            };
            ledger.cleanup_all(&mut cleaner)
        };
        if self.slot_ops.len() != slot_ops_len_before {
            self.mark_slot_ops_changed();
        }
        if report.has_errors() {
            for error in report.errors {
                let diag = PluginDiagnostic::new(
                    error.resource.plugin_id.clone(),
                    DiagnosticSeverity::Error,
                    "cleanup.resource_failed",
                    format!(
                        "cleanup failed for {} {}: {}",
                        error.resource.kind, error.resource.handle, error.error
                    ),
                )
                .with_generation(error.resource.generation_id)
                .with_context(serde_json::json!({
                    "kind": error.resource.kind.as_str(),
                    "handle": error.resource.handle,
                    "resource_id": error.resource.resource_id.get(),
                }));
                self.diagnostics.record(diag);
            }
        }
    }

    /// Cheap-clone handle to the per-host capability audit log. Will be
    /// consumed by the devtools panel (reload transactions).
    pub fn audit_log(&self) -> AuditLog {
        self.audit_log.clone()
    }
}

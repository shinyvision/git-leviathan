//! Test harness for plugin host. Builds an in-memory host that can load
//! plugins from inline strings without touching disk (beyond a per-harness
//! tempdir).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use crate::plugin::diagnostic::{DiagnosticStore, NullSink};
use crate::plugin::host::PluginHost;

pub struct MockHost {
    host: PluginHost,
    _tmp: TempDir,
    plugin_dirs: HashMap<String, PathBuf>,
}

impl Default for MockHost {
    fn default() -> Self {
        Self::new()
    }
}

impl MockHost {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tmp");
        // Tests don't want diagnostics on stderr — they want to assert
        // against the in-memory store. The `NullSink` keeps the
        // emission path identical to production while suppressing
        // stderr noise for `cargo test` output.
        let mut host = PluginHost::new();
        host.set_diagnostic_store(DiagnosticStore::with_sink(Arc::new(NullSink)));
        host.set_plugin_storage_base(tmp.path().join("plugin-storage"));
        // Capability grant tests treat their own tmp directory as a trusted
        // "bundled plugin" root so `MockHost::load_inline` plugins
        // get their declared capabilities auto-granted. Tests that
        // need to exercise the prompt path (e.g.
        // `denied_capability_emits_diagnostic_and_audit_entry`) can
        // either skip the manifest's capability declaration or
        // call `host_mut().revoke_capability(…)` after load.
        host.trust_bundled_plugin_root(tmp.path());
        // Also trust the workspace's `plugins/` directory so the
        // bundled-plugin tests (which load straight from there) get
        // the same auto-grant treatment as production.
        if let Ok(cwd) = std::env::current_dir() {
            host.trust_bundled_plugin_root(cwd.join("plugins"));
        }
        Self {
            host,
            _tmp: tmp,
            plugin_dirs: HashMap::new(),
        }
    }

    /// Snapshot of the host diagnostic store. Cheap-clone; the store
    /// itself is `Arc`-backed so subsequent `record` calls are visible
    /// from this handle without a re-snapshot.
    pub fn diagnostics(&self) -> DiagnosticStore {
        self.host.diagnostics()
    }

    pub fn load_inline(
        &mut self,
        id: &str,
        manifest: &str,
        init: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = self._tmp.path().join(id);
        std::fs::create_dir_all(&dir)?;
        let manifest_with_runtime = inject_runtime_section(manifest);
        std::fs::write(dir.join("plugin.toml"), manifest_with_runtime)?;
        std::fs::write(dir.join("init.lua"), init)?;
        self.host
            .load_plugin(&dir)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        self.plugin_dirs.insert(id.to_string(), dir);
        Ok(())
    }

    /// Stage a set of plugin sources on disk and invoke the
    /// host's resolve-and-load entry point so the dependency resolver
    /// runs over them as a unit. Each tuple is `(id, manifest, init)`.
    /// Plugins are written into the harness tmp dir; the lockfile root
    /// is the same dir, matching `load_from_dir`'s contract. Tests
    /// assert against `host.diagnostics()` and `host.introspect()`.
    pub fn load_inline_set(
        &mut self,
        plugins: &[(&str, &str, &str)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut dirs: Vec<std::path::PathBuf> = Vec::new();
        for (id, manifest, init) in plugins {
            let dir = self._tmp.path().join(id);
            std::fs::create_dir_all(&dir)?;
            let manifest_with_runtime = inject_runtime_section(manifest);
            std::fs::write(dir.join("plugin.toml"), manifest_with_runtime)?;
            std::fs::write(dir.join("init.lua"), init)?;
            self.plugin_dirs.insert(id.to_string(), dir.clone());
            dirs.push(dir);
        }
        self.host.resolve_and_load(self._tmp.path(), &dirs);
        Ok(())
    }

    /// Path to the workspace tmp dir backing this harness — used by
    /// dependency lockfile tests that need to read or write `plugins.lock` /
    /// `plugins.lock.local` directly.
    pub fn lockfile_dir(&self) -> &std::path::Path {
        self._tmp.path()
    }

    /// Strict-globals counterpart for tests that want to assert the
    /// new package-layout enforcement behaviour. Skips the test-default
    /// auto-inject of `[runtime] strict_globals = false`.
    pub fn load_inline_strict(
        &mut self,
        id: &str,
        manifest: &str,
        init: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = self._tmp.path().join(id);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("plugin.toml"), manifest)?;
        std::fs::write(dir.join("init.lua"), init)?;
        self.host
            .load_plugin(&dir)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        self.plugin_dirs.insert(id.to_string(), dir);
        Ok(())
    }

    /// Drop a plugin file at an arbitrary path under a previously
    /// loaded plugin's directory (e.g. `lua/foo/bar.lua` or
    /// `after/plugin/x.lua`). Used by the package layout module-loader tests.
    pub fn write_plugin_file(
        &self,
        plugin_id: &str,
        rel_path: &str,
        content: &str,
    ) -> std::io::Result<()> {
        let dir = self
            .plugin_dirs
            .get(plugin_id)
            .ok_or_else(|| std::io::Error::other(format!("plugin {plugin_id} not loaded")))?;
        let path = dir.join(rel_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
    }

    pub fn has_plugin(&self, id: &str) -> bool {
        self.plugin_dirs.contains_key(id)
    }

    pub fn host(&self) -> &PluginHost {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut PluginHost {
        &mut self.host
    }

    pub fn root(&self) -> &std::path::Path {
        self._tmp.path()
    }

    pub fn plugin_dir(&self, id: &str) -> Option<&std::path::Path> {
        self.plugin_dirs.get(id).map(|p| p.as_path())
    }

    pub fn open_screen(&mut self, plugin_id: &str, screen_id: &str) {
        self.host
            .open_screen(plugin_id.to_string(), screen_id.to_string());
    }

    pub fn dispatch_screen_event(&mut self, plugin_id: &str, screen_id: &str, event: &str) {
        self.host
            .dispatch_event(plugin_id, screen_id, event, serde_json::Value::Null);
    }

    pub fn screen_state_json(&self, plugin_id: &str, screen_id: &str) -> serde_json::Value {
        self.host
            .screen_state_json(plugin_id, screen_id)
            .unwrap_or(serde_json::Value::Null)
    }

    pub fn reload_plugin(&mut self, plugin_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.host
            .reload_plugin(plugin_id)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    pub fn last_reload_error(&self, plugin_id: &str) -> Option<String> {
        self.host.last_reload_error(plugin_id).map(String::from)
    }

    pub fn read_global_i64(&self, plugin_id: &str, name: &str) -> Option<i64> {
        self.host.plugin_global_i64(plugin_id, name)
    }

    pub fn tick(&mut self) {
        self.host.tick();
    }

    /// performance budgets: install a `MockClock`-backed budget tracker so tests
    /// can drive timing deterministically. Returns the clock handle so
    /// the test can `advance_ms()` between callback dispatches. Must
    /// be called before `load_inline` so the tracker covers init.lua
    /// timing too.
    pub fn install_mock_clock(&mut self) -> std::sync::Arc<crate::plugin::performance::MockClock> {
        let clock = std::sync::Arc::new(crate::plugin::performance::MockClock::new());
        let tracker = crate::plugin::performance::BudgetTracker::with_clock(
            self.host.diagnostics(),
            clock.clone(),
        );
        self.host.set_budget_tracker(tracker);
        clock
    }

    /// Cheap-clone handle to the host's budget tracker.
    pub fn budget_tracker(&self) -> crate::plugin::performance::BudgetTracker {
        self.host.budget_tracker()
    }

    pub fn invoke_user_command(
        &mut self,
        plugin_id: &str,
        name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.host
            .invoke_user_command(plugin_id, name)
            .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))
    }

    /// typed command dispatch entry. Routes through the same
    /// [`crate::plugin::commands::CommandRegistry::invoke`] funnel as
    /// the Lua `leviathan.command.invoke` shim.
    pub fn invoke_command(
        &mut self,
        name: &str,
        args: serde_json::Value,
    ) -> crate::plugin::commands::InvokeOutcome {
        self.host.invoke_command(name, args)
    }

    /// devtools dispatch entry. Routes through the same
    /// dispatcher as `invoke_command` and additionally drains the
    /// host's queued devtools actions, returning the structured JSON
    /// result alongside the outcome.
    pub fn invoke_devtools_command(
        &mut self,
        name: &str,
        args: serde_json::Value,
    ) -> (crate::plugin::commands::InvokeOutcome, serde_json::Value) {
        self.host.invoke_devtools_command(name, args)
    }

    /// keymaps dispatch entry. Routes through
    /// [`crate::plugin::host::PluginHost::dispatch_key`].
    pub fn dispatch_key(
        &mut self,
        context: &str,
        buffer: &[crate::plugin::keymap::Keystroke],
    ) -> crate::plugin::keymap::KeymapDispatchOutcome {
        self.host.dispatch_key(context, buffer)
    }

    /// Cheap-clone handle to the unified keymap registry. Used by
    /// keymaps tests that project summaries.
    pub fn keymap_registry(
        &self,
    ) -> std::rc::Rc<std::cell::RefCell<crate::plugin::keymap::KeymapRegistry>> {
        self.host.keymap_registry()
    }

    pub fn set_keymap_leader(&mut self, leader: &str) {
        self.host.set_keymap_leader(leader);
    }

    pub fn set_user_keymap(&mut self, context: &str, key: &str, command: &str) {
        self.host.set_user_keymap(context, key, command);
    }

    pub fn set_builtin_keymap(
        &mut self,
        context: &str,
        key: &str,
        command: &str,
        description: &str,
    ) {
        self.host
            .set_builtin_keymap(context, key, command, description);
    }

    pub fn unload_plugin(&mut self, plugin_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.host
            .unload_plugin(plugin_id)
            .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))
    }

    /// Cheap-clone handle to the unified command registry. Used by
    /// command registry tests that project summaries and assert palette
    /// filtering / dispatch counters.
    pub fn command_registry(
        &self,
    ) -> std::rc::Rc<std::cell::RefCell<crate::plugin::commands::CommandRegistry>> {
        self.host.command_registry()
    }

    pub fn coroutine_count(&self, plugin_id: &str) -> usize {
        self.host.coroutine_count(plugin_id)
    }

    pub fn run_health_checks(&self) -> crate::plugin::api::health::HealthReport {
        self.host.run_health_checks()
    }

    pub fn has_slot(&self, plugin_id: &str, region: &str, container: &str, slot_id: &str) -> bool {
        self.host.has_slot(plugin_id, region, container, slot_id)
    }

    pub fn introspect(&self) -> crate::plugin::devtools::InspectorSnapshot {
        self.host.introspect()
    }

    pub fn reset_plugin_storage(&mut self, plugin_id: &str, surface: &str) -> Result<(), String> {
        self.host.reset_plugin_storage(plugin_id, surface)
    }

    /// typed-event replay. Forwards through
    /// `PluginHost::dispatch_test_event` so tests stage event
    /// sequences deterministically.
    pub fn dispatch_test_event(&mut self, event: &str, payload: serde_json::Value) {
        self.host.dispatch_test_event(event, payload);
    }

    /// Advance the host's virtual debounce clock by `delta_ms` so
    /// tests can validate `debounce_ms` without real wall-clock
    /// sleeps.
    pub fn advance_event_clock(&mut self, delta_ms: u64) {
        self.host.advance_event_clock(delta_ms);
    }

    /// Read a Lua global as `String`. Used by autocmd tests that
    /// observe payload-table fields a callback copied into `_G`.
    pub fn read_global_string(&self, plugin_id: &str, name: &str) -> Option<String> {
        self.host.plugin_global_string(plugin_id, name)
    }

    /// Rewrites the plugin's `plugin.toml` and `init.lua` on disk and
    /// reloads. Used by tests that exercise reload-failure / rollback
    /// paths where the new init.lua needs to be different from the
    /// initial load.
    pub fn reload_with_str(
        &mut self,
        plugin_id: &str,
        manifest: &str,
        init: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = self
            .plugin_dirs
            .get(plugin_id)
            .ok_or("plugin not loaded")?
            .clone();
        let manifest_with_runtime = inject_runtime_section(manifest);
        std::fs::write(dir.join("plugin.toml"), manifest_with_runtime)?;
        std::fs::write(dir.join("init.lua"), init)?;
        self.reload_plugin(plugin_id)
    }
}

fn inject_runtime_section(manifest: &str) -> String {
    if manifest.contains("[runtime]") || manifest.contains("strict_globals") {
        manifest.to_string()
    } else {
        format!("{manifest}\n[runtime]\nstrict_globals = false\n")
    }
}

pub fn test_host_with_plugin(init_lua: &str) -> MockHost {
    let mut host = MockHost::new();
    let manifest = r#"
id = "test"
name = "Test"
version = "0.1.0"
api_version = "1.0"
"#;
    host.load_inline("test", manifest, init_lua)
        .expect("load test plugin");
    host
}

pub fn load_plugin_from_str(plugin_str: &str) -> Result<MockHost, Box<dyn std::error::Error>> {
    let (manifest, init) = split_plugin_str(plugin_str)?;
    let id = extract_id(&manifest)?;
    let mut host = MockHost::new();
    host.load_inline(&id, &manifest, &init)?;
    Ok(host)
}

fn split_plugin_str(s: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let manifest_marker = "[manifest]";
    let init_marker = "[init.lua]";
    let m_start = s
        .find(manifest_marker)
        .ok_or("missing [manifest] section")?;
    let i_start = s.find(init_marker).ok_or("missing [init.lua] section")?;
    if m_start >= i_start {
        return Err("`[manifest]` section must appear before `[init.lua]` section".into());
    }
    let manifest = s[m_start + manifest_marker.len()..i_start]
        .trim()
        .to_string();
    let init = s[i_start + init_marker.len()..].trim().to_string();
    Ok((manifest, init))
}

fn extract_id(manifest: &str) -> Result<String, Box<dyn std::error::Error>> {
    let v: toml::Value = toml::from_str(manifest)?;
    v.get("id")
        .and_then(|i| i.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "manifest missing id".into())
}

#[cfg(test)]
mod self_tests {
    use super::*;

    const SMOKE_MANIFEST: &str = r#"
id = "smoke"
name = "Smoke"
version = "0.1.0"
api_version = "1.0"
"#;

    const BAD_MANIFEST: &str = r#"
id = "bad"
name = "Bad"
version = "0.1.0"
api_version = "1.0"
"#;

    #[test]
    fn mock_host_loads_inline_plugin() {
        let mut host = MockHost::new();
        host.load_inline("smoke", SMOKE_MANIFEST, "").expect("load");
        assert!(host.has_plugin("smoke"));

        let helper_host = test_host_with_plugin("-- helper smoke");
        assert!(helper_host.has_plugin("test"));
    }

    #[test]
    fn mock_host_reports_init_error() {
        let mut host = MockHost::new();
        let err = host
            .load_inline("bad", BAD_MANIFEST, r#"error("boom")"#)
            .unwrap_err();
        assert!(err.to_string().contains("boom"), "got: {err}");
    }

    #[test]
    fn mock_host_exposes_paths() {
        let mut host = MockHost::new();
        host.load_inline(
            "p",
            r#"
			id = "p"
			name = "P"
			version = "0.1.0"
			api_version = "1.0"
		"#,
            "",
        )
        .expect("load");
        assert!(host.root().is_dir());
        assert_eq!(host.plugin_dir("p"), Some(host.root().join("p").as_path()));
        assert!(host.plugin_dir("missing").is_none());
    }

    #[test]
    fn sample_plugins_load_unchanged() {
        // Regression guard for widget schema validation: every static widget
        // shape used by the bundled sample plugins must round-trip through
        // `WidgetKind`. If a plugin breaks here, the schema is too strict.
        let mut host = MockHost::new();
        for plugin in [
            "dancing_banana_test",
            "repository_info",
            "tablist_demo",
            "regions_demo",
        ] {
            let dir = std::path::PathBuf::from("plugins").join(plugin);
            host.host_mut()
                .load_plugin(&dir)
                .unwrap_or_else(|e| panic!("plugin {plugin} broke: {e}"));
        }
    }

    fn bundled_plugin_dirs() -> Vec<std::path::PathBuf> {
        let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir("plugins")
            .expect("plugins dir")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.join("plugin.toml").is_file() && path.join("init.lua").is_file())
            .collect();
        dirs.sort();
        dirs
    }

    fn load_all_bundled_plugins() -> MockHost {
        let mut host = MockHost::new();
        for dir in bundled_plugin_dirs() {
            let plugin_name = dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<unknown>")
                .to_string();
            host.host_mut()
                .load_plugin(&dir)
                .unwrap_or_else(|e| panic!("bundled plugin {plugin_name} failed to load: {e}"));
        }
        host
    }

    #[test]
    fn loads_every_bundled_plugin_under_current_api_version() {
        let dirs = bundled_plugin_dirs();
        assert!(!dirs.is_empty(), "expected bundled plugins");

        let host = load_all_bundled_plugins();
        let snap = host.introspect();
        let ids: Vec<&str> = snap
            .plugins
            .iter()
            .map(|plugin| plugin.id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec![
                "dancing_banana_test",
                "file_explorer",
                "foo_demo",
                "regions_demo",
                "repository_info",
                "tablist_demo",
                "terminal",
            ]
        );
        assert!(
            snap.plugins
                .iter()
                .all(|plugin| plugin.api_version == "1.0"),
            "all bundled plugins should use the v1 authoring surface"
        );
    }

    #[test]
    fn bundled_plugin_manifest_api_version_snapshot() {
        use git_leviathan_plugin_api::api_version::HOST_API_VERSION;

        assert_eq!(HOST_API_VERSION.major, 1);
        assert_eq!(HOST_API_VERSION.minor, 0);

        let mut snapshot = String::from("api_version = \"1.0\"\n");
        for dir in bundled_plugin_dirs() {
            let manifest_path = dir.join("plugin.toml");
            let raw = std::fs::read_to_string(&manifest_path).expect("manifest");
            let manifest: toml::Value = toml::from_str(&raw).expect("manifest toml");
            let id = manifest
                .get("id")
                .and_then(|value| value.as_str())
                .expect("manifest id");
            let api_version = manifest
                .get("api_version")
                .and_then(|value| value.as_str())
                .expect("manifest api_version");
            snapshot.push_str(&format!("{id}: api_version={api_version}\n"));
        }

        assert_eq!(
            snapshot,
            concat!(
                "api_version = \"1.0\"\n",
                "dancing_banana_test: api_version=1.0\n",
                "file_explorer: api_version=1.0\n",
                "foo_demo: api_version=1.0\n",
                "regions_demo: api_version=1.0\n",
                "repository_info: api_version=1.0\n",
                "tablist_demo: api_version=1.0\n",
                "terminal: api_version=1.0\n",
            )
        );
    }

    #[test]
    fn bundled_plugin_slot_registration_snapshot() {
        let host = load_all_bundled_plugins();
        let snap = host.introspect();
        let mut snapshot = String::from("api_version = \"1.0\"\n");
        for slot in snap.slots {
            snapshot.push_str(&format!(
                "{} {} {} priority={} owner={}\n",
                slot.region, slot.container, slot.id, slot.priority, slot.owner_plugin_id
            ));
        }

        assert_eq!(
            snapshot,
            concat!(
				"api_version = \"1.0\"\n",
				"main_bar center plugin.terminal.terminal priority=60 owner=terminal\n",
				"main_bar left builtin.fetch_indicator priority=40 owner=dancing_banana_test\n",
				"main_bar left builtin.repo_info priority=10 owner=repository_info\n",
				"main_bar right plugin.file_explorer.files priority=100 owner=file_explorer\n",
				"main_bar right plugin.foo_demo.foo priority=101 owner=foo_demo\n",
				"repository sidebar.top plugin.regions_demo.banner priority=10 owner=regions_demo\n",
				"tab_bar center builtin.tab_list priority=10 owner=tablist_demo\n",
				"tab_bar right plugin.regions_demo.tag priority=10 owner=regions_demo\n",
			)
        );
    }

    #[test]
    fn region_descriptor_snapshot() {
        use git_leviathan_plugin_api::descriptor::region::{RegionKind, REGIONS};

        let mut snapshot = String::from("api_version = \"1.0\"\n");
        for region in REGIONS.iter() {
            match &region.kind {
                RegionKind::Chrome {
                    sections,
                    dynamic_section_prefixes,
                } => {
                    let dyn_part = if dynamic_section_prefixes.is_empty() {
                        String::new()
                    } else {
                        format!(" dyn=[{}]", dynamic_section_prefixes.join(", "))
                    };
                    snapshot.push_str(&format!(
                        "{}: chrome sections=[{}]{}\n",
                        region.name,
                        sections.join(", "),
                        dyn_part
                    ));
                }
                RegionKind::Content { panes } => {
                    let pane_snapshot = panes
                        .iter()
                        .map(|pane| {
                            let dyn_part = if pane.dynamic_section_prefixes.is_empty() {
                                String::new()
                            } else {
                                format!("+dyn[{}]", pane.dynamic_section_prefixes.join(","))
                            };
                            format!("{}({}{})", pane.name, pane.sections.join(","), dyn_part)
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    snapshot.push_str(&format!(
                        "{}: content panes=[{}]\n",
                        region.name, pane_snapshot
                    ));
                }
            }
        }

        assert_eq!(
            snapshot,
            concat!(
                "api_version = \"1.0\"\n",
                "main_bar: chrome sections=[left, center, right]\n",
                "tab_bar: chrome sections=[left, center, right]\n",
                "status_bar: chrome sections=[left, center, right]\n",
                "repository: content panes=[sidebar(top,bottom+dyn[section:]), main(top,bottom)]\n",
                "repository.graph: content panes=[graph(top,decorations,context_menu+dyn[row:])]\n",
                "repository.details: content panes=[details(commit_header,files)]\n",
                "repository.diff: content panes=[diff(toolbar,context_menu+dyn[line:,hunk:])]\n",
            )
        );
    }

    #[test]
    fn fs_read_blocked_without_capability() {
        let r = load_plugin_from_str(
            r#"
		[manifest]
		id = "no-fs"
		name = "no-fs"
		version = "0.1.0"
		api_version = "1.0"

		[init.lua]
		leviathan.fs.read_file("/etc/passwd")
		"#,
        );
        let err = match r {
            Err(e) => e.to_string(),
            Ok(_) => panic!("expected load failure"),
        };
        assert!(err.contains("capability denied: fs:read"), "got: {err}");
    }

    #[test]
    fn host_load_error_carries_plugin_traceback() {
        let r = load_plugin_from_str(
            r#"
		[manifest]
		id = "buggy"
		name = "buggy"
		version = "0.1.0"
		api_version = "1.0"

		[init.lua]
		leviathan.ui.regions.add_slot{ region = "main_bar", id = "x", section = "nope", priority = 0, widget = { kind = "text", value = "hi" } }
		"#,
        );
        let s = match r {
            Err(e) => e.to_string(),
            Ok(_) => panic!("expected load failure"),
        };
        assert!(s.contains("plugin 'buggy'"), "got: {s}");
        assert!(s.contains("init.lua"), "got: {s}");
        assert!(s.contains("unknown section 'nope'"), "got: {s}");
    }

    #[test]
    fn screen_state_persists_across_reload() {
        let mut host = MockHost::new();
        host.load_inline(
            "counter",
            r#"
			id = "counter"
			name = "Counter"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"
			leviathan.ui.register_screen{
				id = "counter",
				init = function() return { n = 0 } end,
				view = function(s) return { kind = "text", value = tostring(s.n) } end,
				update = function(s, evt)
					if evt == "inc" then s.n = s.n + 1 end
					return s
				end,
				serialize = function(s) return { n = s.n } end,
				deserialize = function(t) return { n = t.n } end,
			}
			"#,
        )
        .expect("load");
        host.open_screen("counter", "counter");
        host.dispatch_screen_event("counter", "counter", "inc");
        host.dispatch_screen_event("counter", "counter", "inc");

        let pre = host.screen_state_json("counter", "counter");
        assert_eq!(pre.get("n").and_then(|v| v.as_i64()), Some(2));

        host.reload_plugin("counter").expect("reload ok");

        let post = host.screen_state_json("counter", "counter");
        assert_eq!(post.get("n").and_then(|v| v.as_i64()), Some(2));
    }

    #[test]
    fn screen_state_resets_when_no_serialize() {
        let mut host = MockHost::new();
        host.load_inline(
            "no_persist",
            r#"
			id = "no_persist"
			name = "no_persist"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"
			leviathan.ui.register_screen{
				id = "main",
				init = function() return { n = 0 } end,
				view = function(s) return { kind = "text", value = tostring(s.n) } end,
				update = function(s, evt)
					if evt == "inc" then s.n = s.n + 1 end
					return s
				end,
			}
			"#,
        )
        .expect("load");
        host.open_screen("no_persist", "main");
        host.dispatch_screen_event("no_persist", "main", "inc");
        let pre = host.screen_state_json("no_persist", "main");
        assert_eq!(pre.get("n").and_then(|v| v.as_i64()), Some(1));

        host.reload_plugin("no_persist").expect("reload ok");

        let post = host.screen_state_json("no_persist", "main");
        assert_eq!(post.get("n").and_then(|v| v.as_i64()), Some(0));
    }

    #[test]
    fn reload_failure_keeps_old_version() {
        let mut host = MockHost::new();
        host.load_inline(
            "v1plugin",
            r#"
			id = "v1plugin"
			name = "v1"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"
			leviathan.ui.regions.add_slot{ region = "main_bar",
				id = "v1.slot",
				section = "left",
				priority = 50,
				widget = { kind = "text", value = "v1" },
			}
			"#,
        )
        .expect("v1 loads");

        assert!(
            host.has_slot("v1plugin", "main_bar", "left", "v1.slot"),
            "v1 slot present"
        );

        // Now overwrite init.lua with a syntactically broken version.
        let dir = host.plugin_dir("v1plugin").expect("dir").to_path_buf();
        std::fs::write(dir.join("init.lua"), "this is not valid lua >>>").unwrap();

        let result = host.reload_plugin("v1plugin");
        assert!(result.is_err(), "reload should fail");

        // v1 still serves; new error recorded.
        assert!(
            host.has_slot("v1plugin", "main_bar", "left", "v1.slot"),
            "v1 slot must still be present after failed reload"
        );
        let err = host.last_reload_error("v1plugin").expect("error recorded");
        assert!(
            err.contains("syntax")
                || err.contains("parse")
                || err.contains("'>'")
                || err.contains("not valid"),
            "got: {err}"
        );
    }

    #[test]
    fn successful_reload_clears_error() {
        let mut host = MockHost::new();
        host.load_inline(
            "clear",
            r#"
			id = "clear"
			name = "clear"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            "-- v1",
        )
        .expect("v1 loads");

        let dir = host.plugin_dir("clear").expect("dir").to_path_buf();

        // First, fail a reload.
        std::fs::write(dir.join("init.lua"), "this is not valid lua >>>").unwrap();
        let _ = host.reload_plugin("clear");
        assert!(host.last_reload_error("clear").is_some());

        // Then fix it and reload again.
        std::fs::write(dir.join("init.lua"), "-- v3").unwrap();
        host.reload_plugin("clear").expect("v3 reloads");
        assert!(
            host.last_reload_error("clear").is_none(),
            "successful reload should clear last error"
        );
    }

    #[test]
    fn services_round_trip() {
        let mut host = MockHost::new();
        host.load_inline(
            "publisher",
            r#"
			id = "publisher"
			name = "publisher"
			version = "0.1.0"
			api_version = "1.0"
			provides_services = ["math@1"]
			"#,
            r#"
			leviathan.services.register("math@1", {
				add = function(a, b) return a + b end,
				mul = function(a, b) return a * b end,
			})
			"#,
        )
        .expect("publisher loads");

        host.load_inline(
            "consumer",
            r#"
			id = "consumer"
			name = "consumer"
			version = "0.1.0"
			api_version = "1.0"
			consumes_services = ["math@1"]
			"#,
            r#"
			local m = leviathan.services.get("math@1")
			_G.add_result = m.add(2, 3)
			_G.mul_result = m.mul(4, 5)
			"#,
        )
        .expect("consumer loads");

        let add_result = host
            .read_global_i64("consumer", "add_result")
            .expect("add result");
        let mul_result = host
            .read_global_i64("consumer", "mul_result")
            .expect("mul result");
        assert_eq!(add_result, 5);
        assert_eq!(mul_result, 20);
    }

    #[test]
    fn services_get_undeclared_rejected() {
        let mut host = MockHost::new();
        let r = host.load_inline(
            "bad",
            r#"
			id = "bad"
			name = "bad"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"
			leviathan.services.get("math@1")
			"#,
        );
        let err = r.expect_err("undeclared get should error").to_string();
        assert!(
            err.contains("not declared in consumes_services") || err.contains("not registered"),
            "got: {err}"
        );
    }

    #[test]
    fn services_register_undeclared_rejected() {
        let mut host = MockHost::new();
        let r = host.load_inline(
            "bad2",
            r#"
			id = "bad2"
			name = "bad2"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"
			leviathan.services.register("math@1", { f = function() end })
			"#,
        );
        let err = r.expect_err("undeclared register should error").to_string();
        assert!(
            err.contains("not declared in provides_services"),
            "got: {err}"
        );
    }

    #[test]
    fn schedule_defers_until_tick() {
        let mut host = MockHost::new();
        host.load_inline(
            "sched",
            r#"
			id = "sched"
			name = "sched"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"
			_G.x = 0
			leviathan.api.schedule(function() _G.x = 1 end)
			"#,
        )
        .expect("load");
        assert_eq!(
            host.read_global_i64("sched", "x"),
            Some(0),
            "before tick: still 0"
        );
        host.tick();
        assert_eq!(host.read_global_i64("sched", "x"), Some(1), "after tick: 1");
    }

    #[test]
    fn defer_fn_waits_for_elapsed_ms() {
        let mut host = MockHost::new();
        host.load_inline(
            "deferred",
            r#"
			id = "deferred"
			name = "deferred"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"
			_G.x = 0
			leviathan.api.defer_fn(50, function() _G.x = 9 end)
			"#,
        )
        .expect("load");
        host.tick();
        assert_eq!(
            host.read_global_i64("deferred", "x"),
            Some(0),
            "before 50ms: still 0"
        );
        std::thread::sleep(std::time::Duration::from_millis(80));
        host.tick();
        assert_eq!(
            host.read_global_i64("deferred", "x"),
            Some(9),
            "after sleep + tick: fired"
        );
    }

    #[test]
    fn user_command_yield_does_not_block() {
        let mut host = MockHost::new();
        host.load_inline(
            "cmd",
            r#"
			id = "cmd"
			name = "cmd"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"
			_G.steps = 0
			leviathan.command.create("slow", {
				run = function()
					for i = 1, 5 do
						_G.steps = _G.steps + 1
						coroutine.yield()
					end
				end,
			})
			"#,
        )
        .expect("load");

        let start = std::time::Instant::now();
        host.invoke_user_command("cmd", "slow").expect("invoke");
        assert!(
            start.elapsed() < std::time::Duration::from_millis(50),
            "first resume must be quick"
        );
        assert_eq!(host.read_global_i64("cmd", "steps"), Some(1));
        assert_eq!(host.coroutine_count("cmd"), 1);

        for _ in 0..10 {
            host.tick();
            if host.coroutine_count("cmd") == 0 {
                break;
            }
        }
        assert_eq!(host.coroutine_count("cmd"), 0, "coroutine should finish");
        assert_eq!(host.read_global_i64("cmd", "steps"), Some(5));
    }

    #[test]
    fn persist_lua_round_trip() {
        let mut host = MockHost::new();
        host.load_inline(
            "p",
            r#"
			id = "p"
			name = "p"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"
			local s = leviathan.persist.open("data", { version = 1 })
			s:set("n", 7)
			_G.read_back = s:get("n")
			"#,
        )
        .expect("load");
        assert_eq!(host.read_global_i64("p", "read_back"), Some(7));
    }

    #[test]
    fn fs_read_allowed_with_capability() {
        let host = load_plugin_from_str(
            r#"
		[manifest]
		id = "yes-fs"
		name = "yes-fs"
		version = "0.1.0"
		api_version = "1.0"
		capabilities = ["fs:read:plugin"]

		[init.lua]
		local _ = leviathan.fs.read_file("init.lua")
		"#,
        )
        .expect("load should succeed");
        assert!(host.has_plugin("yes-fs"));
    }

    #[test]
    fn health_check_runs_per_plugin() {
        let mut host = MockHost::new();
        host.load_inline(
            "p1",
            r#"
			id = "p1"
			name = "p1"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"
			leviathan.health.register(function(ctx)
				ctx:ok("lua present")
				ctx:warn("disk slow")
				ctx:info("cache size: 4")
				ctx:error("missing dep")
			end)
			"#,
        )
        .expect("load");

        let report = host.run_health_checks();
        let p1 = report.for_plugin("p1").expect("p1 in report");
        assert_eq!(p1.items.len(), 4);
        let messages: Vec<&str> = p1.items.iter().map(|item| item.message.as_str()).collect();
        assert!(messages.contains(&"lua present"));
        assert!(messages.contains(&"missing dep"));

        let kinds: Vec<_> = p1.items.iter().map(|i| i.severity).collect();
        use crate::plugin::api::health::Severity;
        assert!(kinds.contains(&Severity::Ok));
        assert!(kinds.contains(&Severity::Warn));
        assert!(kinds.contains(&Severity::Info));
        assert!(kinds.contains(&Severity::Error));
    }

    #[test]
    fn health_report_aggregates_multiple_plugins() {
        let mut host = MockHost::new();
        for id in ["pa", "pb"] {
            host.load_inline(
                id,
                &format!(
                    r#"
				id = "{id}"
				name = "{id}"
				version = "0.1.0"
				api_version = "1.0"
				"#
                ),
                &format!(
                    r#"
				leviathan.health.register(function(ctx)
					ctx:ok("hello from {id}")
				end)
				"#
                ),
            )
            .expect("load");
        }
        let report = host.run_health_checks();
        assert_eq!(report.plugins.len(), 2);
    }

    #[test]
    fn introspect_lists_loaded_plugins() {
        let mut host = MockHost::new();
        for id in ["alpha", "beta"] {
            host.load_inline(
                id,
                &format!(
                    r#"
				id = "{id}"
				name = "{id}"
				version = "0.1.0"
				api_version = "1.0"
				"#
                ),
                "",
            )
            .expect("load");
        }
        let snap = host.introspect();
        assert_eq!(snap.plugins.len(), 2);
        let ids: Vec<&str> = snap.plugins.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"alpha"));
        assert!(ids.contains(&"beta"));
        let alpha = snap
            .plugins
            .iter()
            .find(|plugin| plugin.id == "alpha")
            .expect("alpha summary");
        assert_eq!(alpha.name, "alpha");
        assert_eq!(alpha.version, "0.1.0");
        assert!(alpha.last_reload_error.is_none());
        assert!(alpha.provides_services.is_empty());
        assert!(alpha.consumes_services.is_empty());
        assert!(alpha.capabilities.is_empty());
    }

    #[test]
    fn introspect_records_slot_ownership() {
        let mut host = MockHost::new();
        host.load_inline(
            "owner",
            r#"
			id = "owner"
			name = "owner"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"
			leviathan.ui.regions.add_slot{ region = "main_bar",
				id = "owner.slot",
				section = "left",
				priority = 50,
				widget = { kind = "text", value = "x" },
			}
			"#,
        )
        .expect("load");
        let snap = host.introspect();
        let slot = snap
            .slots
            .iter()
            .find(|s| s.id == "owner.slot")
            .expect("slot present");
        assert_eq!(slot.region, "main_bar");
        assert_eq!(slot.container, "left");
        assert_eq!(slot.owner_plugin_id, "owner");
    }

    #[test]
    fn introspect_records_services() {
        let mut host = MockHost::new();
        host.load_inline(
            "publisher",
            r#"
			id = "publisher"
			name = "publisher"
			version = "0.1.0"
			api_version = "1.0"
			provides_services = ["math@1"]
			"#,
            r#"
			leviathan.services.register("math@1", {
				add = function(a, b) return a + b end,
				sub = function(a, b) return a - b end,
			})
			"#,
        )
        .expect("load");
        let snap = host.introspect();
        let svc = snap
            .services
            .iter()
            .find(|s| s.key == "math@1")
            .expect("service present");
        assert_eq!(svc.publisher_plugin_id, "publisher");
        assert_eq!(svc.methods.len(), 2);
        assert!(svc.methods.contains(&"add".to_string()));
        assert!(svc.methods.contains(&"sub".to_string()));
    }

    #[test]
    fn introspect_includes_recent_audit_entries() {
        let mut host = MockHost::new();
        let _ = host.load_inline(
            "no_fs",
            r#"
			id = "no_fs"
			name = "no_fs"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            r#"leviathan.fs.read_file("/etc/passwd")"#,
        );
        let snap = host.introspect();
        assert!(!host.host().audit_log().entries().is_empty());
        assert!(
            !snap.audit_recent.is_empty(),
            "expected at least one audit entry"
        );
        let denied = snap
            .audit_recent
            .iter()
            .find(|e| e.outcome == crate::plugin::audit::AuditOutcome::Denied);
        assert!(denied.is_some(), "expected a Denied entry");
    }

    #[test]
    fn plugins_without_health_check_absent_from_report() {
        let mut host = MockHost::new();
        host.load_inline(
            "no_health",
            r#"
			id = "no_health"
			name = "no_health"
			version = "0.1.0"
			api_version = "1.0"
			"#,
            "-- nothing",
        )
        .expect("load");
        let report = host.run_health_checks();
        assert!(report.for_plugin("no_health").is_none());
    }

    #[test]
    fn regions_demo_does_not_break_existing_layout() {
        // After regions_demo registers its sidebar.top banner, the slot
        // shows up in the host's introspection — proves the slot
        // registration succeeded. The actual layout fix is a visual concern;
        // this test guards the data path.
        let mut host = MockHost::new();
        let dir = std::path::PathBuf::from("plugins/regions_demo");
        host.host_mut()
            .load_plugin(&dir)
            .expect("regions_demo loads");
        let snap = host.introspect();
        assert!(
            snap.slots.iter().any(|s| s.region == "repository"
                && s.container == "sidebar.top"
                && s.id == "plugin.regions_demo.banner"),
            "regions_demo sidebar banner slot must register"
        );
    }
}

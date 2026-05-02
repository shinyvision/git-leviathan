//! Test harness for plugin host. Builds an in-memory host that can load
//! plugins from inline strings without touching disk (beyond a per-harness
//! tempdir).

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tempfile::TempDir;

use crate::plugin::host::PluginHost;

pub struct MockHost {
	host: PluginHost,
	_tmp: TempDir,
	plugin_dirs: HashMap<String, PathBuf>,
	audit_log: Arc<Mutex<Vec<AuditEntry>>>,
}

#[derive(Debug, Clone)]
pub struct AuditEntry {
	pub plugin_id: String,
	pub source: String,
	pub outcome: AuditOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditOutcome {
	Allowed,
	Denied,
}

impl Default for MockHost {
	fn default() -> Self {
		Self::new()
	}
}

impl MockHost {
	pub fn new() -> Self {
		let tmp = tempfile::tempdir().expect("tmp");
		Self {
			host: PluginHost::new(),
			_tmp: tmp,
			plugin_dirs: HashMap::new(),
			audit_log: Arc::new(Mutex::new(Vec::new())),
		}
	}

	pub fn load_inline(
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

	pub fn has_plugin(&self, id: &str) -> bool {
		self.plugin_dirs.contains_key(id)
	}

	pub fn host(&self) -> &PluginHost {
		&self.host
	}

	pub fn host_mut(&mut self) -> &mut PluginHost {
		&mut self.host
	}

	pub fn audit_log(&self) -> Arc<Mutex<Vec<AuditEntry>>> {
		Arc::clone(&self.audit_log)
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

	pub fn reload_plugin(
		&mut self,
		plugin_id: &str,
	) -> Result<(), Box<dyn std::error::Error>> {
		self.host
			.reload_plugin(plugin_id)
			.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
	}

	pub fn last_reload_error(&self, plugin_id: &str) -> Option<String> {
		self.host.last_reload_error(plugin_id).map(String::from)
	}

	pub fn has_slot(
		&self,
		plugin_id: &str,
		region: &str,
		container: &str,
		slot_id: &str,
	) -> bool {
		self.host.has_slot(plugin_id, region, container, slot_id)
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
		std::fs::write(dir.join("plugin.toml"), manifest)?;
		std::fs::write(dir.join("init.lua"), init)?;
		self.reload_plugin(plugin_id)
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
	let m_start = s.find(manifest_marker).ok_or("missing [manifest] section")?;
	let i_start = s.find(init_marker).ok_or("missing [init.lua] section")?;
	if m_start >= i_start {
		return Err("`[manifest]` section must appear before `[init.lua]` section".into());
	}
	let manifest = s[m_start + manifest_marker.len()..i_start].trim().to_string();
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
		assert!(
			err.contains("capability not granted: fs:read"),
			"got: {err}"
		);
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
		leviathan.ui.main_bar.add{ id = "x", section = "nope", priority = 0, widget = { kind = "text", value = "hi" } }
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
			leviathan.ui.main_bar.add{
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
		let err = host
			.last_reload_error("v1plugin")
			.expect("error recorded");
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
}

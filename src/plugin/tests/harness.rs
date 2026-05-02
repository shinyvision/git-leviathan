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
}

pub fn test_host_with_plugin(init_lua: &str) -> MockHost {
	let mut host = MockHost::new();
	let manifest = r#"[plugin]
id = "test"
name = "Test"
version = "0.1.0"
api = 1
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
	let manifest = s[m_start + manifest_marker.len()..i_start].trim().to_string();
	let init = s[i_start + init_marker.len()..].trim().to_string();
	Ok((manifest, init))
}

fn extract_id(manifest: &str) -> Result<String, Box<dyn std::error::Error>> {
	for line in manifest.lines() {
		if let Some(rest) = line.trim().strip_prefix("id") {
			let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '=');
			return Ok(rest.trim().trim_matches('"').to_string());
		}
	}
	Err("manifest missing id".into())
}

#[cfg(test)]
mod self_tests {
	use super::*;

	const SMOKE_MANIFEST: &str = r#"[plugin]
id = "smoke"
name = "Smoke"
version = "0.1.0"
api = 1
"#;

	const BAD_MANIFEST: &str = r#"[plugin]
id = "bad"
name = "Bad"
version = "0.1.0"
api = 1
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
}

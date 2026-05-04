//! Plugin error type. Wraps mlua/runtime errors with plugin identity and
//! Lua traceback so plugin authors get actionable failures.

use std::fmt;

#[derive(Debug, Clone)]
pub struct PluginError {
    pub plugin_id: String,
    pub source: String,
    pub message: String,
    pub lua_traceback: Option<String>,
}

impl PluginError {
    pub fn new(
        plugin_id: impl Into<String>,
        source: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            source: source.into(),
            message: message.into(),
            lua_traceback: None,
        }
    }

    /// Build a PluginError from an mlua::Error, extracting the traceback if
    /// present. Use `source` to label the host-side call site that failed
    /// (e.g. "init.lua exec", "leviathan.ui.slot.add").
    pub fn from_mlua(
        plugin_id: impl Into<String>,
        source: impl Into<String>,
        e: &mlua::Error,
    ) -> Self {
        let traceback = match e {
            mlua::Error::CallbackError { traceback, .. } => Some(traceback.clone()),
            mlua::Error::RuntimeError(s) | mlua::Error::SyntaxError { message: s, .. } => {
                if s.contains("stack traceback") {
                    Some(s.clone())
                } else {
                    None
                }
            }
            _ => None,
        };
        Self {
            plugin_id: plugin_id.into(),
            source: source.into(),
            message: e.to_string(),
            lua_traceback: traceback,
        }
    }
}

fn clean_traceback_line(s: &str) -> String {
    if let Some(start) = s.find("[string \"") {
        if let Some(close) = s[start..].find("\"]:") {
            let path = &s[start + "[string \"".len()..start + close];
            let after = &s[start + close + "\"]:".len()..];
            return format!("{path}:{after}");
        }
    }
    s.to_string()
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "plugin '{}': {}: {}",
            self.plugin_id, self.source, self.message
        )?;
        if let Some(t) = &self.lua_traceback {
            for line in t.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("[string")
                    || trimmed.starts_with("plugins/")
                    || trimmed.contains(".lua:")
                {
                    write!(f, "\n  at {}", clean_traceback_line(trimmed))?;
                    break;
                }
            }
        }
        Ok(())
    }
}

impl std::error::Error for PluginError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_plugin_id_and_source() {
        let err = PluginError {
            plugin_id: "buggy".into(),
            source: "leviathan.ui.slot.add".into(),
            message: "unknown section 'nope'".into(),
            lua_traceback: Some("[string \"plugins/buggy/init.lua\"]:5: in main chunk".into()),
        };
        let s = err.to_string();
        assert!(s.contains("plugin 'buggy'"), "got: {s}");
        assert!(s.contains("leviathan.ui.slot.add"), "got: {s}");
        assert!(s.contains("unknown section 'nope'"), "got: {s}");
        assert!(s.contains("init.lua:5"), "got: {s}");
    }

    #[test]
    fn display_omits_traceback_section_when_none() {
        let err = PluginError {
            plugin_id: "p".into(),
            source: "leviathan.fs.read_file".into(),
            message: "boom".into(),
            lua_traceback: None,
        };
        let s = err.to_string();
        assert!(
            !s.contains("traceback"),
            "should not mention traceback when None: {s}"
        );
    }
}

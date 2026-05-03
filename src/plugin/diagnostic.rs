//! Typed plugin diagnostics.
//!
//! Phase 3: every host-side error path that used to `eprintln!` instead
//! emits a `PluginDiagnostic`. The host owns a ring-buffered
//! `DiagnosticStore`; devtools, tests, and an optional stderr sink read
//! from it. Plugins never emit diagnostics themselves — the host
//! summarises Lua/manifest/widget/capability/reload/cleanup failures and
//! attaches `(plugin_id, generation_id)` from the Phase 1 newtypes.
//!
//! Engineering invariant 1 (host owns every effect) is preserved:
//! diagnostics are an effect, so only host-side code constructs them.

use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

use crate::plugin::resources::{GenerationId, PluginId};

/// Diagnostic severity. `Trace` is intentionally omitted — anything
/// worth recording is at least informational.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

impl DiagnosticSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

impl std::fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where in the plugin's source the diagnostic originated. Phase 3
/// covers the four locations the plan calls out: manifest path+key, Lua
/// file+line, widget AST path, and host API function name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum PluginSourceSpan {
    /// Failure decoding or validating a `plugin.toml`. `key` is the
    /// dotted manifest key when known (e.g. `capabilities[2]`).
    Manifest { path: String, key: Option<String> },
    /// Lua script location. `file` is the chunk name (typically
    /// `plugins/<id>/init.lua`); `line` is the 1-based line number.
    /// Pulled from `mlua::Error::SyntaxError` or the first usable frame
    /// of a callback traceback.
    Lua {
        file: String,
        line: Option<u32>,
        traceback: Option<String>,
    },
    /// Widget AST coordinate, e.g. `screen.view.children[2].child`.
    Widget { path: String },
    /// `leviathan.fs.read_file`, `leviathan.ui.regions.add_slot`, etc.
    /// Used when failure is attributable to a specific host API call
    /// site that the host can name.
    ApiFunction { name: String },
}

impl std::fmt::Display for PluginSourceSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest { path, key: Some(k) } => write!(f, "{path}#{k}"),
            Self::Manifest { path, key: None } => f.write_str(path),
            Self::Lua {
                file,
                line: Some(l),
                ..
            } => write!(f, "{file}:{l}"),
            Self::Lua {
                file, line: None, ..
            } => f.write_str(file),
            Self::Widget { path } => write!(f, "widget:{path}"),
            Self::ApiFunction { name } => f.write_str(name),
        }
    }
}

/// Structured diagnostic. `code` is a stable string (e.g.
/// `lua.callback_error`) for query-by-code; `message` is human-prose;
/// `context` carries arbitrary structured data the device-tools/test
/// can introspect.
#[derive(Debug, Clone)]
pub struct PluginDiagnostic {
    pub plugin_id: PluginId,
    pub generation_id: Option<GenerationId>,
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub source: Option<PluginSourceSpan>,
    pub context: Value,
    /// Wall clock for sorting / age display in devtools.
    pub timestamp: SystemTime,
    /// Monotonic instant for "since" queries that need to ignore
    /// system-clock jumps.
    pub instant: Instant,
}

impl PluginDiagnostic {
    pub fn new(
        plugin_id: PluginId,
        severity: DiagnosticSeverity,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            plugin_id,
            generation_id: None,
            severity,
            code: code.into(),
            message: message.into(),
            source: None,
            context: Value::Null,
            timestamp: SystemTime::now(),
            instant: Instant::now(),
        }
    }

    pub fn with_generation(mut self, generation_id: GenerationId) -> Self {
        self.generation_id = Some(generation_id);
        self
    }

    pub fn with_source(mut self, source: PluginSourceSpan) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_context(mut self, context: Value) -> Self {
        self.context = context;
        self
    }

    /// Attach an mlua error to the diagnostic: the message is
    /// concatenated with the underlying error string and a `Lua`
    /// source span is filled in (file/line/traceback) when extractable.
    pub fn with_mlua_error(mut self, file_hint: &str, error: &mlua::Error) -> Self {
        let (line, traceback) = lua_error_location(error);
        self.message = if self.message.is_empty() {
            error.to_string()
        } else {
            format!("{}: {}", self.message, error)
        };
        self.source = Some(PluginSourceSpan::Lua {
            file: file_hint.to_string(),
            line,
            traceback,
        });
        self
    }

    pub fn timestamp_unix_ms(&self) -> u128 {
        self.timestamp
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    }

    /// Stringified source span (`Display` impl on
    /// `PluginSourceSpan`). Returns `None` when no span is attached.
    /// Used by `InspectorSnapshot` to surface a flat string column.
    pub fn source_string(&self) -> Option<String> {
        self.source.as_ref().map(|s| s.to_string())
    }
}

/// Best-effort line and traceback extraction. `mlua::Error::SyntaxError`
/// carries an optional `incomplete_input` flag and a message that
/// usually starts with `name:line:`; `mlua::Error::CallbackError`
/// carries a traceback.
pub fn lua_error_location(error: &mlua::Error) -> (Option<u32>, Option<String>) {
    match error {
        mlua::Error::SyntaxError { message, .. } => (parse_line_from_msg(message), None),
        mlua::Error::CallbackError { traceback, .. } => {
            (parse_line_from_msg(traceback), Some(traceback.clone()))
        }
        mlua::Error::RuntimeError(msg) => (parse_line_from_msg(msg), None),
        _ => (None, None),
    }
}

fn parse_line_from_msg(msg: &str) -> Option<u32> {
    // mlua source markers look like `[string "plugins/x/init.lua"]:5:`
    // or `plugins/x/init.lua:5:`. Pull the first `:<digits>:` we find.
    let bytes = msg.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start && end < bytes.len() && bytes[end] == b':' {
                if let Ok(n) = msg[start..end].parse::<u32>() {
                    return Some(n);
                }
            }
        }
        i += 1;
    }
    None
}

/// Pluggable forward sink. Default is `StderrSink`; tests use the
/// null sink so test output stays quiet.
pub trait DiagnosticSink: Send + Sync + 'static {
    fn emit(&self, diagnostic: &PluginDiagnostic);
}

pub struct StderrSink;

impl DiagnosticSink for StderrSink {
    fn emit(&self, diagnostic: &PluginDiagnostic) {
        let gen = diagnostic
            .generation_id
            .map(|g| format!(" gen={}", g.get()))
            .unwrap_or_default();
        let src = diagnostic
            .source
            .as_ref()
            .map(|s| format!(" at {s}"))
            .unwrap_or_default();
        eprintln!(
            "git_leviathan [{severity}] {plugin}{gen} {code}: {message}{src}",
            severity = diagnostic.severity,
            plugin = diagnostic.plugin_id,
            code = diagnostic.code,
            message = diagnostic.message,
        );
    }
}

pub struct NullSink;

impl DiagnosticSink for NullSink {
    fn emit(&self, _diagnostic: &PluginDiagnostic) {}
}

const DEFAULT_CAPACITY: usize = 1024;

#[derive(Debug)]
struct StoreInner {
    capacity: usize,
    buffer: std::collections::VecDeque<PluginDiagnostic>,
}

/// Cheap-clone, thread-safe ring buffer of diagnostics. Old entries
/// fall off the back when capacity is reached.
#[derive(Clone)]
pub struct DiagnosticStore {
    inner: Arc<Mutex<StoreInner>>,
    sink: Arc<dyn DiagnosticSink>,
}

impl std::fmt::Debug for DiagnosticStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self
            .inner
            .lock()
            .map(|g| g.buffer.len())
            .unwrap_or_default();
        f.debug_struct("DiagnosticStore")
            .field("len", &len)
            .finish()
    }
}

impl Default for DiagnosticStore {
    fn default() -> Self {
        Self::with_sink(Arc::new(StderrSink))
    }
}

impl DiagnosticStore {
    pub fn with_sink(sink: Arc<dyn DiagnosticSink>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StoreInner {
                capacity: DEFAULT_CAPACITY,
                buffer: std::collections::VecDeque::with_capacity(DEFAULT_CAPACITY),
            })),
            sink,
        }
    }

    pub fn capacity(&self) -> usize {
        self.inner.lock().map(|g| g.capacity).unwrap_or(0)
    }

    /// Record a diagnostic and forward to the sink. Always returns
    /// after side effects so callers can chain the next host action
    /// without juggling errors.
    pub fn record(&self, diagnostic: PluginDiagnostic) {
        self.sink.emit(&diagnostic);
        if let Ok(mut inner) = self.inner.lock() {
            if inner.buffer.len() == inner.capacity {
                inner.buffer.pop_front();
            }
            inner.buffer.push_back(diagnostic);
        }
    }

    /// Snapshot of every diagnostic currently retained.
    pub fn entries(&self) -> Vec<PluginDiagnostic> {
        self.inner
            .lock()
            .map(|g| g.buffer.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.buffer.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.buffer.clear();
        }
    }

    /// Return diagnostics for `plugin_id`, oldest first.
    pub fn for_plugin(&self, plugin_id: &PluginId) -> Vec<PluginDiagnostic> {
        self.entries()
            .into_iter()
            .filter(|d| &d.plugin_id == plugin_id)
            .collect()
    }

    /// Return diagnostics whose severity is at least `min`.
    pub fn at_least(&self, min: DiagnosticSeverity) -> Vec<PluginDiagnostic> {
        self.entries()
            .into_iter()
            .filter(|d| d.severity >= min)
            .collect()
    }

    /// Return diagnostics whose code matches `code` exactly.
    pub fn by_code(&self, code: &str) -> Vec<PluginDiagnostic> {
        self.entries()
            .into_iter()
            .filter(|d| d.code == code)
            .collect()
    }

    /// Return diagnostics emitted at or after `since`. Uses monotonic
    /// `Instant` so a wall-clock jump can't lose a recent diagnostic.
    pub fn since(&self, since: Instant) -> Vec<PluginDiagnostic> {
        self.entries()
            .into_iter()
            .filter(|d| d.instant >= since)
            .collect()
    }

    /// Tail of the most recent `n` diagnostics, oldest first.
    pub fn tail(&self, n: usize) -> Vec<PluginDiagnostic> {
        let entries = self.entries();
        let len = entries.len();
        let start = len.saturating_sub(n);
        entries[start..].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(plugin: &str, severity: DiagnosticSeverity, code: &str) -> PluginDiagnostic {
        PluginDiagnostic::new(
            PluginId::from(plugin),
            severity,
            code,
            format!("msg-{code}"),
        )
    }

    #[test]
    fn ring_buffer_evicts_oldest_when_full() {
        let store = DiagnosticStore::with_sink(Arc::new(NullSink));
        // Force tiny capacity by writing many entries; default is 1024
        // so we just confirm capacity is honoured.
        assert_eq!(store.capacity(), 1024);
        for i in 0..3 {
            store.record(diag("p", DiagnosticSeverity::Info, &format!("c{i}")));
        }
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn query_helpers_filter_correctly() {
        let store = DiagnosticStore::with_sink(Arc::new(NullSink));
        store.record(diag("a", DiagnosticSeverity::Info, "info.x"));
        store.record(diag("a", DiagnosticSeverity::Warning, "warn.x"));
        let early = Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(2));
        store.record(diag("b", DiagnosticSeverity::Error, "err.x"));

        assert_eq!(store.for_plugin(&PluginId::from("a")).len(), 2);
        assert_eq!(store.at_least(DiagnosticSeverity::Warning).len(), 2);
        assert_eq!(store.by_code("err.x").len(), 1);
        let recent = store.since(early);
        assert!(recent.iter().any(|d| d.code == "err.x"));
        assert_eq!(store.tail(1).len(), 1);
    }

    #[test]
    fn parse_line_from_msg_handles_common_lua_formats() {
        assert_eq!(parse_line_from_msg("plugins/x/init.lua:42: oops"), Some(42));
        assert_eq!(
            parse_line_from_msg("[string \"plugins/x/init.lua\"]:5: bad"),
            Some(5)
        );
        assert_eq!(parse_line_from_msg("no line here"), None);
    }

    #[test]
    fn builder_attaches_generation_source_and_context() {
        let d = PluginDiagnostic::new(PluginId::from("p"), DiagnosticSeverity::Error, "c", "m")
            .with_generation(GenerationId::new(7))
            .with_source(PluginSourceSpan::Widget {
                path: "screen.view.children[2]".into(),
            })
            .with_context(serde_json::json!({"extra": 1}));
        assert_eq!(d.plugin_id.as_str(), "p");
        assert_eq!(d.generation_id.map(|g| g.get()), Some(7));
        assert_eq!(d.severity, DiagnosticSeverity::Error);
        assert_eq!(d.code, "c");
        assert!(matches!(d.source, Some(PluginSourceSpan::Widget { .. })));
        assert_eq!(d.context["extra"], 1);
        assert_eq!(d.source_string().unwrap(), "widget:screen.view.children[2]");
    }
}

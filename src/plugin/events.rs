//! Phase 7 typed-event registry and autocmd dispatch funnel.
//!
//! Every fire of a host event flows through [`EventBus::dispatch`].
//! That includes:
//! - `PluginHost::fire_event(name)`, which constructs an empty payload
//!   and routes through the funnel.
//! - The new `PluginHost::fire_event_typed(name, payload)` API that
//!   the app uses to fire fully-typed Phase 7 events.
//! - The test-only `PluginHost::dispatch_test_event(name, payload)`
//!   replay hook (it shares the same funnel — no shortcut).
//!
//! Responsibilities owned here (Invariant 1: the host owns every
//! effect):
//! - validate event name against the descriptor table;
//! - check the pattern glob against any string field on the payload;
//! - apply per-autocmd debounce against the host-owned virtual clock;
//! - drop `once = true` entries after they fire;
//! - count consecutive failures per autocmd, disabling at the
//!   `MAX_CONSECUTIVE_FAILURES` threshold and emitting a structured
//!   diagnostic;
//!
//! Autocmd entries are owned by `(plugin_id, generation_id)` (Invariant
//! 2). The resource ledger drives cleanup; on reload swap, the host
//! splices staged entries in and the previous generation's are
//! reaped through the ledger walk.

use std::collections::HashMap;

use git_leviathan_plugin_api::descriptor::api as event_descriptor;
use mlua::RegistryKey;

use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::resources::{GenerationId, PluginId};

/// Threshold at which a misbehaving autocmd is auto-disabled. Five is
/// large enough to absorb transient Lua errors (e.g. a momentarily
/// missing table field) yet small enough that a permanently broken
/// callback stops spamming the diagnostic store after a handful of
/// fires.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;

/// Stable opaque autocmd id. Allocated host-side, surfaced into
/// devtools, never exposed to plugin Lua (plugins refer to their
/// autocmds by group handle instead).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AutocmdId(u64);

impl AutocmdId {
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Stable autocmd-group handle. Returned to plugin Lua as a plain
/// integer so groups round-trip through `vim.api`-style inspection
/// without needing userdata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(u64);

impl GroupId {
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Plugin-supplied autocmd configuration after decoding from Lua.
/// Defaults are the same shape the descriptor advertises.
#[derive(Debug, Clone, Default)]
pub struct AutocmdOptions {
    pub group: Option<GroupId>,
    pub once: bool,
    pub pattern: Option<String>,
    pub debounce_ms: u64,
    pub priority: i32,
    pub source_location: Option<String>,
}

/// Live autocmd registration. Owned host-side; the `callback` registry
/// key references a Lua function on the plugin's per-generation Lua
/// state.
///
/// `event` is the exact canonical event name the plugin passed to
/// `leviathan.autocmd.create`. `canonical_event` mirrors it and drives
/// payload schema validation and the devtools row's primary event
/// column.
pub struct AutocmdEntry {
    pub id: AutocmdId,
    pub plugin_id: String,
    pub generation_id: GenerationId,
    pub group: Option<GroupId>,
    pub event: &'static str,
    pub canonical_event: &'static str,
    pub callback: RegistryKey,
    pub options: AutocmdOptions,
    pub runtime: AutocmdRuntime,
}

/// Mutable counters and disable state. Kept in its own struct so that
/// [`AutocmdEntry`] can be cloned into a devtools summary without
/// mutating it.
#[derive(Debug, Clone, Default)]
pub struct AutocmdRuntime {
    pub fires: u64,
    pub failures: u32,
    pub consecutive_failures: u32,
    pub disabled: bool,
    pub last_fire_clock_ms: Option<u64>,
}

/// Host-side payload value. Always a JSON object so the typed
/// `payload` table the Lua callback sees has a stable shape (the
/// descriptor's `payload_fields` define the keys).
pub type EventPayload = serde_json::Map<String, serde_json::Value>;

/// Result of one Lua callback invocation. Public so the host can
/// fold the per-call outcome into ledger / dispatch bookkeeping.
/// Failure causes are recorded as structured diagnostics inside
/// the funnel — this enum only signals which counter to bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    Ok,
    /// The callback raised. The funnel records a structured
    /// diagnostic; the host bumps the autocmd's failure counters.
    Error,
    /// The callback didn't run (debounced, disabled, pattern
    /// rejected, plugin missing). Failure counters untouched.
    Skipped,
}

/// Owns the mutable autocmd table plus monotonically-increasing id /
/// group / clock counters. Phase 7's "single funnel" lives on this
/// type via [`EventBus::dispatch`].
#[derive(Default)]
pub struct EventBus {
    entries: Vec<AutocmdEntry>,
    next_entry_id: u64,
    groups: HashMap<(String, String), GroupId>,
    next_group_id: u64,
    /// Virtual host clock used for debounce. Tests advance it
    /// explicitly via [`EventBus::advance_clock`]; production wall-
    /// clock advancement is driven by [`EventBus::set_real_clock_ms`]
    /// from the host's tick path so debounce decisions stay
    /// deterministic regardless of system-clock jumps.
    clock_ms: u64,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[AutocmdEntry] {
        &self.entries
    }

    pub fn entries_mut(&mut self) -> &mut Vec<AutocmdEntry> {
        &mut self.entries
    }

    pub fn clock_ms(&self) -> u64 {
        self.clock_ms
    }

    /// Advance the virtual clock. Tests use this to drive debounce
    /// behaviour deterministically without real `std::thread::sleep`.
    pub fn advance_clock(&mut self, delta_ms: u64) {
        self.clock_ms = self.clock_ms.saturating_add(delta_ms);
    }

    fn alloc_entry_id(&mut self) -> AutocmdId {
        self.next_entry_id += 1;
        AutocmdId(self.next_entry_id)
    }

    /// Look up or allocate a group handle for `(plugin_id, name)`.
    /// Honours `clear = true` by removing every entry currently in
    /// that group before returning the handle.
    pub fn group_handle(&mut self, plugin_id: &str, name: &str, clear: bool) -> GroupId {
        let key = (plugin_id.to_string(), name.to_string());
        let id = if let Some(existing) = self.groups.get(&key) {
            *existing
        } else {
            self.next_group_id += 1;
            let id = GroupId(self.next_group_id);
            self.groups.insert(key, id);
            id
        };
        if clear {
            self.clear_group(plugin_id, id);
        }
        id
    }

    /// Drop every autocmd that lives in `group` for `plugin_id`.
    /// Returns the removed entries so the caller can deal with the
    /// `RegistryKey`s (free them through the ledger / lua state).
    pub fn clear_group(&mut self, plugin_id: &str, group: GroupId) -> Vec<AutocmdEntry> {
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            let matches = {
                let entry = &self.entries[i];
                entry.plugin_id == plugin_id && entry.group == Some(group)
            };
            if matches {
                removed.push(self.entries.remove(i));
            } else {
                i += 1;
            }
        }
        removed
    }

    /// Push a new autocmd row. Returns the allocated id. Sorting by
    /// `(priority desc, id asc)` happens lazily in [`Self::dispatch`]
    /// so insertion is O(1).
    pub fn register(
        &mut self,
        plugin_id: String,
        generation_id: GenerationId,
        subscribed_event: &'static str,
        canonical_event: &'static str,
        callback: RegistryKey,
        options: AutocmdOptions,
    ) -> AutocmdId {
        let id = self.alloc_entry_id();
        self.entries.push(AutocmdEntry {
            id,
            plugin_id,
            generation_id,
            group: options.group,
            event: subscribed_event,
            canonical_event,
            callback,
            options,
            runtime: AutocmdRuntime::default(),
        });
        id
    }

    /// Drop every autocmd owned by `plugin_id` regardless of
    /// generation. Used by `unload_plugin`.
    pub fn drop_for_plugin(&mut self, plugin_id: &str) -> Vec<AutocmdEntry> {
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].plugin_id == plugin_id {
                removed.push(self.entries.remove(i));
            } else {
                i += 1;
            }
        }
        self.groups.retain(|(pid, _), _| pid != plugin_id);
        removed
    }
}

/// Resolve a canonical event name to its descriptor.
///
/// Returns `Err(name)` for unknown events; the caller turns that
/// into an `autocmd.invalid_event` diagnostic at the registration
/// site.
pub fn resolve_event(
    name: &str,
) -> Result<(&'static event_descriptor::ApiEvent, Option<&'static str>), String> {
    let Some(descriptor) = event_descriptor::event_descriptor(name) else {
        return Err(name.to_string());
    };
    Ok((descriptor, None))
}

/// Validate the payload against the canonical event's
/// `payload_fields`. Records `autocmd.payload_mismatch` diagnostics
/// for every missing required field or shape error and returns false
/// when at least one error was recorded. The dispatch funnel still
/// proceeds — the diagnostic is informational because plugins are
/// allowed to pattern-match on what they get and the test suite's
/// replay hook deliberately exercises this path.
pub fn validate_payload(
    descriptor: &event_descriptor::ApiEvent,
    payload: &EventPayload,
    diagnostics: &DiagnosticStore,
) -> bool {
    let mut ok = true;
    for field in descriptor.payload_fields {
        match payload.get(field.name) {
            Some(value) => {
                if !lua_type_matches(field.lua_type, value) {
                    ok = false;
                    diagnostics.record(
                        PluginDiagnostic::new(
                            PluginId::from("<host>"),
                            DiagnosticSeverity::Warning,
                            "autocmd.payload_mismatch",
                            format!(
                                "event {} payload field `{}` should be {}, got {}",
                                descriptor.name,
                                field.name,
                                field.lua_type,
                                value_kind(value)
                            ),
                        )
                        .with_source(PluginSourceSpan::ApiFunction {
                            name: format!("event:{}", descriptor.name),
                        }),
                    );
                }
            }
            None if field.required => {
                ok = false;
                diagnostics.record(
                    PluginDiagnostic::new(
                        PluginId::from("<host>"),
                        DiagnosticSeverity::Warning,
                        "autocmd.payload_mismatch",
                        format!(
                            "event {} payload missing required field `{}`",
                            descriptor.name, field.name
                        ),
                    )
                    .with_source(PluginSourceSpan::ApiFunction {
                        name: format!("event:{}", descriptor.name),
                    }),
                );
            }
            None => {}
        }
    }
    ok
}

fn lua_type_matches(declared: &str, value: &serde_json::Value) -> bool {
    // Tolerate the descriptor's union types (`string|nil`) by
    // splitting on `|` and accepting the value when any branch
    // matches.
    declared.split('|').any(|branch| {
        let branch = branch.trim();
        match branch {
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "table" => value.is_object() || value.is_array(),
            "nil" => value.is_null(),
            _ => true, // unknown type tag — be permissive
        }
    })
}

fn value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "nil",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "table",
    }
}

/// Order entries by (priority desc, id asc) so a higher-priority
/// autocmd runs first and ties stay in declaration order. Returns
/// indices into the original `entries` slice; callers iterate by
/// borrowed entry rather than cloning.
pub fn dispatch_order(entries: &[AutocmdEntry]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..entries.len()).collect();
    indices.sort_by(|a, b| {
        let ea = &entries[*a];
        let eb = &entries[*b];
        eb.options
            .priority
            .cmp(&ea.options.priority)
            .then(ea.id.0.cmp(&eb.id.0))
    });
    indices
}

/// Glob match: `*` matches any run of characters, `?` one character,
/// everything else literal. Plain ASCII; no character classes.
pub fn glob_match(pattern: &str, value: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), value.as_bytes())
}

fn glob_match_bytes(pattern: &[u8], value: &[u8]) -> bool {
    // Iterative algorithm with backtracking; O(pattern.len * value.len)
    // worst case which is fine for short branch / path patterns.
    let (mut p, mut v) = (0usize, 0usize);
    let (mut star_p, mut star_v): (Option<usize>, usize) = (None, 0);
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star_p = Some(p);
            star_v = v;
            p += 1;
        } else if let Some(sp) = star_p {
            p = sp + 1;
            star_v += 1;
            v = star_v;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

/// True when the entry's pattern (if any) matches some string field
/// in the payload. No pattern == always match. A pattern with no
/// matching field == no match.
pub fn pattern_matches(entry: &AutocmdEntry, payload: &EventPayload) -> bool {
    let Some(pattern) = entry.options.pattern.as_deref() else {
        return true;
    };
    payload
        .values()
        .filter_map(|v| v.as_str())
        .any(|s| glob_match(pattern, s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_basic() {
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("feature/*", "feature/x"));
        assert!(glob_match("feature/*", "feature/x/y"));
        assert!(!glob_match("feature/*", "main"));
        assert!(glob_match("v?.0", "v1.0"));
        assert!(!glob_match("v?.0", "v10.0"));
        assert!(glob_match("a*c", "abbbbbc"));
        assert!(!glob_match("a*c", "abbbbb"));
    }

    #[test]
    fn resolve_event_accepts_canonical_names_only() {
        let (canonical, alias) = resolve_event("FetchStarted").expect("canonical known");
        assert_eq!(canonical.name, "FetchStarted");
        assert!(alias.is_none());

        assert!(resolve_event("FetchStart").is_err());
        assert!(resolve_event("DefinitelyNotAnEvent").is_err());
    }

    #[test]
    fn lua_type_matches_basic() {
        assert!(lua_type_matches("string", &serde_json::json!("x")));
        assert!(!lua_type_matches("string", &serde_json::json!(1)));
        assert!(lua_type_matches("integer", &serde_json::json!(7)));
        assert!(lua_type_matches("table", &serde_json::json!({"a": 1})));
        assert!(lua_type_matches("string|nil", &serde_json::Value::Null));
    }

    #[test]
    fn dispatch_order_priority_then_id() {
        let entries = vec![
            AutocmdEntry {
                id: AutocmdId(1),
                plugin_id: "p".into(),
                generation_id: GenerationId::new(1),
                group: None,
                event: "X",
                canonical_event: "X",
                callback: dummy_key(),
                options: AutocmdOptions {
                    priority: 0,
                    ..Default::default()
                },
                runtime: AutocmdRuntime::default(),
            },
            AutocmdEntry {
                id: AutocmdId(2),
                plugin_id: "p".into(),
                generation_id: GenerationId::new(1),
                group: None,
                event: "X",
                canonical_event: "X",
                callback: dummy_key(),
                options: AutocmdOptions {
                    priority: 10,
                    ..Default::default()
                },
                runtime: AutocmdRuntime::default(),
            },
        ];
        let order = dispatch_order(&entries);
        assert_eq!(order, vec![1, 0]);
    }

    fn dummy_key() -> RegistryKey {
        // A real RegistryKey would require an mlua::Lua; tests that
        // need the real thing build it through the host. The order
        // helper only reads ids/priority, not the key, so we
        // synthesise one through a private Lua state.
        let lua = mlua::Lua::new();
        lua.create_registry_value(true).unwrap()
    }
}

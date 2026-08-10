//! unified command registry.
//!
//! One [`CommandRegistry`] holds every command: built-in host commands
//! (registered at startup with [`HOST_COMMAND_PLUGIN_ID`]) and plugin
//! commands (registered through `leviathan.command.create` Lua calls).
//! Both kinds dispatch through [`CommandRegistry::invoke`].
//!
//! Engineering invariants honoured here:
//!
//! 1. **Host owns every effect.** Lua never directly calls another
//!    plugin's command; it calls `leviathan.command.invoke`, which
//!    routes through this registry where the host validates args,
//!    checks capabilities, runs the body, and emits diagnostics +
//!    typed events.
//! 2. **Generation-scoped ownership.** Plugin commands are keyed by
//!    `(plugin_id, generation_id, name)` and dropped on unload/reload.
//! 6. **Capabilities at use time.** A command's declared
//!    `capabilities` are checked at invocation time against a
//!    plugin-specific allow predicate; denials are recorded as
//!    `command.capability_denied` diagnostics + audit entries.
//! 7. **Devtools required.** [`CommandRegistry::summaries`] feeds the
//!    `commands` row on `InspectorSnapshot` and the headless palette.
//!
//! Diagnostic codes emitted from this module:
//!
//! - `command.registered` (info) — a command was added.
//! - `command.invoked` (info) — invocation started.
//! - `command.completed` (info, with `context.duration_ms`) — body
//!   returned successfully.
//! - `command.failed` (error, with `context.duration_ms` and
//!   `context.cause`) — body raised or runner reported failure.
//! - `command.invalid_args` (warning) — caller's args failed schema
//!   validation; body did NOT run.
//! - `command.capability_denied` (error) — declared capability not
//!   granted for this plugin; body did NOT run.
//! - `command.not_found` (warning) — caller invoked an unknown name.
//! - `command.duplicate_registration` (warning) — same plugin
//!   registered the same name twice; the later one wins.
//!
//! After every invocation, the registry asks its caller-supplied
//! event sink to fire the typed `CommandExecuted` autocmd event with
//! `{ name, plugin_id, ok, duration_ms }` — both success and failure
//! paths emit it so subscribers see every dispatch.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use mlua::{Function, Lua, RegistryKey};

use crate::plugin::diagnostic::{
    DiagnosticSeverity, DiagnosticStore, PluginDiagnostic, PluginSourceSpan,
};
use crate::plugin::resources::{GenerationId, PluginId};

/// Sentinel plugin id under which built-in host commands register.
/// Surfaced verbatim in palette / devtools rows so the source of every
/// command is unambiguous.
pub const HOST_COMMAND_PLUGIN_ID: &str = "<host>";

/// Argument type tag carried by [`CommandArg::ty`]. Keep this enum in
/// lockstep with the `LeviathanCommandArg.type` descriptor field — it's
/// the validation contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandArgType {
    String,
    Boolean,
    Integer,
    Number,
    /// Enum with the listed allowed string values. Encoded as
    /// `enum:a,b,c` in Lua.
    Enum(Vec<String>),
}

impl CommandArgType {
    /// Parse the descriptor string (`"string"`, `"boolean"`,
    /// `"integer"`, `"number"`, `"enum:a,b,c"`). Strict — anything
    /// else returns `Err` with a stable error message used in the
    /// `command.invalid_args` diagnostic.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "string" => Ok(Self::String),
            "boolean" => Ok(Self::Boolean),
            "integer" => Ok(Self::Integer),
            "number" => Ok(Self::Number),
            other if other.starts_with("enum:") => {
                let rest = &other[5..];
                if rest.is_empty() {
                    return Err("enum type requires at least one value (enum:a,b,c)".into());
                }
                let values: Vec<String> = rest
                    .split(',')
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                    .collect();
                if values.is_empty() {
                    return Err("enum type requires at least one non-empty value".into());
                }
                Ok(Self::Enum(values))
            }
            other => Err(format!(
                "unknown arg type `{other}`; expected string, boolean, integer, number, or enum:..."
            )),
        }
    }

    pub fn as_descriptor(&self) -> String {
        match self {
            Self::String => "string".into(),
            Self::Boolean => "boolean".into(),
            Self::Integer => "integer".into(),
            Self::Number => "number".into(),
            Self::Enum(vs) => format!("enum:{}", vs.join(",")),
        }
    }
}

/// One arg in a command's schema.
#[derive(Debug, Clone)]
pub struct CommandArg {
    pub name: String,
    pub ty: CommandArgType,
    /// True when the caller MUST supply the arg and no default exists.
    /// A `default = ...` entry implicitly satisfies an otherwise-required
    /// arg.
    pub required: bool,
    /// Default value substituted when the caller omits the arg.
    /// Validated against `ty` when the descriptor is constructed so a
    /// bad default surfaces as a `command.duplicate_registration`
    /// diagnostic at registration time, not later at invocation.
    pub default: Option<serde_json::Value>,
    pub doc: String,
}

/// Static metadata about an activation context. keymaps will route
/// keymap dispatch through these context names; for now we treat them
/// as opaque strings and carry the descriptor list to keep the palette
/// honest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommandContext;

impl CommandContext {
    /// Default context when a command's `context` is unset.
    pub const GLOBAL: &'static str = "global";
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAvailability {
    pub enabled: bool,
    pub reason: Option<String>,
}

impl CommandAvailability {
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            reason: None,
        }
    }

    pub fn disabled(reason: impl Into<String>) -> Self {
        Self {
            enabled: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandHooks {
    pub before: bool,
    pub after: bool,
    pub veto: bool,
    pub replace: bool,
}

impl CommandHooks {
    pub fn result_event_only() -> Self {
        Self {
            before: false,
            after: true,
            veto: false,
            replace: false,
        }
    }
}

/// One command — descriptor + body + ownership info.
pub struct CommandDescriptor {
    pub name: String,
    pub title: String,
    pub description: String,
    pub plugin_id: String,
    pub generation_id: Option<GenerationId>,
    pub context: String,
    pub args: Vec<CommandArg>,
    pub destructive: bool,
    pub capabilities: Vec<String>,
    pub plugin_invocation_capabilities: Vec<String>,
    pub availability: CommandAvailability,
    pub keymap_eligible: bool,
    pub palette_visible: bool,
    pub hooks: CommandHooks,
    pub run: CommandBody,
}

type HostCommandCallback = dyn Fn(&serde_json::Value) -> Result<(), String>;

/// A command body. Lua bodies live on the owning plugin's Lua state;
/// host bodies are plain closures. Either way, dispatch goes through
/// [`CommandRunner`] so the registry never knows about Lua details.
pub enum CommandBody {
    Lua { callback: RegistryKey },
    Host(Box<HostCommandCallback>),
}

/// What happened when [`CommandRegistry::invoke`] dispatched. Returned
/// to callers (the Lua API, palette, tests) so they can drive UI off
/// the same dispatch path the host emits diagnostics on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeOutcome {
    Ok,
    InvalidArgs(Vec<String>),
    CapabilityDenied(String),
    ContextDenied(String),
    NotFound,
    /// The body returned an error (Lua raised, or a host runner
    /// returned `Err`). The string is the runner-reported cause.
    Failed(String),
}

impl InvokeOutcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }
}

/// Counters and last-outcome state per command. Cloned into the
/// devtools snapshot.
#[derive(Debug, Clone, Default)]
pub struct CommandRuntime {
    pub fires: u64,
    pub failures: u64,
    pub last_outcome: Option<String>,
    pub last_invoked_at_unix_ms: u128,
    pub last_duration_ms: u128,
}

/// Registry entry: descriptor + mutable runtime counters.
pub struct CommandEntry {
    pub descriptor: CommandDescriptor,
    pub runtime: CommandRuntime,
}

/// Trait the registry leans on at invocation time to:
///
/// - resolve a Lua callback's owning plugin's `Lua` state;
/// - check whether a plugin holds the named capability;
/// - emit the typed `CommandExecuted` event;
/// - record audit entries for capability checks (mirrors typed diagnostics).
///
/// The host implements this through a thin shim in `host.rs`. Tests
/// implement a no-op variant so the registry can be exercised without
/// a full PluginHost.
pub trait CommandRunner {
    /// Run the descriptor's body with the validated args. The args
    /// are already merged with defaults — the runner sees a complete
    /// table.
    fn run(&mut self, entry: &CommandEntry, args: &serde_json::Value) -> Result<(), String>;

    /// Capability check. Return `Ok(())` when granted, `Err(reason)`
    /// when denied. Implementations should also emit an audit-log
    /// entry so the security trail is complete.
    fn check_capability(&mut self, plugin_id: &str, capability: &str) -> Result<(), String>;

    fn check_invocation_capability(
        &mut self,
        command_name: &str,
        capability: &str,
    ) -> Result<(), String>;

    fn check_invocation_context(
        &mut self,
        command_name: &str,
        command_context: &str,
    ) -> Result<(), String>;

    /// Fire the typed `CommandExecuted` event. The registry calls this
    /// after every dispatch attempt that reached the body (i.e. not
    /// for `NotFound` / `InvalidArgs` / `CapabilityDenied`).
    fn emit_command_executed(&mut self, name: &str, plugin_id: &str, ok: bool, duration_ms: u128);
}

/// Single registry shared between the host and the Lua APIs. Stored
/// inside the host as `Rc<RefCell<…>>` so the Lua-side
/// `leviathan.command.create` shim can borrow it mutably during
/// init.lua execution while the host holds a clone for dispatch.
#[derive(Default)]
pub struct CommandRegistry {
    /// Order is preserved so the palette and `list()` output sort
    /// stably by registration order within (plugin_id, generation_id).
    /// Lookups are linear over the small command set — every gate
    /// (palette filter, dispatch) walks the whole list anyway.
    entries: Vec<CommandEntry>,
    /// Identity of the command currently detached for dispatch, plus
    /// whether an unload/reload dropped it while it ran. Prevents the
    /// reattach from resurrecting a command a mid-dispatch
    /// `drop_for_plugin`/`drop_for_generation` already removed.
    detached: Option<DetachedDispatch>,
}

#[derive(Clone)]
struct DetachedDispatch {
    plugin_id: String,
    generation_id: Option<GenerationId>,
    dropped: bool,
}

impl DetachedDispatch {
    fn matches_plugin(&self, plugin_id: &str) -> bool {
        self.plugin_id == plugin_id
    }
    fn matches_generation(&self, plugin_id: &str, generation_id: GenerationId) -> bool {
        self.plugin_id == plugin_id && self.generation_id == Some(generation_id)
    }
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[CommandEntry] {
        &self.entries
    }

    /// Insert (or replace) a command. Same `(plugin_id, name)` ->
    /// later wins, with a `command.duplicate_registration` warning
    /// recorded against the new descriptor's plugin so reloaded plugins
    /// don't spam on the third+ load. Returns the index of the inserted
    /// entry.
    pub fn register(
        &mut self,
        descriptor: CommandDescriptor,
        diagnostics: &DiagnosticStore,
    ) -> usize {
        if let Some(index) = self.entries.iter().position(|e| {
            e.descriptor.plugin_id == descriptor.plugin_id && e.descriptor.name == descriptor.name
        }) {
            diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(descriptor.plugin_id.as_str()),
                    DiagnosticSeverity::Warning,
                    "command.duplicate_registration",
                    format!(
                        "command `{}` already registered for plugin `{}`; replacing",
                        descriptor.name, descriptor.plugin_id
                    ),
                )
                .with_source(PluginSourceSpan::ApiFunction {
                    name: format!("command:{}", descriptor.name),
                })
                .with_context(serde_json::json!({
                    "plugin_id": descriptor.plugin_id,
                    "name": descriptor.name,
                })),
            );
            self.entries[index] = CommandEntry {
                descriptor,
                runtime: CommandRuntime::default(),
            };
            index
        } else {
            diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(descriptor.plugin_id.as_str()),
                    DiagnosticSeverity::Info,
                    "command.registered",
                    format!(
                        "command `{}` registered (context: {})",
                        descriptor.name, descriptor.context
                    ),
                )
                .with_source(PluginSourceSpan::ApiFunction {
                    name: format!("command:{}", descriptor.name),
                }),
            );
            self.entries.push(CommandEntry {
                descriptor,
                runtime: CommandRuntime::default(),
            });
            self.entries.len() - 1
        }
    }

    /// Remove every entry owned by `plugin_id`. Used by `unload_plugin`.
    /// Returns the removed entries so the caller can free Lua
    /// `RegistryKey`s through the owning Lua state.
    pub fn drop_for_plugin(&mut self, plugin_id: &str) -> Vec<CommandEntry> {
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].descriptor.plugin_id == plugin_id {
                removed.push(self.entries.remove(i));
            } else {
                i += 1;
            }
        }
        if let Some(detached) = self.detached.as_mut() {
            if detached.matches_plugin(plugin_id) {
                detached.dropped = true;
            }
        }
        removed
    }

    /// Remove every entry owned by `(plugin_id, generation_id)`. Used
    /// during reload-commit before the new generation's commands are
    /// spliced in. Returns the removed entries for Lua key cleanup.
    pub fn drop_for_generation(
        &mut self,
        plugin_id: &str,
        generation_id: GenerationId,
    ) -> Vec<CommandEntry> {
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            let matches = self.entries[i].descriptor.plugin_id == plugin_id
                && self.entries[i].descriptor.generation_id == Some(generation_id);
            if matches {
                removed.push(self.entries.remove(i));
            } else {
                i += 1;
            }
        }
        if let Some(detached) = self.detached.as_mut() {
            if detached.matches_generation(plugin_id, generation_id) {
                detached.dropped = true;
            }
        }
        removed
    }

    /// Find a command by name. Multiple plugins may declare the same
    /// command name; this returns the first match in registration
    /// order. The palette and dispatch path treat `name` as the
    /// primary key — keymaps will resolve ambiguity by
    /// `(plugin_id, name)`.
    pub fn find(&self, name: &str) -> Option<&CommandEntry> {
        self.entries.iter().find(|e| e.descriptor.name == name)
    }

    /// Validate the caller's args against the descriptor schema.
    /// Returns the merged JSON value (defaults filled in) on success
    /// or a list of human-readable error strings on failure.
    ///
    /// Strict shape:
    /// - extra keys not in the schema -> error;
    /// - missing required keys with no default -> error;
    /// - wrong type -> error;
    /// - enum value not in allowed set -> error.
    pub fn validate_args(
        descriptor: &CommandDescriptor,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, Vec<String>> {
        let supplied = match args {
            serde_json::Value::Null => serde_json::Map::new(),
            serde_json::Value::Object(m) => m.clone(),
            other => {
                return Err(vec![format!(
                    "args must be a table or nil; got {}",
                    json_kind(other)
                )]);
            }
        };
        let mut errors: Vec<String> = Vec::new();
        let mut merged = serde_json::Map::new();

        // Reject extra keys.
        let known: std::collections::HashSet<&str> =
            descriptor.args.iter().map(|a| a.name.as_str()).collect();
        for key in supplied.keys() {
            if !known.contains(key.as_str()) {
                errors.push(format!("unexpected arg `{key}`"));
            }
        }

        for arg in &descriptor.args {
            match supplied.get(&arg.name) {
                Some(v) => match validate_value(&arg.ty, v) {
                    Ok(()) => {
                        merged.insert(arg.name.clone(), v.clone());
                    }
                    Err(e) => errors.push(format!("arg `{}`: {e}", arg.name)),
                },
                None => {
                    if let Some(default) = &arg.default {
                        merged.insert(arg.name.clone(), default.clone());
                    } else if arg.required {
                        errors.push(format!("missing required arg `{}`", arg.name));
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(serde_json::Value::Object(merged))
        } else {
            Err(errors)
        }
    }

    /// The single dispatch path. Used by the Lua API
    /// (`leviathan.command.invoke`), the palette, the `host.invoke_command`
    /// wrapper used by UI buttons / keymaps / tests — every entry point
    /// goes through this function.
    ///
    /// Returns the structured outcome and the dispatch duration so the
    /// caller can surface the same numbers the diagnostic uses.
    pub fn invoke(
        &mut self,
        name: &str,
        args: serde_json::Value,
        runner: &mut dyn CommandRunner,
        diagnostics: &DiagnosticStore,
        budget_tracker: &crate::plugin::performance::BudgetTracker,
    ) -> (InvokeOutcome, u128) {
        let Some(index) = self.entries.iter().position(|e| e.descriptor.name == name) else {
            record_command_not_found(diagnostics, name);
            return (InvokeOutcome::NotFound, 0);
        };

        invoke_entry(
            &mut self.entries[index],
            name,
            args,
            runner,
            diagnostics,
            budget_tracker,
        )
    }

    /// Project every entry into a stable summary the palette /
    /// devtools / Lua `list()` API can consume.
    pub fn summaries(&self) -> Vec<CommandSummary> {
        self.entries
            .iter()
            .map(|e| CommandSummary {
                name: e.descriptor.name.clone(),
                title: e.descriptor.title.clone(),
                description: e.descriptor.description.clone(),
                plugin_id: e.descriptor.plugin_id.clone(),
                generation_id: e.descriptor.generation_id.map(|g| g.get()),
                context: e.descriptor.context.clone(),
                destructive: e.descriptor.destructive,
                capabilities: e.descriptor.capabilities.clone(),
                plugin_invocation_capabilities: e.descriptor.plugin_invocation_capabilities.clone(),
                enabled: e.descriptor.availability.enabled,
                disabled_reason: e.descriptor.availability.reason.clone(),
                keymap_eligible: e.descriptor.keymap_eligible,
                palette_visible: e.descriptor.palette_visible,
                hooks: e.descriptor.hooks,
                args: e
                    .descriptor
                    .args
                    .iter()
                    .map(|a| CommandArgSummary {
                        name: a.name.clone(),
                        ty: a.ty.as_descriptor(),
                        required: a.required,
                        default: a.default.clone(),
                        doc: a.doc.clone(),
                    })
                    .collect(),
                fires: e.runtime.fires,
                failures: e.runtime.failures,
                last_outcome: e.runtime.last_outcome.clone(),
                last_duration_ms: e.runtime.last_duration_ms,
            })
            .collect()
    }
}

fn record_command_not_found(diagnostics: &DiagnosticStore, name: &str) {
    diagnostics.record(
        PluginDiagnostic::new(
            PluginId::from("<host>"),
            DiagnosticSeverity::Warning,
            "command.not_found",
            format!("command `{name}` not registered"),
        )
        .with_source(PluginSourceSpan::ApiFunction {
            name: format!("command:{name}"),
        }),
    );
}

fn invoke_entry(
    entry: &mut CommandEntry,
    name: &str,
    args: serde_json::Value,
    runner: &mut dyn CommandRunner,
    diagnostics: &DiagnosticStore,
    budget_tracker: &crate::plugin::performance::BudgetTracker,
) -> (InvokeOutcome, u128) {
    let started = Instant::now();

    // Snapshot the descriptor data we need before we mutate the
    // entry's runtime counters, so the borrow window stays narrow.
    let plugin_id = entry.descriptor.plugin_id.clone();
    let context = entry.descriptor.context.clone();
    let capabilities = entry.descriptor.capabilities.clone();
    let invocation_capabilities = entry.descriptor.plugin_invocation_capabilities.clone();
    let generation_id = entry.descriptor.generation_id;

    // Validate.
    let merged = match CommandRegistry::validate_args(&entry.descriptor, &args) {
        Ok(v) => v,
        Err(errors) => {
            let joined = errors.join("; ");
            diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(plugin_id.as_str()),
                    DiagnosticSeverity::Warning,
                    "command.invalid_args",
                    format!("command `{name}` rejected: {joined}"),
                )
                .with_source(PluginSourceSpan::ApiFunction {
                    name: format!("command:{name}"),
                })
                .with_context(serde_json::json!({
                    "name": name,
                    "errors": errors,
                    "plugin_id": plugin_id,
                })),
            );
            return (InvokeOutcome::InvalidArgs(errors), 0);
        }
    };

    // Capability check. Failing capabilities short-circuit before
    // the body runs and emit both a diagnostic and audit entry.
    for cap in &capabilities {
        if let Err(reason) = runner.check_capability(&plugin_id, cap) {
            let mut diag = PluginDiagnostic::new(
                PluginId::from(plugin_id.as_str()),
                DiagnosticSeverity::Error,
                "command.capability_denied",
                format!("command `{name}` denied: capability `{cap}` not granted ({reason})"),
            )
            .with_source(PluginSourceSpan::ApiFunction {
                name: format!("command:{name}"),
            })
            .with_context(serde_json::json!({
                "name": name,
                "capability": cap,
                "plugin_id": plugin_id,
                "context": context,
            }));
            if let Some(gid) = generation_id {
                diag = diag.with_generation(gid);
            }
            diagnostics.record(diag);
            return (InvokeOutcome::CapabilityDenied(cap.clone()), 0);
        }
    }

    if let Err(reason) = runner.check_invocation_context(name, &context) {
        diagnostics.record(
            PluginDiagnostic::new(
                PluginId::from(plugin_id.as_str()),
                DiagnosticSeverity::Error,
                "command.context_denied",
                format!("command `{name}` denied in context `{context}`: {reason}"),
            )
            .with_source(PluginSourceSpan::ApiFunction {
                name: format!("command:{name}"),
            })
            .with_context(serde_json::json!({
                "name": name,
                "plugin_id": plugin_id,
                "context": context,
                "target": name,
                "reason": reason,
            })),
        );
        return (InvokeOutcome::ContextDenied(context), 0);
    }

    for cap in &invocation_capabilities {
        if let Err(reason) = runner.check_invocation_capability(name, cap) {
            diagnostics.record(
                PluginDiagnostic::new(
                    PluginId::from(plugin_id.as_str()),
                    DiagnosticSeverity::Error,
                    "command.capability_denied",
                    format!("command `{name}` denied: caller lacks `{cap}` ({reason})"),
                )
                .with_source(PluginSourceSpan::ApiFunction {
                    name: format!("command:{name}"),
                })
                .with_context(serde_json::json!({
                    "name": name,
                    "capability": cap,
                    "plugin_id": plugin_id,
                    "context": context,
                    "target": name,
                })),
            );
            return (InvokeOutcome::CapabilityDenied(cap.clone()), 0);
        }
    }

    // Pre-dispatch info diagnostic.
    let pre_diag = PluginDiagnostic::new(
        PluginId::from(plugin_id.as_str()),
        DiagnosticSeverity::Info,
        "command.invoked",
        format!("command `{name}` invoked"),
    )
    .with_source(PluginSourceSpan::ApiFunction {
        name: format!("command:{name}"),
    })
    .with_context(serde_json::json!({
        "name": name,
        "plugin_id": plugin_id,
        "context": context,
        "args": merged.clone(),
    }));
    diagnostics.record(if let Some(gid) = generation_id {
        pre_diag.with_generation(gid)
    } else {
        pre_diag
    });

    // Run the body. Every body runs through the
    // budget tracker so its duration / breaker state is recorded
    // and a tripped breaker short-circuits before the runner is
    // invoked. The tracker emits its own diagnostic on skip;
    // we surface the skip as `Failed("circuit breaker tripped")`
    // so existing command-failure paths (event fan-out, audit)
    // still see a uniform shape.
    let pid_typed = crate::plugin::resources::PluginId::from(plugin_id.as_str());
    let cb_kind = crate::plugin::performance::CallbackKind::CommandCallback;
    let cb_id = format!("command:{name}");
    let cb_gen = generation_id.unwrap_or_else(|| crate::plugin::resources::GenerationId::new(0));
    let perf_outcome =
        budget_tracker.track_call::<(), String>(cb_kind, &pid_typed, cb_gen, &cb_id, || {
            runner.run(entry, &merged)
        });
    let result: Result<(), String> = match perf_outcome {
        crate::plugin::performance::Outcome::Ok(()) => Ok(()),
        crate::plugin::performance::Outcome::Err(e) => Err(e),
        crate::plugin::performance::Outcome::Skipped => Err("circuit breaker tripped".to_string()),
    };
    let duration_ms = started.elapsed().as_millis();

    // Bookkeeping + post-diagnostics.
    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let outcome = match &result {
        Ok(()) => {
            entry.runtime.fires += 1;
            entry.runtime.last_outcome = Some("ok".to_string());
            entry.runtime.last_invoked_at_unix_ms = now_unix_ms;
            entry.runtime.last_duration_ms = duration_ms;
            let post_diag = PluginDiagnostic::new(
                PluginId::from(plugin_id.as_str()),
                DiagnosticSeverity::Info,
                "command.completed",
                format!("command `{name}` completed"),
            )
            .with_source(PluginSourceSpan::ApiFunction {
                name: format!("command:{name}"),
            })
            .with_context(serde_json::json!({
                "name": name,
                "plugin_id": plugin_id,
                "context": context,
                "args": merged,
                "duration_ms": duration_ms,
            }));
            diagnostics.record(if let Some(gid) = generation_id {
                post_diag.with_generation(gid)
            } else {
                post_diag
            });
            InvokeOutcome::Ok
        }
        Err(cause) => {
            entry.runtime.fires += 1;
            entry.runtime.failures += 1;
            entry.runtime.last_outcome = Some(format!("failed: {cause}"));
            entry.runtime.last_invoked_at_unix_ms = now_unix_ms;
            entry.runtime.last_duration_ms = duration_ms;
            let fail_diag = PluginDiagnostic::new(
                PluginId::from(plugin_id.as_str()),
                DiagnosticSeverity::Error,
                "command.failed",
                format!("command `{name}` failed: {cause}"),
            )
            .with_source(PluginSourceSpan::ApiFunction {
                name: format!("command:{name}"),
            })
            .with_context(serde_json::json!({
                "name": name,
                "plugin_id": plugin_id,
                "context": context,
                "args": merged,
                "duration_ms": duration_ms,
                "cause": cause,
            }));
            diagnostics.record(if let Some(gid) = generation_id {
                fail_diag.with_generation(gid)
            } else {
                fail_diag
            });
            InvokeOutcome::Failed(cause.clone())
        }
    };

    runner.emit_command_executed(name, &plugin_id, outcome.is_ok(), duration_ms);
    (outcome, duration_ms)
}

/// Per-command projection used by the palette, `InspectorSnapshot`,
/// and the Lua `list()` return value.
#[derive(Debug, Clone)]
pub struct CommandSummary {
    pub name: String,
    pub title: String,
    pub description: String,
    pub plugin_id: String,
    pub generation_id: Option<u64>,
    pub context: String,
    pub destructive: bool,
    pub capabilities: Vec<String>,
    pub plugin_invocation_capabilities: Vec<String>,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub keymap_eligible: bool,
    pub palette_visible: bool,
    pub hooks: CommandHooks,
    pub args: Vec<CommandArgSummary>,
    pub fires: u64,
    pub failures: u64,
    pub last_outcome: Option<String>,
    pub last_duration_ms: u128,
}

#[derive(Debug, Clone)]
pub struct CommandArgSummary {
    pub name: String,
    pub ty: String,
    pub required: bool,
    pub default: Option<serde_json::Value>,
    pub doc: String,
}

fn validate_value(ty: &CommandArgType, value: &serde_json::Value) -> Result<(), String> {
    match ty {
        CommandArgType::String => {
            if value.is_string() {
                Ok(())
            } else {
                Err(format!("expected string, got {}", json_kind(value)))
            }
        }
        CommandArgType::Boolean => {
            if value.is_boolean() {
                Ok(())
            } else {
                Err(format!("expected boolean, got {}", json_kind(value)))
            }
        }
        CommandArgType::Integer => {
            if value.is_i64() || value.is_u64() {
                Ok(())
            } else {
                Err(format!("expected integer, got {}", json_kind(value)))
            }
        }
        CommandArgType::Number => {
            if value.is_number() {
                Ok(())
            } else {
                Err(format!("expected number, got {}", json_kind(value)))
            }
        }
        CommandArgType::Enum(allowed) => {
            let Some(s) = value.as_str() else {
                return Err(format!(
                    "expected one of [{}], got {}",
                    allowed.join(","),
                    json_kind(value)
                ));
            };
            if allowed.iter().any(|a| a == s) {
                Ok(())
            } else {
                Err(format!(
                    "expected one of [{}], got `{s}`",
                    allowed.join(",")
                ))
            }
        }
    }
}

fn json_kind(value: &serde_json::Value) -> &'static str {
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

/// Plugin-side capture of `leviathan.command.create` calls during init.
/// Drained by the host into permanent registry entries after init.lua
/// finishes — same shape as the autocmd staging path. Kept on
/// `BuildState` in [`crate::plugin::api`].
pub struct RawCommand {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub context: Option<String>,
    pub destructive: bool,
    pub capabilities: Vec<String>,
    pub args: Vec<RawCommandArg>,
    /// Lua function registry key on the calling plugin's Lua state.
    pub callback: RegistryKey,
    pub source_location: Option<String>,
}

/// One arg as captured from Lua. The host parses `ty` into
/// [`CommandArgType`] when the command is committed into the registry.
pub struct RawCommandArg {
    pub name: String,
    pub ty: String,
    pub required: bool,
    pub default: Option<serde_json::Value>,
    pub doc: Option<String>,
}

/// Build a [`CommandDescriptor`] from a `RawCommand` after init.lua.
/// Validates the schema (parses arg types, checks default sanity)
/// and returns either the descriptor or a list of error strings the
/// caller turns into `command.invalid_args` diagnostics at registration
/// time.
pub fn build_descriptor(
    raw: RawCommand,
    plugin_id: String,
    generation_id: Option<GenerationId>,
) -> Result<CommandDescriptor, Vec<String>> {
    let mut errors: Vec<String> = Vec::new();
    let mut args: Vec<CommandArg> = Vec::with_capacity(raw.args.len());
    for raw_arg in raw.args {
        match CommandArgType::parse(&raw_arg.ty) {
            Ok(ty) => {
                if let Some(default) = &raw_arg.default {
                    if let Err(e) = validate_value(&ty, default) {
                        errors.push(format!("arg `{}` default: {e}", raw_arg.name));
                        continue;
                    }
                }
                args.push(CommandArg {
                    name: raw_arg.name,
                    ty,
                    required: raw_arg.required,
                    default: raw_arg.default,
                    doc: raw_arg.doc.unwrap_or_default(),
                });
            }
            Err(e) => errors.push(format!("arg `{}` type: {e}", raw_arg.name)),
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(CommandDescriptor {
        title: raw.title.clone().unwrap_or_else(|| raw.name.clone()),
        description: raw.description.unwrap_or_default(),
        plugin_id,
        generation_id,
        context: raw
            .context
            .unwrap_or_else(|| CommandContext::GLOBAL.to_string()),
        args,
        destructive: raw.destructive,
        capabilities: raw.capabilities,
        plugin_invocation_capabilities: Vec::new(),
        availability: CommandAvailability::enabled(),
        keymap_eligible: true,
        palette_visible: true,
        hooks: CommandHooks::result_event_only(),
        run: CommandBody::Lua {
            callback: raw.callback,
        },
        name: raw.name,
    })
}

/// Cheap-clone shared handle to the live registry used by Lua
/// `invoke` / `list` shims. Held by both the host (which mutates
/// during dispatch) and every plugin's API installer.
pub type SharedCommandRegistry = Rc<RefCell<CommandRegistry>>;

/// Per-plugin runtime context the dispatcher needs to run a Lua-backed
/// command body and to gate capabilities at use time. The host owns
/// the master map; every API installer is handed a cheap-clone view.
#[derive(Default, Clone)]
pub struct CommandPluginRegistry {
    inner: Rc<RefCell<CommandPluginRegistryInner>>,
}

#[derive(Default)]
struct CommandPluginRegistryInner {
    plugins: HashMap<String, CommandPluginContext>,
}

/// Per-plugin handles needed by the dispatcher. `lua` is the owning
/// generation's `Lua` state used to call Lua-backed bodies;
/// `capability_guard` is used by the dispatcher's capability check
/// path so denials hit the audit log + diagnostic store same as
/// every other gated host call.
#[derive(Clone)]
pub struct CommandPluginContext {
    pub lua: Rc<Lua>,
    pub capability_guard: Rc<crate::plugin::capabilities::CapabilityGuard>,
    pub ui_context: crate::plugin::ui::context::UiContextStore,
    pub generation_id: GenerationId,
}

impl CommandPluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, plugin_id: impl Into<String>, ctx: CommandPluginContext) {
        self.inner
            .borrow_mut()
            .plugins
            .insert(plugin_id.into(), ctx);
    }

    pub fn remove(&self, plugin_id: &str) {
        self.inner.borrow_mut().plugins.remove(plugin_id);
    }

    pub fn get(&self, plugin_id: &str) -> Option<CommandPluginContext> {
        self.inner.borrow().plugins.get(plugin_id).cloned()
    }
}

/// Queued command-execution event. The Lua `invoke` shim and the
/// Rust `host.invoke_command` path both push one entry per dispatch
/// (success or failure) into this queue; the host drains the queue
/// on `tick` and fires the typed `CommandExecuted` autocmd event for
/// each. This indirection keeps the Lua shim from needing a `&mut
/// PluginHost` at call time.
#[derive(Default, Clone)]
pub struct PendingCommandEvents {
    inner: Rc<RefCell<Vec<PendingCommandEvent>>>,
}

#[derive(Debug, Clone)]
pub struct PendingCommandEvent {
    pub name: String,
    pub plugin_id: String,
    pub ok: bool,
    pub duration_ms: u128,
}

impl PendingCommandEvents {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, event: PendingCommandEvent) {
        self.inner.borrow_mut().push(event);
    }

    pub fn drain(&self) -> Vec<PendingCommandEvent> {
        std::mem::take(&mut *self.inner.borrow_mut())
    }

    pub fn is_empty(&self) -> bool {
        self.inner.borrow().is_empty()
    }
}

/// All shared state the dispatcher needs. Constructed once on the
/// host, cloned into every `leviathan.command.invoke` shim and into
/// the host's `invoke_command` Rust entry. Both paths route through
/// [`dispatch_command`].
#[derive(Clone)]
pub struct CommandDispatchEnv {
    pub commands: SharedCommandRegistry,
    pub plugin_registry: CommandPluginRegistry,
    pub diagnostics: DiagnosticStore,
    pub pending_events: PendingCommandEvents,
    pub active_context: Rc<RefCell<serde_json::Value>>,
    /// performance budget tracker. Threaded through so the dispatcher
    /// can wrap each command body in a `track_call` against the
    /// `CommandCallback` budget.
    pub budget_tracker: crate::plugin::performance::BudgetTracker,
}

#[derive(Clone)]
pub struct CommandInvocationCaller {
    pub plugin_id: String,
    pub capability_guard: Option<Rc<crate::plugin::capabilities::CapabilityGuard>>,
}

/// Synchronous dispatch entry point used by both the Lua shim and the
/// Rust-side host wrapper. Returns the structured outcome; the
/// `CommandExecuted` event for successful + failed runs is queued in
/// `env.pending_events` for the host to fire during the next tick.
///
/// Capability denials are still audited inside the
/// [`CommandRegistry::invoke`] flow via the supplied
/// [`DispatchEnvRunner`]; this wrapper only orchestrates the borrow
/// dance against the shared `Rc<RefCell<…>>`s.
pub fn dispatch_command(
    env: &CommandDispatchEnv,
    name: &str,
    args: serde_json::Value,
) -> InvokeOutcome {
    dispatch_command_as(env, None, name, args)
}

pub fn dispatch_command_as(
    env: &CommandDispatchEnv,
    caller: Option<CommandInvocationCaller>,
    name: &str,
    args: serde_json::Value,
) -> InvokeOutcome {
    let mut runner = DispatchEnvRunner {
        plugin_registry: env.plugin_registry.clone(),
        pending_events: env.pending_events.clone(),
        active_context: Rc::clone(&env.active_context),
        caller,
    };
    let Some((mut entry, index)) = detach_command_for_dispatch(&env.commands, name) else {
        record_command_not_found(&env.diagnostics, name);
        return InvokeOutcome::NotFound;
    };
    let (outcome, _duration) = invoke_entry(
        &mut entry,
        name,
        args,
        &mut runner,
        &env.diagnostics,
        &env.budget_tracker,
    );
    reattach_command_after_dispatch(&env.commands, entry, index);
    outcome
}

fn detach_command_for_dispatch(
    commands: &SharedCommandRegistry,
    name: &str,
) -> Option<(CommandEntry, usize)> {
    let mut registry = commands.borrow_mut();
    let index = registry
        .entries
        .iter()
        .position(|entry| entry.descriptor.name == name)?;
    let entry = registry.entries.remove(index);
    registry.detached = Some(DetachedDispatch {
        plugin_id: entry.descriptor.plugin_id.clone(),
        generation_id: entry.descriptor.generation_id,
        dropped: false,
    });
    Some((entry, index))
}

fn reattach_command_after_dispatch(
    commands: &SharedCommandRegistry,
    entry: CommandEntry,
    original_index: usize,
) {
    let mut registry = commands.borrow_mut();
    let dropped = registry.detached.take().is_some_and(|d| d.dropped);
    // A reload/unload during dispatch already removed this command's live
    // entry; reinserting the detached copy would resurrect a stale Lua key.
    if dropped {
        return;
    }
    let insert_index = original_index.min(registry.entries.len());
    registry.entries.insert(insert_index, entry);
}

/// `CommandRunner` impl over the shared dispatch env. Looks Lua up
/// out of [`CommandPluginRegistry`] for Lua bodies and routes
/// capability checks through the plugin's [`CapabilityGuard`].
pub struct DispatchEnvRunner {
    plugin_registry: CommandPluginRegistry,
    pending_events: PendingCommandEvents,
    active_context: Rc<RefCell<serde_json::Value>>,
    caller: Option<CommandInvocationCaller>,
}

impl CommandRunner for DispatchEnvRunner {
    fn run(&mut self, entry: &CommandEntry, args: &serde_json::Value) -> Result<(), String> {
        match &entry.descriptor.run {
            CommandBody::Host(f) => f(args),
            CommandBody::Lua { callback } => {
                let ctx = self
                    .plugin_registry
                    .get(&entry.descriptor.plugin_id)
                    .ok_or_else(|| {
                        format!(
                            "owning plugin `{}` is not loaded",
                            entry.descriptor.plugin_id
                        )
                    })?;
                ctx.ui_context.set(self.active_context.borrow().clone());
                run_lua_body(&ctx.lua, callback, args)
            }
        }
    }

    fn check_capability(&mut self, plugin_id: &str, capability: &str) -> Result<(), String> {
        // Host commands never gate behind plugin capability grants —
        // their runner registers them with no `capabilities`. So a
        // capability check arriving for the host plugin id with a
        // non-empty list would be a misconfiguration. Treat as denied
        // so the diagnostic surfaces the bad descriptor.
        if plugin_id == HOST_COMMAND_PLUGIN_ID {
            return Err(format!(
                "host command should not declare capability `{capability}`"
            ));
        }
        let ctx = self
            .plugin_registry
            .get(plugin_id)
            .ok_or_else(|| format!("plugin `{plugin_id}` is not loaded"))?;
        ctx.capability_guard.check_named(capability)
    }

    fn check_invocation_capability(
        &mut self,
        command_name: &str,
        capability: &str,
    ) -> Result<(), String> {
        let Some(caller) = self.caller.as_ref() else {
            return Ok(());
        };
        if caller.plugin_id == HOST_COMMAND_PLUGIN_ID
            || caller.plugin_id == crate::plugin::keymap::USER_KEYMAP_PLUGIN_ID
        {
            return Ok(());
        }
        if let Some(guard) = caller.capability_guard.as_ref() {
            return guard.check_named_for_target(
                capability,
                command_name,
                "leviathan.command.invoke",
                None,
                "add the capability to plugin.toml",
            );
        }
        let ctx = self
            .plugin_registry
            .get(&caller.plugin_id)
            .ok_or_else(|| format!("plugin `{}` is not loaded", caller.plugin_id))?;
        ctx.capability_guard.check_named_for_target(
            capability,
            command_name,
            "leviathan.command.invoke",
            None,
            "add the capability to plugin.toml",
        )
    }

    fn check_invocation_context(
        &mut self,
        _command_name: &str,
        command_context: &str,
    ) -> Result<(), String> {
        let Some(caller) = self.caller.as_ref() else {
            return Ok(());
        };
        if caller.plugin_id == HOST_COMMAND_PLUGIN_ID
            || caller.plugin_id == crate::plugin::keymap::USER_KEYMAP_PLUGIN_ID
            || caller.capability_guard.is_none()
        {
            return Ok(());
        }
        let active = self.active_context.borrow();
        if command_context_allowed(command_context, &active) {
            Ok(())
        } else {
            Err(format!("active surface is `{}`", context_surface(&active)))
        }
    }

    fn emit_command_executed(&mut self, name: &str, plugin_id: &str, ok: bool, duration_ms: u128) {
        self.pending_events.push(PendingCommandEvent {
            name: name.to_string(),
            plugin_id: plugin_id.to_string(),
            ok,
            duration_ms,
        });
    }
}

fn command_context_allowed(command_context: &str, active: &serde_json::Value) -> bool {
    if command_context == CommandContext::GLOBAL {
        return true;
    }
    if command_context == "repository" || command_context.starts_with("repository.") {
        return active
            .pointer("/repository/is_open")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    }
    if command_context == "tab_bar" {
        return active
            .pointer("/tab/count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            > 0;
    }
    context_surface(active) == command_context
}

fn context_surface(active: &serde_json::Value) -> &str {
    active.get("surface").and_then(|v| v.as_str()).unwrap_or("")
}

impl CommandEntry {
    pub fn run(&self) -> &CommandBody {
        &self.descriptor.run
    }
}

/// Helper that runs a Lua command body on the supplied `Lua` state.
/// Used by the host runner to translate the JSON args table into a
/// Lua table. Errors flow back as `Err(String)` so the registry's
/// `command.failed` diagnostic carries a stable cause.
pub fn run_lua_body(
    lua: &Lua,
    callback: &RegistryKey,
    args: &serde_json::Value,
) -> Result<(), String> {
    use mlua::LuaSerdeExt;
    let func: Function = lua
        .registry_value(callback)
        .map_err(|e| format!("callback lookup: {e}"))?;
    let lua_args = lua
        .to_value(args)
        .map_err(|e| format!("args to_value: {e}"))?;
    func.call::<()>(lua_args).map_err(|e| e.to_string())?;
    Ok(())
}

/// Discard every `RegistryKey` carried by `removed` against `lua`.
/// Called after `drop_for_plugin` / `drop_for_generation` so reaped
/// commands release their Lua references promptly. `lua` may be the
/// owning plugin's state; if it doesn't recognise the key the call
/// is a silent no-op (matches mlua's drop semantics).
pub fn release_lua_keys(removed: Vec<CommandEntry>, lua: &Lua) {
    for entry in removed {
        if let CommandBody::Lua { callback } = entry.descriptor.run {
            let _ = lua.remove_registry_value(callback);
        }
    }
}

/// Headless palette state. Holds a query string and a destructive-show
/// toggle; produces filtered, ranked summaries via [`Self::filter`].
/// The visible overlay (keymaps / later) sits on top of this.
#[derive(Debug, Clone, Default)]
pub struct PaletteState {
    pub query: String,
    pub show_destructive: bool,
    /// Currently-active context name. Commands declared in `global`
    /// always show; commands declared in any other context show only
    /// when that context matches. Empty == "show every context".
    pub context: String,
}

impl PaletteState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
    }

    pub fn set_context(&mut self, context: impl Into<String>) {
        self.context = context.into();
    }

    pub fn set_show_destructive(&mut self, show: bool) {
        self.show_destructive = show;
    }

    /// Filter and rank the supplied summaries against the current
    /// palette state. Ranking is deterministic:
    ///
    /// 1. exact name match;
    /// 2. case-insensitive prefix on title or name;
    /// 3. case-insensitive substring on title, name, description, or
    ///    plugin_id;
    /// 4. by `(plugin_id, name)` for ties.
    pub fn filter(&self, summaries: &[CommandSummary]) -> Vec<CommandSummary> {
        let needle = self.query.to_ascii_lowercase();
        let mut scored: Vec<(u32, &CommandSummary)> = Vec::new();
        for summary in summaries {
            if !summary.palette_visible {
                continue;
            }
            if !summary.enabled {
                continue;
            }
            if summary.destructive && !self.show_destructive {
                continue;
            }
            if !self.context.is_empty()
                && summary.context != CommandContext::GLOBAL
                && summary.context != self.context
            {
                continue;
            }
            if needle.is_empty() {
                scored.push((1000, summary));
                continue;
            }
            let lower_name = summary.name.to_ascii_lowercase();
            let lower_title = summary.title.to_ascii_lowercase();
            let lower_desc = summary.description.to_ascii_lowercase();
            let lower_plugin = summary.plugin_id.to_ascii_lowercase();
            let mut best: Option<u32> = None;
            if lower_name == needle {
                best = Some(0);
            } else if lower_name.starts_with(&needle) || lower_title.starts_with(&needle) {
                best = Some(10);
            } else if lower_name.contains(&needle)
                || lower_title.contains(&needle)
                || lower_desc.contains(&needle)
                || lower_plugin.contains(&needle)
            {
                best = Some(20);
            }
            if let Some(score) = best {
                scored.push((score, summary));
            }
        }
        scored.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.plugin_id.cmp(&b.1.plugin_id))
                .then(a.1.name.cmp(&b.1.name))
        });
        scored.into_iter().map(|(_, s)| s.clone()).collect()
    }

    /// Convenience: filter and return the first match. Used by tests
    /// and by the keyboard activation path that should run "the most
    /// likely" command for the current query.
    pub fn first_match(&self, summaries: &[CommandSummary]) -> Option<CommandSummary> {
        self.filter(summaries).into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag_store() -> DiagnosticStore {
        DiagnosticStore::with_sink(std::sync::Arc::new(crate::plugin::diagnostic::NullSink))
    }

    fn budget_tracker() -> crate::plugin::performance::BudgetTracker {
        crate::plugin::performance::BudgetTracker::new(diag_store())
    }

    fn host_descriptor(name: &str) -> CommandDescriptor {
        CommandDescriptor {
            name: name.into(),
            title: name.into(),
            description: String::new(),
            plugin_id: HOST_COMMAND_PLUGIN_ID.into(),
            generation_id: None,
            context: CommandContext::GLOBAL.into(),
            args: Vec::new(),
            destructive: false,
            capabilities: Vec::new(),
            plugin_invocation_capabilities: Vec::new(),
            availability: CommandAvailability::enabled(),
            keymap_eligible: true,
            palette_visible: true,
            hooks: CommandHooks::result_event_only(),
            run: CommandBody::Host(Box::new(|_| Ok(()))),
        }
    }

    struct StubRunner {
        ran: Rc<RefCell<Vec<String>>>,
        denied: Vec<String>,
        events: Rc<RefCell<Vec<(String, bool)>>>,
    }

    type StubRunnerParts = (
        StubRunner,
        Rc<RefCell<Vec<String>>>,
        Rc<RefCell<Vec<(String, bool)>>>,
    );

    impl CommandRunner for StubRunner {
        fn run(&mut self, entry: &CommandEntry, _args: &serde_json::Value) -> Result<(), String> {
            self.ran.borrow_mut().push(entry.descriptor.name.clone());
            match &entry.descriptor.run {
                CommandBody::Host(f) => f(&serde_json::Value::Null),
                CommandBody::Lua { .. } => Ok(()),
            }
        }
        fn check_capability(&mut self, _plugin_id: &str, capability: &str) -> Result<(), String> {
            if self.denied.iter().any(|c| c == capability) {
                Err("denied".into())
            } else {
                Ok(())
            }
        }
        fn check_invocation_capability(
            &mut self,
            _command_name: &str,
            capability: &str,
        ) -> Result<(), String> {
            self.check_capability("caller", capability)
        }
        fn check_invocation_context(
            &mut self,
            _command_name: &str,
            _command_context: &str,
        ) -> Result<(), String> {
            Ok(())
        }
        fn emit_command_executed(
            &mut self,
            name: &str,
            _plugin_id: &str,
            ok: bool,
            _duration_ms: u128,
        ) {
            self.events.borrow_mut().push((name.to_string(), ok));
        }
    }

    fn stub_runner() -> StubRunnerParts {
        let ran: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let events: Rc<RefCell<Vec<(String, bool)>>> = Rc::new(RefCell::new(Vec::new()));
        (
            StubRunner {
                ran: Rc::clone(&ran),
                denied: Vec::new(),
                events: Rc::clone(&events),
            },
            ran,
            events,
        )
    }

    #[test]
    fn dispatch_command_releases_registry_borrow_while_body_runs() {
        let diagnostics = diag_store();
        let commands: SharedCommandRegistry = Rc::new(RefCell::new(CommandRegistry::new()));
        let saw_list = Rc::new(RefCell::new(false));

        commands
            .borrow_mut()
            .register(host_descriptor("other"), &diagnostics);

        let commands_for_body = Rc::clone(&commands);
        let saw_list_for_body = Rc::clone(&saw_list);
        commands.borrow_mut().register(
            CommandDescriptor {
                name: "open".into(),
                title: "open".into(),
                description: String::new(),
                plugin_id: HOST_COMMAND_PLUGIN_ID.into(),
                generation_id: None,
                context: CommandContext::GLOBAL.into(),
                args: Vec::new(),
                destructive: false,
                capabilities: Vec::new(),
                plugin_invocation_capabilities: Vec::new(),
                availability: CommandAvailability::enabled(),
                keymap_eligible: true,
                palette_visible: true,
                hooks: CommandHooks::result_event_only(),
                run: CommandBody::Host(Box::new(move |_| {
                    let names: Vec<String> = commands_for_body
                        .borrow()
                        .summaries()
                        .into_iter()
                        .map(|summary| summary.name)
                        .collect();
                    assert!(names.iter().any(|name| name == "other"));
                    *saw_list_for_body.borrow_mut() = true;
                    Ok(())
                })),
            },
            &diagnostics,
        );

        let env = CommandDispatchEnv {
            commands: Rc::clone(&commands),
            plugin_registry: CommandPluginRegistry::new(),
            diagnostics,
            pending_events: PendingCommandEvents::new(),
            active_context: Rc::new(RefCell::new(serde_json::json!({
                "surface": "screen",
                "repository": { "is_open": false },
                "tab": { "count": 0 },
            }))),
            budget_tracker: budget_tracker(),
        };

        assert_eq!(
            dispatch_command(&env, "open", serde_json::Value::Null),
            InvokeOutcome::Ok
        );
        assert!(*saw_list.borrow());
        assert!(commands
            .borrow()
            .entries()
            .iter()
            .any(|entry| entry.descriptor.name == "open"));
    }

    #[test]
    fn arg_type_parser_accepts_expected_forms() {
        assert!(matches!(
            CommandArgType::parse("string").unwrap(),
            CommandArgType::String
        ));
        assert!(matches!(
            CommandArgType::parse("boolean").unwrap(),
            CommandArgType::Boolean
        ));
        assert!(matches!(
            CommandArgType::parse("integer").unwrap(),
            CommandArgType::Integer
        ));
        assert!(matches!(
            CommandArgType::parse("number").unwrap(),
            CommandArgType::Number
        ));
        let e = CommandArgType::parse("enum:a,b,c").unwrap();
        match e {
            CommandArgType::Enum(values) => assert_eq!(values, vec!["a", "b", "c"]),
            _ => panic!("expected enum"),
        }
        assert!(CommandArgType::parse("garbage").is_err());
        assert!(CommandArgType::parse("enum:").is_err());
    }

    #[test]
    fn validate_args_strict_about_extra_and_missing() {
        let mut desc = host_descriptor("Test");
        desc.args.push(CommandArg {
            name: "force".into(),
            ty: CommandArgType::Boolean,
            required: false,
            default: Some(serde_json::json!(false)),
            doc: String::new(),
        });
        // Extra key.
        let res = CommandRegistry::validate_args(
            &desc,
            &serde_json::json!({ "force": true, "extra": 1 }),
        );
        assert!(res.is_err());
        // Wrong type.
        let res = CommandRegistry::validate_args(&desc, &serde_json::json!({ "force": "yes" }));
        assert!(res.is_err());
        // Default fills in.
        let merged = CommandRegistry::validate_args(&desc, &serde_json::Value::Null).unwrap();
        assert_eq!(merged["force"], serde_json::json!(false));
    }

    #[test]
    fn invoke_records_command_executed_on_success() {
        let mut reg = CommandRegistry::new();
        let store = diag_store();
        reg.register(host_descriptor("Hello"), &store);
        let (mut runner, ran, events) = stub_runner();
        let (out, _) = reg.invoke(
            "Hello",
            serde_json::Value::Null,
            &mut runner,
            &store,
            &budget_tracker(),
        );
        assert!(out.is_ok());
        assert_eq!(ran.borrow().as_slice(), ["Hello"]);
        assert_eq!(events.borrow().as_slice(), [("Hello".into(), true)]);
        assert_eq!(reg.find("Hello").unwrap().runtime.fires, 1);
    }

    #[test]
    fn invoke_records_command_executed_on_failure() {
        let mut reg = CommandRegistry::new();
        let store = diag_store();
        let mut desc = host_descriptor("Boom");
        desc.run = CommandBody::Host(Box::new(|_| Err("kaboom".into())));
        reg.register(desc, &store);
        let (mut runner, _, events) = stub_runner();
        let (out, _) = reg.invoke(
            "Boom",
            serde_json::Value::Null,
            &mut runner,
            &store,
            &budget_tracker(),
        );
        assert!(matches!(out, InvokeOutcome::Failed(ref c) if c == "kaboom"));
        assert_eq!(events.borrow().as_slice(), [("Boom".into(), false)]);
        let entry = reg.find("Boom").unwrap();
        assert_eq!(entry.runtime.failures, 1);
    }

    #[test]
    fn capability_denied_short_circuits_before_body() {
        let mut reg = CommandRegistry::new();
        let store = diag_store();
        let mut desc = host_descriptor("DangerZone");
        desc.capabilities.push("git:write".into());
        reg.register(desc, &store);
        let (mut runner, ran, events) = stub_runner();
        runner.denied.push("git:write".into());
        let (out, _) = reg.invoke(
            "DangerZone",
            serde_json::Value::Null,
            &mut runner,
            &store,
            &budget_tracker(),
        );
        assert!(matches!(out, InvokeOutcome::CapabilityDenied(ref c) if c == "git:write"));
        assert!(ran.borrow().is_empty(), "body must not run");
        assert!(events.borrow().is_empty(), "no CommandExecuted on denial");
    }

    #[test]
    fn drop_for_plugin_returns_removed_entries() {
        let mut reg = CommandRegistry::new();
        let store = diag_store();
        let mut a = host_descriptor("A");
        a.plugin_id = "p1".into();
        let mut b = host_descriptor("B");
        b.plugin_id = "p2".into();
        reg.register(a, &store);
        reg.register(b, &store);
        let removed = reg.drop_for_plugin("p1");
        assert_eq!(removed.len(), 1);
        assert!(reg.find("A").is_none());
        assert!(reg.find("B").is_some());
    }

    #[test]
    fn palette_destructive_filter() {
        let mut reg = CommandRegistry::new();
        let store = diag_store();
        let mut safe = host_descriptor("Safe");
        let mut danger = host_descriptor("Danger");
        danger.destructive = true;
        safe.title = "Safe Refresh".into();
        danger.title = "Reset Hard".into();
        reg.register(safe, &store);
        reg.register(danger, &store);

        let summaries = reg.summaries();
        let mut palette = PaletteState::new();
        palette.set_query("");
        let visible = palette.filter(&summaries);
        let names: Vec<&str> = visible.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Safe"]);

        palette.set_show_destructive(true);
        let visible = palette.filter(&summaries);
        let names: Vec<&str> = visible.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Danger"));
        assert!(names.contains(&"Safe"));
    }

    #[test]
    fn palette_filter_skips_disabled_commands() {
        let mut reg = CommandRegistry::new();
        let store = diag_store();
        let mut disabled = host_descriptor("Amend");
        disabled.availability = CommandAvailability::disabled("nothing to amend");
        reg.register(host_descriptor("Refresh"), &store);
        reg.register(disabled, &store);
        let summaries = reg.summaries();
        let mut palette = PaletteState::new();
        palette.set_query("");
        let visible = palette.filter(&summaries);
        let names: Vec<&str> = visible.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Refresh"]);
    }

    #[test]
    fn palette_filter_ranks_exact_match_first() {
        let mut reg = CommandRegistry::new();
        let store = diag_store();
        reg.register(host_descriptor("Refresh"), &store);
        reg.register(host_descriptor("RefreshAll"), &store);
        let summaries = reg.summaries();
        let mut palette = PaletteState::new();
        palette.set_query("refresh");
        let visible = palette.filter(&summaries);
        assert_eq!(visible.first().unwrap().name, "Refresh");
    }

    #[test]
    fn palette_context_filter_global_always_shows() {
        let mut reg = CommandRegistry::new();
        let store = diag_store();
        let global = host_descriptor("Global");
        let mut repo = host_descriptor("InRepo");
        repo.context = "repository".into();
        reg.register(global, &store);
        reg.register(repo, &store);
        let summaries = reg.summaries();
        let mut palette = PaletteState::new();
        palette.set_context("global");
        let visible = palette.filter(&summaries);
        let names: Vec<&str> = visible.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Global"]);
        palette.set_context("repository");
        let visible = palette.filter(&summaries);
        let names: Vec<&str> = visible.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Global"));
        assert!(names.contains(&"InRepo"));
    }

    #[test]
    fn invalid_args_emits_diagnostic_and_skips_body() {
        let mut reg = CommandRegistry::new();
        let store = diag_store();
        let mut desc = host_descriptor("StrictArgs");
        desc.args.push(CommandArg {
            name: "n".into(),
            ty: CommandArgType::Integer,
            required: true,
            default: None,
            doc: String::new(),
        });
        reg.register(desc, &store);
        let (mut runner, ran, events) = stub_runner();
        let (out, _) = reg.invoke(
            "StrictArgs",
            serde_json::Value::Null,
            &mut runner,
            &store,
            &budget_tracker(),
        );
        assert!(matches!(out, InvokeOutcome::InvalidArgs(_)));
        assert!(ran.borrow().is_empty(), "body must not run on invalid args");
        assert!(
            events.borrow().is_empty(),
            "no CommandExecuted on invalid args"
        );
        let codes: Vec<String> = store.tail(20).into_iter().map(|d| d.code).collect();
        assert!(codes.iter().any(|c| c == "command.invalid_args"));
    }

    #[test]
    fn unknown_command_emits_not_found() {
        let mut reg = CommandRegistry::new();
        let store = diag_store();
        let (mut runner, _, _) = stub_runner();
        let (out, _) = reg.invoke(
            "Nope",
            serde_json::Value::Null,
            &mut runner,
            &store,
            &budget_tracker(),
        );
        assert!(matches!(out, InvokeOutcome::NotFound));
        let codes: Vec<String> = store.tail(20).into_iter().map(|d| d.code).collect();
        assert!(codes.iter().any(|c| c == "command.not_found"));
    }
}

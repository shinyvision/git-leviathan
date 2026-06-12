//! host-provided devtools commands.
//!
//! Registers ten plugin-management commands under the host plugin id
//! (`<host>`). Every command goes through the same command registry dispatcher
//! every other host or plugin command goes through, so palette
//! listing, schema validation, capability gating, audit + diagnostic
//! emission, and the `CommandExecuted` event all behave identically.
//!
//! Each command's body captures a [`DevtoolsActionQueue`] handle and
//! pushes a [`DevtoolsAction`] describing what the host should do.
//! [`crate::plugin::host::PluginHost::invoke_command`] drains the
//! queue after dispatch returns and applies each action against
//! `&mut self`, producing a structured JSON result. Read-only
//! commands (e.g. `Plugin: Show Capability Audit`) flow through the
//! same path — the host action handler computes their result from
//! registries it owns — so every devtools command shares one entry
//! point.
//!
//! Diagnostic codes emitted from this module:
//!
//! - `devtools.command.bad_args` — args were schema-valid but
//!   semantically wrong (e.g. `surfaces` contained an unknown name).
//!
//! Other diagnostic codes (`devtools.command.unknown_plugin`,
//! `devtools.command.unknown`) are emitted by the host action handler
//! when it processes the queued action.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{json, Value};

use crate::plugin::commands::{
    CommandArg, CommandArgType, CommandAvailability, CommandBody, CommandContext,
    CommandDescriptor, CommandHooks, CommandRegistry, HOST_COMMAND_PLUGIN_ID,
};
use crate::plugin::diagnostic::DiagnosticStore;

/// Cheap-clone shared queue of devtools actions waiting for the host
/// to apply. The host owns the master `Rc`; every registered command
/// body captures a clone. The host drains it inside
/// `invoke_command` after dispatch returns.
pub type DevtoolsActionQueue = Rc<RefCell<Vec<DevtoolsAction>>>;

/// One queued action a command body asked the host to run
/// after dispatch returns. Carries the command name and its merged
/// (defaults-applied) args so the host applies the action with the
/// same view the body saw. The host action handler matches on
/// `name` and projects fields out of `args` directly.
#[derive(Debug, Clone)]
pub struct DevtoolsAction {
    pub name: String,
    pub args: Value,
}

impl DevtoolsAction {
    pub fn new(name: impl Into<String>, args: Value) -> Self {
        Self {
            name: name.into(),
            args,
        }
    }

    /// Convenience: read a string arg by key.
    pub fn arg_str<'a>(&'a self, key: &str) -> Option<&'a str> {
        self.args.get(key).and_then(|v| v.as_str())
    }
}

struct DevtoolsCommandSpec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    destructive: bool,
    args: Vec<CommandArg>,
}

fn plugin_id_arg() -> CommandArg {
    CommandArg {
        name: "plugin_id".into(),
        ty: CommandArgType::String,
        required: true,
        default: None,
        doc: "Plugin id to operate on.".into(),
    }
}

fn contribution_arg(name: &'static str, doc: &'static str) -> CommandArg {
    CommandArg {
        name: name.into(),
        ty: CommandArgType::String,
        required: true,
        default: None,
        doc: doc.into(),
    }
}

fn optional_priority_arg() -> CommandArg {
    CommandArg {
        name: "priority".into(),
        ty: CommandArgType::Integer,
        required: false,
        default: None,
        doc: "Optional replacement priority for reorderable slot contributions.".into(),
    }
}

fn optional_hidden_arg() -> CommandArg {
    CommandArg {
        name: "hidden".into(),
        ty: CommandArgType::Boolean,
        required: false,
        default: None,
        doc: "When present, set visibility instead of toggling it.".into(),
    }
}

fn contribution_args(include_priority: bool) -> Vec<CommandArg> {
    let mut args = vec![
        contribution_arg(
            "plugin_id",
            "Contribution owner plugin id; use `builtin` for built-ins.",
        ),
        contribution_arg("region", "Region id such as `main_bar`."),
        contribution_arg(
            "container",
            "Container key such as `left` or `sidebar.top`.",
        ),
        contribution_arg("id", "Contribution id."),
    ];
    if include_priority {
        args.push(optional_hidden_arg());
        args.push(optional_priority_arg());
    }
    args
}

fn optional_plugin_id_arg() -> CommandArg {
    CommandArg {
        name: "plugin_id".into(),
        ty: CommandArgType::String,
        required: false,
        default: None,
        doc: "Plugin id to filter on; omit for host-wide.".into(),
    }
}

fn limit_arg() -> CommandArg {
    CommandArg {
        name: "limit".into(),
        ty: CommandArgType::Integer,
        required: false,
        default: Some(json!(50)),
        doc: "Max rows to return.".into(),
    }
}

fn devtools_specs() -> Vec<DevtoolsCommandSpec> {
    vec![
        DevtoolsCommandSpec {
            name: "plugin.reload",
            title: "Plugin: Reload",
            description: "Stage a fresh generation of a plugin and atomically swap it.",
            destructive: false,
            args: vec![plugin_id_arg()],
        },
        DevtoolsCommandSpec {
            name: "plugin.disable",
            title: "Plugin: Disable",
            description: "Mark a plugin disabled so subsequent loads skip it.",
            destructive: true,
            args: vec![plugin_id_arg()],
        },
        DevtoolsCommandSpec {
            name: "plugin.enable",
            title: "Plugin: Enable",
            description: "Remove the disabled flag and reload the plugin.",
            destructive: false,
            args: vec![plugin_id_arg()],
        },
        DevtoolsCommandSpec {
            name: "plugin.open_log",
            title: "Plugin: Open Log",
            description: "Return the plugin's diagnostic-log path.",
            destructive: false,
            args: vec![plugin_id_arg()],
        },
        DevtoolsCommandSpec {
            name: "plugin.inspect_ui_tree",
            title: "Plugin: Inspect UI Tree",
            description: "Return a structured projection of the plugin's widget AST inventory.",
            destructive: false,
            args: vec![plugin_id_arg()],
        },
        DevtoolsCommandSpec {
            name: "plugin.inspect_ui_context",
            title: "Plugin: Inspect UI Context",
            description: "Return the current typed UI context snapshot.",
            destructive: false,
            args: vec![],
        },
        DevtoolsCommandSpec {
            name: "plugin.inspect_dock_layout",
            title: "Plugin: Inspect Dock Layout",
            description: "Return dock panel registrations and persisted layout state.",
            destructive: false,
            args: vec![],
        },
        DevtoolsCommandSpec {
            name: "plugin.run_health_check",
            title: "Plugin: Run Health Check",
            description: "Invoke the plugin's registered health checks and return the result.",
            destructive: false,
            args: vec![plugin_id_arg()],
        },
        DevtoolsCommandSpec {
            name: "plugin.clear_state",
            title: "Plugin: Clear State",
            description:
                "Wipe selected storage surfaces. Defaults to state + cache; never touches \
                          secrets unless explicitly listed.",
            destructive: true,
            args: vec![
                plugin_id_arg(),
                CommandArg {
                    name: "surfaces".into(),
                    ty: CommandArgType::String,
                    required: false,
                    default: Some(json!("state,cache")),
                    doc: "Comma-separated surface list (state|cache|config|repo|settings|secrets)."
                        .into(),
                },
            ],
        },
        DevtoolsCommandSpec {
            name: "plugin.export_diagnostic_bundle",
            title: "Plugin: Export Diagnostic Bundle",
            description: "Export a structured bundle for offline debugging. \
                          Excludes secret values even when include_secrets = true.",
            destructive: false,
            args: vec![
                optional_plugin_id_arg(),
                CommandArg {
                    name: "include_state".into(),
                    ty: CommandArgType::Boolean,
                    required: false,
                    default: Some(json!(false)),
                    doc: "Include plugin state JSON (never secrets).".into(),
                },
                CommandArg {
                    name: "include_secrets".into(),
                    ty: CommandArgType::Boolean,
                    required: false,
                    default: Some(json!(false)),
                    doc: "Include secret metadata (key names only; never values).".into(),
                },
            ],
        },
        DevtoolsCommandSpec {
            name: "plugin.show_capability_audit",
            title: "Plugin: Show Capability Audit",
            description: "Return recent capability audit entries, optionally filtered by plugin.",
            destructive: false,
            args: vec![optional_plugin_id_arg(), limit_arg()],
        },
        DevtoolsCommandSpec {
            name: "plugin.show_runtime_path",
            title: "Plugin: Show Runtime Path",
            description: "Return the resolved runtime-path entries for a plugin.",
            destructive: false,
            args: vec![plugin_id_arg()],
        },
        DevtoolsCommandSpec {
            name: "plugin_ui.toggle_contribution",
            title: "Plugin UI: Toggle Contribution",
            description: "Hide or show one built-in or plugin UI contribution.",
            destructive: false,
            args: contribution_args(true),
        },
        DevtoolsCommandSpec {
            name: "plugin_ui.reset_layout",
            title: "Plugin UI: Reset Layout",
            description: "Clear plugin UI contribution overrides and reset host layout state.",
            destructive: true,
            args: vec![],
        },
        DevtoolsCommandSpec {
            name: "plugin_ui.inspect_contribution",
            title: "Plugin UI: Inspect Contribution",
            description: "Return effective override state for one UI contribution.",
            destructive: false,
            args: contribution_args(false),
        },
    ]
}

/// Stable list of every devtools command name, in the same order as
/// [`devtools_specs`]. Kept as a `const` so the dispatch guard can
/// check membership without rebuilding the full spec vector.
const DEVTOOLS_COMMAND_NAMES: &[&str] = &[
    "plugin.reload",
    "plugin.disable",
    "plugin.enable",
    "plugin.open_log",
    "plugin.inspect_ui_tree",
    "plugin.inspect_ui_context",
    "plugin.inspect_dock_layout",
    "plugin.run_health_check",
    "plugin.clear_state",
    "plugin.export_diagnostic_bundle",
    "plugin.show_capability_audit",
    "plugin.show_runtime_path",
    "plugin_ui.toggle_contribution",
    "plugin_ui.reset_layout",
    "plugin_ui.inspect_contribution",
];

/// Stable list of every devtools command name. Used by tests to
/// confirm every command is reachable through the dispatcher and by
/// the host to recognise devtools dispatches inside `invoke_command`.
pub fn devtools_command_names() -> Vec<&'static str> {
    DEVTOOLS_COMMAND_NAMES.to_vec()
}

/// Register every devtools command into `registry`. Bodies capture
/// `actions` so they can queue an action for the host to apply with
/// `&mut PluginHost` after dispatch returns.
pub fn register(
    registry: &mut CommandRegistry,
    actions: DevtoolsActionQueue,
    diagnostics: &DiagnosticStore,
) {
    for spec in devtools_specs() {
        let name = spec.name.to_string();
        let actions_for_body = Rc::clone(&actions);
        let body: CommandBody = CommandBody::Host(Box::new(move |args: &Value| {
            run_command(name.as_str(), args, &actions_for_body)
        }));
        let descriptor = CommandDescriptor {
            name: spec.name.into(),
            title: spec.title.into(),
            description: spec.description.into(),
            plugin_id: HOST_COMMAND_PLUGIN_ID.into(),
            generation_id: None,
            context: CommandContext::GLOBAL.into(),
            args: spec.args,
            destructive: spec.destructive,
            capabilities: Vec::new(),
            plugin_invocation_capabilities: Vec::new(),
            availability: CommandAvailability::enabled(),
            keymap_eligible: true,
            palette_visible: true,
            hooks: CommandHooks::result_event_only(),
            run: body,
        };
        registry.register(descriptor, diagnostics);
    }
}

/// Body shared across every devtools command. Validates body-side
/// semantic constraints (the only one today: `plugin.clear_state`'s
/// `surfaces` allow-list) and queues a [`DevtoolsAction`] for the
/// host to apply with `&mut PluginHost`. Returns `Err` only on
/// semantic argument errors — missing plugin ids surface as
/// `devtools.command.unknown_plugin` diagnostics from the host
/// action handler, since we don't have `&PluginHost` here.
fn run_command(name: &str, args: &Value, actions: &DevtoolsActionQueue) -> Result<(), String> {
    if !DEVTOOLS_COMMAND_NAMES.contains(&name) {
        return Err(format!("unknown devtools command `{name}`"));
    }
    let action = DevtoolsAction::new(name, args.clone());
    if action.name == "plugin.clear_state" {
        let surfaces_raw = action.arg_str("surfaces").unwrap_or("state,cache");
        for s in surfaces_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if !KNOWN_SURFACES.contains(&s) {
                return Err(format!("unknown storage surface `{s}`"));
            }
        }
    }
    actions.borrow_mut().push(action);
    Ok(())
}

/// Surfaces accepted by `plugin.clear_state`. Mirrors the storage
/// surface enum's external string form. `secrets` is intentionally
/// included so a caller who *explicitly* asks for it can wipe them;
/// the default surface list never includes `secrets`.
const KNOWN_SURFACES: &[&str] = &["state", "cache", "config", "repo", "settings", "secrets"];

#[cfg(test)]
mod tests {
    use super::*;

    fn new_queue() -> DevtoolsActionQueue {
        Rc::new(RefCell::new(Vec::new()))
    }

    #[test]
    fn devtools_command_names_match_specs() {
        let names = devtools_command_names();
        assert_eq!(names.len(), 15);
        // Stable order so palette tests can rely on it.
        let expected = [
            "plugin.reload",
            "plugin.disable",
            "plugin.enable",
            "plugin.open_log",
            "plugin.inspect_ui_tree",
            "plugin.inspect_ui_context",
            "plugin.inspect_dock_layout",
            "plugin.run_health_check",
            "plugin.clear_state",
            "plugin.export_diagnostic_bundle",
            "plugin.show_capability_audit",
            "plugin.show_runtime_path",
            "plugin_ui.toggle_contribution",
            "plugin_ui.reset_layout",
            "plugin_ui.inspect_contribution",
        ];
        assert_eq!(names, expected);
        // The const list must stay in lockstep with the specs.
        let spec_names: Vec<&str> = devtools_specs().into_iter().map(|s| s.name).collect();
        assert_eq!(DEVTOOLS_COMMAND_NAMES, spec_names.as_slice());
    }

    #[test]
    fn run_command_queues_action_and_validates() {
        let actions = new_queue();
        run_command("plugin.reload", &json!({ "plugin_id": "p" }), &actions).unwrap();
        let queued = std::mem::take(&mut *actions.borrow_mut());
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].name, "plugin.reload");
        assert_eq!(queued[0].arg_str("plugin_id"), Some("p"));
    }

    #[test]
    fn clear_state_rejects_unknown_surface() {
        let actions = new_queue();
        let err = run_command(
            "plugin.clear_state",
            &json!({ "plugin_id": "p", "surfaces": "state,nope" }),
            &actions,
        )
        .unwrap_err();
        assert!(err.contains("nope"), "got {err}");
    }

    #[test]
    fn clear_state_default_surfaces_skip_secrets() {
        // The descriptor schema fills in `surfaces = "state,cache"`
        // when the dispatcher validates args. This unit test calls
        // `run_command` directly, so we pass the merged args (the
        // shape the dispatcher would have produced) explicitly.
        let actions = new_queue();
        run_command(
            "plugin.clear_state",
            &json!({ "plugin_id": "p", "surfaces": "state,cache" }),
            &actions,
        )
        .unwrap();
        let queued = std::mem::take(&mut *actions.borrow_mut());
        assert_eq!(queued.len(), 1);
        let surfaces = queued[0]
            .arg_str("surfaces")
            .map(|s| s.split(',').map(str::trim).collect::<Vec<_>>())
            .unwrap_or_default();
        assert!(surfaces.contains(&"state"));
        assert!(surfaces.contains(&"cache"));
        assert!(!surfaces.contains(&"secrets"));
    }

    #[test]
    fn arg_str_returns_value_or_none() {
        let action = DevtoolsAction::new("x", json!({}));
        assert_eq!(action.arg_str("missing"), None);
        let action_set = DevtoolsAction::new("x", json!({ "k": "v" }));
        assert_eq!(action_set.arg_str("k"), Some("v"));
    }
}

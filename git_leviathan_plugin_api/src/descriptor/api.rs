//! Self-describing host Lua API descriptors.
//!
//! The tables in this module are the source of truth for v1 host APIs
//! exposed under the `leviathan.*` Lua global. Runtime introspection,
//! `leviathan.has`, Lua annotations, Markdown docs, and generated
//! validation metadata all read from these descriptors.

use serde::Serialize;

use crate::api_version::HOST_API_VERSION;
use crate::descriptor::region::{RegionKind, REGIONS};
use crate::descriptor::widget::{WidgetDescriptor, WIDGETS};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ApiVersionInfo {
    pub major: u32,
    pub minor: u32,
    pub label: &'static str,
    pub compatibility: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ApiParam {
    pub name: &'static str,
    pub lua_type: &'static str,
    pub required: bool,
    pub doc: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ApiReturn {
    pub lua_type: &'static str,
    pub doc: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ApiValidation {
    pub args: &'static [&'static str],
    pub returns: &'static [&'static str],
    pub notes: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ApiFunction {
    pub path: &'static str,
    pub name: &'static str,
    pub since: &'static str,
    pub compatibility: &'static str,
    pub doc: &'static str,
    pub params: &'static [ApiParam],
    pub returns: &'static [ApiReturn],
    pub capabilities: &'static [&'static str],
    pub validation: ApiValidation,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ApiEvent {
    pub name: &'static str,
    pub since: &'static str,
    pub doc: &'static str,
    /// Top-level Lua type of the payload table the host hands the
    /// callback. Always `"table"` for the typed Phase 7 events; legacy
    /// v1 aliases inherit this from the canonical event they alias.
    pub payload_type: &'static str,
    /// Schema for the payload table fields. `&[]` for events with no
    /// payload (the host still hands the callback an empty table so
    /// the call signature stays uniform).
    pub payload_fields: &'static [ApiTypeField],
    /// Compatibility aliases — when the host fires this event, every
    /// listed alias also fires for v1 listeners. Empty for events
    /// introduced in Phase 7 with no v1 predecessor, and for events
    /// that *are* themselves aliases.
    pub aliases: &'static [&'static str],
    /// True for legacy v1 names retained as aliases of a canonical
    /// Phase 7 event. The runtime registry skips firing them as
    /// canonicals (they only fire when their canonical fires).
    pub is_alias: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ApiTypeField {
    pub name: &'static str,
    pub lua_type: &'static str,
    pub required: bool,
    pub doc: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ApiTypeMethod {
    pub name: &'static str,
    pub doc: &'static str,
    pub params: &'static [ApiParam],
    pub returns: &'static [ApiReturn],
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ApiType {
    pub name: &'static str,
    pub since: &'static str,
    pub doc: &'static str,
    pub fields: &'static [ApiTypeField],
    pub methods: &'static [ApiTypeMethod],
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ApiCapability {
    pub name: &'static str,
    pub since: &'static str,
    pub doc: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ApiModule {
    pub name: &'static str,
    pub table: &'static str,
    pub version: ApiVersionInfo,
    pub doc: &'static str,
    pub functions: &'static [ApiFunction],
    pub events: &'static [&'static str],
    pub types: &'static [&'static str],
    pub capabilities: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiRegion {
    pub name: &'static str,
    pub kind: &'static str,
    pub sections: Vec<&'static str>,
    pub panes: Vec<ApiRegionPane>,
    /// Phase 17: dynamic section prefixes valid at the region (chrome)
    /// level. Each entry like `"section:"` allows `"section:<id>"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_section_prefixes: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiRegionPane {
    pub name: &'static str,
    pub sections: Vec<&'static str>,
    /// Phase 17: dynamic section prefixes valid in this pane.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_section_prefixes: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostApiDescription {
    pub host_api_version: ApiVersionInfo,
    pub modules: &'static [ApiModule],
    pub events: &'static [ApiEvent],
    pub types: &'static [ApiType],
    pub capabilities: &'static [ApiCapability],
    pub regions: Vec<ApiRegion>,
    pub widgets: Vec<&'static WidgetDescriptor>,
}

const V1: ApiVersionInfo = ApiVersionInfo {
    major: 1,
    minor: 0,
    label: "1.0",
    compatibility: "v1",
};

const NO_VALIDATION: ApiValidation = ApiValidation {
    args: &[],
    returns: &[],
    notes: &[],
};

const STRING_PATH: &[ApiParam] = &[ApiParam {
    name: "path",
    lua_type: "string",
    required: true,
    doc: "Path string.",
}];

const STRING_NAME: &[ApiParam] = &[ApiParam {
    name: "name",
    lua_type: "string",
    required: true,
    doc: "Name string.",
}];

const BOOL_RET: &[ApiReturn] = &[ApiReturn {
    lua_type: "boolean",
    doc: "Boolean result.",
}];

const NILABLE_STRING_ERR_RET: &[ApiReturn] = &[
    ApiReturn {
        lua_type: "string|nil",
        doc: "Value on success, nil on failure.",
    },
    ApiReturn {
        lua_type: "string|nil",
        doc: "Error message on failure.",
    },
];

const OK_ERR_RET: &[ApiReturn] = &[
    ApiReturn {
        lua_type: "boolean",
        doc: "True on success, false on failure.",
    },
    ApiReturn {
        lua_type: "string|nil",
        doc: "Error message on failure.",
    },
];

const ROOT_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.has",
        name: "has",
        since: "1.0",
        compatibility: "v1",
        doc: "Return true when the host exposes a descriptor feature such as `fs.read_file@1`.",
        params: &[ApiParam {
            name: "feature",
            lua_type: "string",
            required: true,
            doc: "`module.feature@major` descriptor query.",
        }],
        returns: BOOL_RET,
        capabilities: &[],
        validation: ApiValidation {
            args: &["feature must be a string in module.feature@version form"],
            returns: &["boolean"],
            notes: &["Malformed feature strings return false."],
        },
    },
    ApiFunction {
        path: "leviathan.log",
        name: "log",
        since: "1.0",
        compatibility: "v1",
        doc: "Log a message via the host.",
        params: &[ApiParam {
            name: "message",
            lua_type: "string",
            required: true,
            doc: "Message to log to host stderr.",
        }],
        returns: &[],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.schedule",
        name: "schedule",
        since: "1.12",
        compatibility: "v1",
        doc: "Defer a callback to the next tick (top-level alias of `leviathan.api.schedule`).",
        params: &[ApiParam {
            name: "callback",
            lua_type: "fun()",
            required: true,
            doc: "Function invoked on the next tick.",
        }],
        returns: &[],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
];

const API_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.api.describe",
        name: "describe",
        since: "1.0",
        compatibility: "v1",
        doc: "Return the full host API descriptor table used for generated docs and validation metadata.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "table",
            doc: "Host API descriptor.",
        }],
        capabilities: &[],
        validation: ApiValidation {
            args: &[],
            returns: &["HostApiDescription"],
            notes: &["Shape is generated from git_leviathan_plugin_api::descriptor."],
        },
    },
    ApiFunction {
        path: "leviathan.api.create_autocmd",
        name: "create_autocmd",
        since: "1.0",
        compatibility: "v1",
        doc: "Subscribe to host events. Callback fires once per event firing.",
        params: &[
            ApiParam {
                name: "events",
                lua_type: "string[]",
                required: true,
                doc: "List of event names to subscribe to.",
            },
            ApiParam {
                name: "opts",
                lua_type: "LeviathanAutocmdOpts",
                required: true,
                doc: "Options table containing `callback`.",
            },
        ],
        returns: &[],
        capabilities: &[],
        validation: ApiValidation {
            args: &["events must be an array-like table of strings", "opts.callback must be a function"],
            returns: &[],
            notes: &["One registry callback is stored per event."],
        },
    },
    ApiFunction {
        path: "leviathan.api.schedule",
        name: "schedule",
        since: "1.0",
        compatibility: "v1",
        doc: "Run a Lua callback on the next plugin host tick.",
        params: &[ApiParam {
            name: "callback",
            lua_type: "fun()",
            required: true,
            doc: "Callback to enqueue.",
        }],
        returns: &[],
        capabilities: &[],
        validation: ApiValidation {
            args: &["callback must be a function"],
            returns: &[],
            notes: &["The callback is recorded in the resource ledger as an async job."],
        },
    },
    ApiFunction {
        path: "leviathan.api.defer_fn",
        name: "defer_fn",
        since: "1.0",
        compatibility: "v1",
        doc: "Run a Lua callback after a host-side millisecond delay.",
        params: &[
            ApiParam {
                name: "ms",
                lua_type: "integer",
                required: true,
                doc: "Delay in milliseconds.",
            },
            ApiParam {
                name: "callback",
                lua_type: "fun()",
                required: true,
                doc: "Callback to enqueue.",
            },
        ],
        returns: &[],
        capabilities: &[],
        validation: ApiValidation {
            args: &["ms must be an integer", "callback must be a function"],
            returns: &[],
            notes: &["The callback is recorded in the resource ledger as a timer."],
        },
    },
    ApiFunction {
        path: "leviathan.api.create_user_command",
        name: "create_user_command",
        since: "1.0",
        compatibility: "v1",
        doc: "Register a named coroutine-driven plugin command.",
        params: &[
            ApiParam {
                name: "name",
                lua_type: "string",
                required: true,
                doc: "Command name.",
            },
            ApiParam {
                name: "callback",
                lua_type: "fun()",
                required: true,
                doc: "Command callback.",
            },
        ],
        returns: &[],
        capabilities: &[],
        validation: ApiValidation {
            args: &["name must be a string", "callback must be a function"],
            returns: &[],
            notes: &["Commands can yield; resumable coroutines are parked in the plugin queue."],
        },
    },
];

const UI_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.ui.list_regions",
        name: "list_regions",
        since: "1.0",
        compatibility: "v1",
        doc: "List names of registered UI regions.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "string[]",
            doc: "Region names in descriptor order.",
        }],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.region",
        name: "region",
        since: "1.0",
        compatibility: "v1",
        doc: "Look up a region handle by descriptor name.",
        params: &[ApiParam {
            name: "name",
            lua_type: "string",
            required: true,
            doc: "Region name.",
        }],
        returns: &[ApiReturn {
            lua_type: "table",
            doc: "Region handle with add/remove/replace.",
        }],
        capabilities: &[],
        validation: ApiValidation {
            args: &["name must match a region descriptor"],
            returns: &["region handle table"],
            notes: &["Unknown names raise an error."],
        },
    },
    ApiFunction {
        path: "leviathan.ui.register_screen",
        name: "register_screen",
        since: "1.0",
        compatibility: "v1",
        doc: "Register a plugin screen with init/view/update lifecycle callbacks.",
        params: &[ApiParam {
            name: "spec",
            lua_type: "LeviathanScreenSpec",
            required: true,
            doc: "Screen descriptor.",
        }],
        returns: &[],
        capabilities: &[],
        validation: ApiValidation {
            args: &[
                "spec.id must be a string",
                "spec.init/view/update must be functions",
            ],
            returns: &[],
            notes: &["serialize and deserialize are optional functions."],
        },
    },
    // Phase 17 extension-point APIs (overlay / context_menu /
    // graph_decoration / diff_decoration). Same module table.
    ApiFunction {
        path: "leviathan.ui.overlay",
        name: "overlay",
        since: "1.0",
        compatibility: "v1",
        doc: "Register an overlay widget the host renders above the active screen.",
        params: UI_OVERLAY_PARAM,
        returns: &[],
        capabilities: UI_OVERLAY_CAP,
        validation: ApiValidation {
            args: &[
                "spec.id must be a string",
                "spec.widget must be a LeviathanWidget table",
                "spec.dismissible must be boolean",
                "spec.priority must be a number",
            ],
            returns: &[],
            notes: &["Host owns Esc / click-outside dismissal."],
        },
    },
    ApiFunction {
        path: "leviathan.ui.context_menu",
        name: "context_menu",
        since: "1.0",
        compatibility: "v1",
        doc: "Contribute a context-menu item at an extension point.",
        params: UI_CONTEXT_MENU_PARAMS,
        returns: &[],
        capabilities: UI_CONTEXT_MENU_CAP,
        validation: ApiValidation {
            args: &[
                "region must address a known *.context_menu section",
                "item.id / label / command must be strings",
                "item.priority must be a number",
            ],
            returns: &[],
            notes: &["Items sorted by priority ascending; capability-gated by condition_capability if set."],
        },
    },
    ApiFunction {
        path: "leviathan.ui.graph_decoration",
        name: "graph_decoration",
        since: "1.0",
        compatibility: "v1",
        doc: "Attach a decoration to a commit row (badge / icon / marker / lane).",
        params: UI_GRAPH_DECORATION_PARAMS,
        returns: &[],
        capabilities: UI_GRAPH_DECORATION_CAP,
        validation: ApiValidation {
            args: &["decoration.kind must be one of badge / icon / marker / lane"],
            returns: &[],
            notes: &["Bound to repository.graph.row:<commit_hash>."],
        },
    },
    ApiFunction {
        path: "leviathan.ui.diff_decoration",
        name: "diff_decoration",
        since: "1.0",
        compatibility: "v1",
        doc: "Attach a decoration to a diff line / hunk (line_hint / hunk_badge / line_gutter).",
        params: UI_DIFF_DECORATION_PARAMS,
        returns: &[],
        capabilities: UI_DIFF_DECORATION_CAP,
        validation: ApiValidation {
            args: &["decoration.kind must be one of line_hint / hunk_badge / line_gutter"],
            returns: &[],
            notes: &["Bound to repository.diff.line:<file>:<line> or repository.diff.hunk:<id>."],
        },
    },
];

const REGION_ADD_PARAM: &[ApiParam] = &[ApiParam {
    name: "spec",
    lua_type: "LeviathanSlotSpec",
    required: true,
    doc: "Slot descriptor.",
}];

const REGION_REMOVE_PARAM: &[ApiParam] = &[ApiParam {
    name: "target",
    lua_type: "LeviathanSlotTarget",
    required: true,
    doc: "Slot address and id.",
}];

const REGION_REPLACE_PARAMS: &[ApiParam] = &[
    ApiParam {
        name: "target",
        lua_type: "LeviathanSlotTarget",
        required: true,
        doc: "Existing slot address and id.",
    },
    ApiParam {
        name: "spec",
        lua_type: "LeviathanSlotSpec",
        required: true,
        doc: "Replacement slot descriptor.",
    },
];

const REGION_VALIDATION: ApiValidation = ApiValidation {
    args: &[
        "section must match the region descriptor",
        "content regions require pane",
        "widget must validate as a LeviathanWidget or be a function",
    ],
    returns: &[],
    notes: &["Slot ownership is recorded in the Phase 1 resource ledger."],
};

const MAIN_BAR_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.ui.main_bar.add",
        name: "add",
        since: "1.0",
        compatibility: "v1",
        doc: "Add a slot to the main bar region.",
        params: REGION_ADD_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.main_bar.remove",
        name: "remove",
        since: "1.0",
        compatibility: "v1",
        doc: "Remove a slot from the main bar region.",
        params: REGION_REMOVE_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.main_bar.replace",
        name: "replace",
        since: "1.0",
        compatibility: "v1",
        doc: "Replace a slot in the main bar region.",
        params: REGION_REPLACE_PARAMS,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
];

const TAB_BAR_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.ui.tab_bar.add",
        name: "add",
        since: "1.0",
        compatibility: "v1",
        doc: "Add a slot to the tab bar region.",
        params: REGION_ADD_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.tab_bar.remove",
        name: "remove",
        since: "1.0",
        compatibility: "v1",
        doc: "Remove a slot from the tab bar region.",
        params: REGION_REMOVE_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.tab_bar.replace",
        name: "replace",
        since: "1.0",
        compatibility: "v1",
        doc: "Replace a slot in the tab bar region.",
        params: REGION_REPLACE_PARAMS,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
];

const REPOSITORY_REGION_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.ui.repository.add",
        name: "add",
        since: "1.0",
        compatibility: "v1",
        doc: "Add a slot to the repository content region.",
        params: REGION_ADD_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.repository.remove",
        name: "remove",
        since: "1.0",
        compatibility: "v1",
        doc: "Remove a slot from the repository content region.",
        params: REGION_REMOVE_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.repository.replace",
        name: "replace",
        since: "1.0",
        compatibility: "v1",
        doc: "Replace a slot in the repository content region.",
        params: REGION_REPLACE_PARAMS,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
];

// Phase 17: status_bar / repository.graph / repository.details /
// repository.diff descriptor function tables. Each region exposes
// the same {add, remove, replace} surface as the existing chrome /
// content regions.
const STATUS_BAR_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.ui.status_bar.add",
        name: "add",
        since: "1.0",
        compatibility: "v1",
        doc: "Add a slot to the status bar region.",
        params: REGION_ADD_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.status_bar.remove",
        name: "remove",
        since: "1.0",
        compatibility: "v1",
        doc: "Remove a slot from the status bar region.",
        params: REGION_REMOVE_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.status_bar.replace",
        name: "replace",
        since: "1.0",
        compatibility: "v1",
        doc: "Replace a slot in the status bar region.",
        params: REGION_REPLACE_PARAMS,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
];

const REPOSITORY_GRAPH_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.ui.repository.graph.add",
        name: "add",
        since: "1.0",
        compatibility: "v1",
        doc: "Add a slot to the repository.graph region (commit row decorations).",
        params: REGION_ADD_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.repository.graph.remove",
        name: "remove",
        since: "1.0",
        compatibility: "v1",
        doc: "Remove a slot from the repository.graph region.",
        params: REGION_REMOVE_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.repository.graph.replace",
        name: "replace",
        since: "1.0",
        compatibility: "v1",
        doc: "Replace a slot in the repository.graph region.",
        params: REGION_REPLACE_PARAMS,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
];

const REPOSITORY_DETAILS_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.ui.repository.details.add",
        name: "add",
        since: "1.0",
        compatibility: "v1",
        doc: "Add a slot to the repository.details region (commit header / files).",
        params: REGION_ADD_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.repository.details.remove",
        name: "remove",
        since: "1.0",
        compatibility: "v1",
        doc: "Remove a slot from the repository.details region.",
        params: REGION_REMOVE_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.repository.details.replace",
        name: "replace",
        since: "1.0",
        compatibility: "v1",
        doc: "Replace a slot in the repository.details region.",
        params: REGION_REPLACE_PARAMS,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
];

const REPOSITORY_DIFF_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.ui.repository.diff.add",
        name: "add",
        since: "1.0",
        compatibility: "v1",
        doc: "Add a slot to the repository.diff region (toolbar / hunk / line / context menu).",
        params: REGION_ADD_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.repository.diff.remove",
        name: "remove",
        since: "1.0",
        compatibility: "v1",
        doc: "Remove a slot from the repository.diff region.",
        params: REGION_REMOVE_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.repository.diff.replace",
        name: "replace",
        since: "1.0",
        compatibility: "v1",
        doc: "Replace a slot in the repository.diff region.",
        params: REGION_REPLACE_PARAMS,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
];

// Phase 17: cross-region extension-point APIs (overlays, context menu
// items, graph decorations, diff decorations). Each surface is
// capability-gated: see `UI_EXT_*_CAP`.
const UI_OVERLAY_CAP: &[&str] = &["ui:overlay"];
const UI_CONTEXT_MENU_CAP: &[&str] = &["ui:context_menu"];
const UI_GRAPH_DECORATION_CAP: &[&str] = &["ui:graph_decoration"];
const UI_DIFF_DECORATION_CAP: &[&str] = &["ui:diff_decoration"];

const UI_OVERLAY_PARAM: &[ApiParam] = &[ApiParam {
    name: "spec",
    lua_type: "LeviathanOverlaySpec",
    required: true,
    doc: "Overlay descriptor (id, widget, dismissible, priority).",
}];

const UI_CONTEXT_MENU_PARAMS: &[ApiParam] = &[
    ApiParam {
        name: "region",
        lua_type: "string",
        required: true,
        doc: "Extension point address (e.g. \"repository.diff.context_menu\").",
    },
    ApiParam {
        name: "item",
        lua_type: "LeviathanContextMenuItem",
        required: true,
        doc: "Menu item (id, label, command, priority, condition_capability).",
    },
];

const UI_GRAPH_DECORATION_PARAMS: &[ApiParam] = &[
    ApiParam {
        name: "commit_hash",
        lua_type: "string",
        required: true,
        doc: "Commit row hash the decoration applies to.",
    },
    ApiParam {
        name: "decoration",
        lua_type: "LeviathanGraphDecoration",
        required: true,
        doc: "Decoration AST: badge / icon / marker / lane.",
    },
];

const UI_DIFF_DECORATION_PARAMS: &[ApiParam] = &[ApiParam {
    name: "decoration",
    lua_type: "LeviathanDiffDecoration",
    required: true,
    doc: "Decoration AST: line_hint / hunk_badge / line_gutter.",
}];

const FS_READ_CAP: &[&str] = &["fs:read"];
const FS_WRITE_CAP: &[&str] = &["fs:write:*"];
const FS_READ_WRITE_CAP: &[&str] = &["fs:read", "fs:write:*"];
const ENV_CAP: &[&str] = &["env"];

const FS_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction { path: "leviathan.fs.read_file", name: "read_file", since: "1.0", compatibility: "v1", doc: "Read UTF-8 file contents. Returns content or (nil, err).", params: STRING_PATH, returns: NILABLE_STRING_ERR_RET, capabilities: FS_READ_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.read_lines", name: "read_lines", since: "1.0", compatibility: "v1", doc: "Read a UTF-8 file as an array of lines.", params: STRING_PATH, returns: &[ApiReturn { lua_type: "string[]|nil", doc: "Lines on success." }, ApiReturn { lua_type: "string|nil", doc: "Error message on failure." }], capabilities: FS_READ_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.write_file", name: "write_file", since: "1.0", compatibility: "v1", doc: "Write content to a file, replacing existing contents.", params: &[ApiParam { name: "path", lua_type: "string", required: true, doc: "Target path." }, ApiParam { name: "content", lua_type: "string", required: true, doc: "UTF-8 content." }], returns: OK_ERR_RET, capabilities: FS_WRITE_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.append_file", name: "append_file", since: "1.0", compatibility: "v1", doc: "Append content to a file, creating it when absent.", params: &[ApiParam { name: "path", lua_type: "string", required: true, doc: "Target path." }, ApiParam { name: "content", lua_type: "string", required: true, doc: "UTF-8 content." }], returns: OK_ERR_RET, capabilities: FS_WRITE_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.delete", name: "delete", since: "1.0", compatibility: "v1", doc: "Delete a file or directory tree.", params: STRING_PATH, returns: OK_ERR_RET, capabilities: FS_WRITE_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.mkdir", name: "mkdir", since: "1.0", compatibility: "v1", doc: "Create a directory and missing parents.", params: STRING_PATH, returns: OK_ERR_RET, capabilities: FS_WRITE_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.copy", name: "copy", since: "1.0", compatibility: "v1", doc: "Copy a regular file.", params: &[ApiParam { name: "src", lua_type: "string", required: true, doc: "Source path." }, ApiParam { name: "dst", lua_type: "string", required: true, doc: "Destination path." }], returns: OK_ERR_RET, capabilities: FS_READ_WRITE_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.rename", name: "rename", since: "1.0", compatibility: "v1", doc: "Move or rename a path.", params: &[ApiParam { name: "src", lua_type: "string", required: true, doc: "Source path." }, ApiParam { name: "dst", lua_type: "string", required: true, doc: "Destination path." }], returns: OK_ERR_RET, capabilities: FS_WRITE_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.touch", name: "touch", since: "1.0", compatibility: "v1", doc: "Create a file or update its modification time.", params: STRING_PATH, returns: OK_ERR_RET, capabilities: FS_WRITE_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.read_link", name: "read_link", since: "1.0", compatibility: "v1", doc: "Read a symlink target without following it.", params: STRING_PATH, returns: NILABLE_STRING_ERR_RET, capabilities: FS_READ_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.list_dir", name: "list_dir", since: "1.0", compatibility: "v1", doc: "List directory entries sorted with directories first.", params: STRING_PATH, returns: &[ApiReturn { lua_type: "LeviathanFsEntry[]|nil", doc: "Directory entries on success." }, ApiReturn { lua_type: "string|nil", doc: "Error message on failure." }], capabilities: FS_READ_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.exists", name: "exists", since: "1.0", compatibility: "v1", doc: "Return whether a path exists.", params: STRING_PATH, returns: BOOL_RET, capabilities: FS_READ_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.is_file", name: "is_file", since: "1.0", compatibility: "v1", doc: "Return whether a path is a regular file.", params: STRING_PATH, returns: BOOL_RET, capabilities: FS_READ_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.is_dir", name: "is_dir", since: "1.0", compatibility: "v1", doc: "Return whether a path is a directory.", params: STRING_PATH, returns: BOOL_RET, capabilities: FS_READ_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.is_symlink", name: "is_symlink", since: "1.0", compatibility: "v1", doc: "Return whether a path itself is a symlink.", params: STRING_PATH, returns: BOOL_RET, capabilities: FS_READ_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.size", name: "size", since: "1.0", compatibility: "v1", doc: "Return symlink metadata size in bytes.", params: STRING_PATH, returns: &[ApiReturn { lua_type: "integer|nil", doc: "Byte size on success." }, ApiReturn { lua_type: "string|nil", doc: "Error message on failure." }], capabilities: FS_READ_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.modified", name: "modified", since: "1.0", compatibility: "v1", doc: "Return modification time as Unix seconds.", params: STRING_PATH, returns: &[ApiReturn { lua_type: "number|nil", doc: "Unix timestamp on success." }, ApiReturn { lua_type: "string|nil", doc: "Error message on failure." }], capabilities: FS_READ_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.metadata", name: "metadata", since: "1.0", compatibility: "v1", doc: "Return metadata shaped like a single list_dir entry.", params: STRING_PATH, returns: &[ApiReturn { lua_type: "LeviathanFsEntry|nil", doc: "Entry metadata on success." }, ApiReturn { lua_type: "string|nil", doc: "Error message on failure." }], capabilities: FS_READ_CAP, validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.is_absolute", name: "is_absolute", since: "1.0", compatibility: "v1", doc: "Return whether a path string is absolute.", params: STRING_PATH, returns: BOOL_RET, capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.parent", name: "parent", since: "1.0", compatibility: "v1", doc: "Return the parent path string, or nil.", params: STRING_PATH, returns: &[ApiReturn { lua_type: "string|nil", doc: "Parent path." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.basename", name: "basename", since: "1.0", compatibility: "v1", doc: "Return the final path component, or nil.", params: STRING_PATH, returns: &[ApiReturn { lua_type: "string|nil", doc: "File name." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.stem", name: "stem", since: "1.0", compatibility: "v1", doc: "Return the file stem, or nil.", params: STRING_PATH, returns: &[ApiReturn { lua_type: "string|nil", doc: "File stem." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.extension", name: "extension", since: "1.0", compatibility: "v1", doc: "Return the final file extension, or nil.", params: STRING_PATH, returns: &[ApiReturn { lua_type: "string|nil", doc: "File extension." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.join", name: "join", since: "1.0", compatibility: "v1", doc: "Join two path strings.", params: &[ApiParam { name: "a", lua_type: "string", required: true, doc: "Base path." }, ApiParam { name: "b", lua_type: "string", required: true, doc: "Child path." }], returns: &[ApiReturn { lua_type: "string", doc: "Joined path." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.relative_to", name: "relative_to", since: "1.0", compatibility: "v1", doc: "Return path relative to base when path is under base.", params: &[ApiParam { name: "path", lua_type: "string", required: true, doc: "Path to rewrite." }, ApiParam { name: "base", lua_type: "string", required: true, doc: "Base directory." }], returns: &[ApiReturn { lua_type: "string|nil", doc: "Relative path or nil." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.with_extension", name: "with_extension", since: "1.0", compatibility: "v1", doc: "Return path with its final extension replaced.", params: &[ApiParam { name: "path", lua_type: "string", required: true, doc: "Path to rewrite." }, ApiParam { name: "ext", lua_type: "string", required: true, doc: "New extension." }], returns: &[ApiReturn { lua_type: "string|nil", doc: "Rewritten path or nil." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.with_file_name", name: "with_file_name", since: "1.0", compatibility: "v1", doc: "Return path with its final component replaced.", params: &[ApiParam { name: "path", lua_type: "string", required: true, doc: "Path to rewrite." }, ApiParam { name: "name", lua_type: "string", required: true, doc: "Replacement file name." }], returns: &[ApiReturn { lua_type: "string|nil", doc: "Rewritten path or nil." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.cwd", name: "cwd", since: "1.0", compatibility: "v1", doc: "Return the process current working directory.", params: &[], returns: NILABLE_STRING_ERR_RET, capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.home", name: "home", since: "1.0", compatibility: "v1", doc: "Return the user home directory, or nil.", params: &[], returns: &[ApiReturn { lua_type: "string|nil", doc: "Home directory." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.temp_dir", name: "temp_dir", since: "1.0", compatibility: "v1", doc: "Return the process temporary directory.", params: &[], returns: &[ApiReturn { lua_type: "string", doc: "Temporary directory." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.config_dir", name: "config_dir", since: "1.0", compatibility: "v1", doc: "Return the standard user config directory, or nil.", params: &[], returns: &[ApiReturn { lua_type: "string|nil", doc: "Config directory." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.cache_dir", name: "cache_dir", since: "1.0", compatibility: "v1", doc: "Return the standard user cache directory, or nil.", params: &[], returns: &[ApiReturn { lua_type: "string|nil", doc: "Cache directory." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.data_dir", name: "data_dir", since: "1.0", compatibility: "v1", doc: "Return the standard user data directory, or nil.", params: &[], returns: &[ApiReturn { lua_type: "string|nil", doc: "Data directory." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.state_dir", name: "state_dir", since: "1.0", compatibility: "v1", doc: "Return the standard user state directory, or nil.", params: &[], returns: &[ApiReturn { lua_type: "string|nil", doc: "State directory." }], capabilities: &[], validation: NO_VALIDATION },
    ApiFunction { path: "leviathan.fs.canonicalize", name: "canonicalize", since: "1.0", compatibility: "v1", doc: "Resolve a path to an absolute canonical path.", params: STRING_PATH, returns: NILABLE_STRING_ERR_RET, capabilities: &[], validation: ApiValidation { args: &["path must be a string"], returns: &["string|nil", "string|nil"], notes: &["Compatibility note: v1 canonicalize is ungated even though it reads filesystem state."] } },
    ApiFunction {
        path: "leviathan.fs.watch",
        name: "watch",
        since: "1.12",
        compatibility: "v1",
        doc: "Watch a path for filesystem changes and dispatch events to a callback.",
        params: &[
            ApiParam { name: "path", lua_type: "string", required: true, doc: "Path to watch (file or directory)." },
            ApiParam { name: "opts", lua_type: "LeviathanFsWatchOpts|nil", required: false, doc: "{ recursive = bool }." },
            ApiParam { name: "callback", lua_type: "fun(event: LeviathanFsWatchEvent)", required: true, doc: "Callback invoked on each event." },
        ],
        returns: &[ApiReturn { lua_type: "LeviathanFsWatchHandle", doc: "Handle with `:cancel()` method." }],
        capabilities: &["fs:watch"],
        validation: ApiValidation {
            args: &["path must resolve under a granted fs:watch scope"],
            returns: &["FsWatchHandle"],
            notes: &["Cancel via the returned handle; the host also cancels on plugin reload/unload."],
        },
    },
];

const GIT_READ_STATUS_CAP: &[&str] = &["git:read:status"];
const GIT_READ_LOG_CAP: &[&str] = &["git:read:log"];
const GIT_READ_DIFF_CAP: &[&str] = &["git:read:diff"];
const GIT_READ_SHOW_CAP: &[&str] = &["git:read:show"];
const GIT_READ_BLAME_CAP: &[&str] = &["git:read:blame"];
const GIT_WRITE_CHECKOUT_CAP: &[&str] = &["git:write:checkout"];
const GIT_WRITE_BRANCH_CAP: &[&str] = &["git:write:branch"];
const GIT_WRITE_TAG_CAP: &[&str] = &["git:write:tag"];
const GIT_WRITE_COMMIT_CAP: &[&str] = &["git:write:commit"];
const GIT_WRITE_STASH_CAP: &[&str] = &["git:write:stash"];
const GIT_WRITE_RESET_CAP: &[&str] = &["git:write:reset"];
const GIT_WRITE_FETCH_CAP: &[&str] = &["git:write:fetch"];
const GIT_WRITE_PUSH_CAP: &[&str] = &["git:write:push"];
const GIT_WRITE_MERGE_CAP: &[&str] = &["git:write:merge"];
const GIT_WRITE_REBASE_CAP: &[&str] = &["git:write:rebase"];

const REPOSITORY_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.repository.current",
        name: "current",
        since: "1.0",
        compatibility: "v1",
        doc: "Return the cached `leviathan.repository` snapshot table for the active repo.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "table",
            doc: "Active repository snapshot.",
        }],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.repository.refs",
        name: "refs",
        since: "1.0",
        compatibility: "v1",
        doc: "Return the active repository's typed refs (locals, remotes, tags). Returns (refs, nil) or (nil, err).",
        params: &[],
        returns: &[
            ApiReturn { lua_type: "table|nil", doc: "Refs snapshot on success." },
            ApiReturn { lua_type: "string|nil", doc: "Error message on failure." },
        ],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.repository.head",
        name: "head",
        since: "1.0",
        compatibility: "v1",
        doc: "Return the active repository's HEAD info: { hash, ref, detached }.",
        params: &[],
        returns: &[
            ApiReturn { lua_type: "table|nil", doc: "HEAD snapshot on success." },
            ApiReturn { lua_type: "string|nil", doc: "Error message on failure." },
        ],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.repository.status",
        name: "status",
        since: "1.0",
        compatibility: "v1",
        doc: "Return the working-tree status: { staged[], unstaged[], conflicted[] }.",
        params: &[],
        returns: &[
            ApiReturn { lua_type: "table|nil", doc: "Status snapshot on success." },
            ApiReturn { lua_type: "string|nil", doc: "Error message on failure." },
        ],
        capabilities: GIT_READ_STATUS_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.repository.commits",
        name: "commits",
        since: "1.0",
        compatibility: "v1",
        doc: "Return up to `limit` commits from the active repository. Each entry: { hash, summary, author, timestamp, parents }.",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: false,
            doc: "Options table: { limit = 100, rev = \"HEAD\" }.",
        }],
        returns: &[
            ApiReturn { lua_type: "table[]|nil", doc: "Commit list on success." },
            ApiReturn { lua_type: "string|nil", doc: "Error message on failure." },
        ],
        capabilities: GIT_READ_LOG_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.repository.diff",
        name: "diff",
        since: "1.0",
        compatibility: "v1",
        doc: "Return per-file diff for `commit`. Each entry: { path, status, additions, deletions }.",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "Options table: { commit = hash }.",
        }],
        returns: &[
            ApiReturn { lua_type: "table[]|nil", doc: "Diff snapshot on success." },
            ApiReturn { lua_type: "string|nil", doc: "Error message on failure." },
        ],
        capabilities: GIT_READ_DIFF_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.repository.file_at",
        name: "file_at",
        since: "1.0",
        compatibility: "v1",
        doc: "Return the file at a given commit/path as { lines = string[] } (per-line diff content).",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "Options table: { commit = hash, path = \"src/main.rs\" }.",
        }],
        returns: &[
            ApiReturn { lua_type: "table|nil", doc: "File-at-commit snapshot on success." },
            ApiReturn { lua_type: "string|nil", doc: "Error message on failure." },
        ],
        capabilities: GIT_READ_SHOW_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.repository.blame",
        name: "blame",
        since: "1.0",
        compatibility: "v1",
        doc: "Phase 11 placeholder: blame is not yet wired through the host gateway. Returns (nil, \"unsupported\").",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "Options table: { path = \"src/main.rs\", line_range = {start,end} }.",
        }],
        returns: &[
            ApiReturn { lua_type: "nil", doc: "Always nil." },
            ApiReturn { lua_type: "string", doc: "Error message." },
        ],
        capabilities: GIT_READ_BLAME_CAP,
        validation: NO_VALIDATION,
    },
];

const GIT_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.git.checkout",
        name: "checkout",
        since: "1.0",
        compatibility: "v1",
        doc: "Check out `ref`. Returns (true, nil) on enqueue + success, (false, err) otherwise.",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "{ ref = \"main\" }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_CHECKOUT_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.create_branch",
        name: "create_branch",
        since: "1.0",
        compatibility: "v1",
        doc: "Create branch `name` at `start_point` (defaults to HEAD).",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "{ name = \"topic\", start_point = \"HEAD\" }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_BRANCH_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.delete_branch",
        name: "delete_branch",
        since: "1.0",
        compatibility: "v1",
        doc: "Delete branch `name`. `force = true` requires destructive confirmation.",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "{ name = \"topic\", force = false }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_BRANCH_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.create_tag",
        name: "create_tag",
        since: "1.0",
        compatibility: "v1",
        doc: "Create lightweight tag `name` at `target` (defaults to HEAD).",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "{ name = \"v1.2.3\", target = \"HEAD\" }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_TAG_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.delete_tag",
        name: "delete_tag",
        since: "1.0",
        compatibility: "v1",
        doc: "Delete tag `name` locally. Destructive — requires confirmation.",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "{ name = \"v1.2.3\" }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_TAG_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.commit",
        name: "commit",
        since: "1.0",
        compatibility: "v1",
        doc: "Commit currently-staged changes with `message`.",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "{ message = \"...\" }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_COMMIT_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.stash_push",
        name: "stash_push",
        since: "1.0",
        compatibility: "v1",
        doc: "Stash current dirty changes.",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: false,
            doc: "{ message = \"from plugin\" }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_STASH_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.stash_pop",
        name: "stash_pop",
        since: "1.0",
        compatibility: "v1",
        doc: "Pop the most recent stash entry (or the entry at `index`).",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: false,
            doc: "{ index = 0 }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_STASH_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.reset",
        name: "reset",
        since: "1.0",
        compatibility: "v1",
        doc: "Reset current branch to `ref` with `mode` (soft|mixed|hard). `hard` requires destructive confirmation.",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "{ ref = \"HEAD~1\", mode = \"soft|mixed|hard\" }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_RESET_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.fetch",
        name: "fetch",
        since: "1.0",
        compatibility: "v1",
        doc: "Fetch from `remote` (or every remote when omitted).",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: false,
            doc: "{ remote = \"origin\" }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_FETCH_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.push",
        name: "push",
        since: "1.0",
        compatibility: "v1",
        doc: "Push the current branch to `remote`. Force pushes are not yet supported.",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: false,
            doc: "{ remote = \"origin\", ref = \"main\" }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_PUSH_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.merge",
        name: "merge",
        since: "1.0",
        compatibility: "v1",
        doc: "Merge `ref` into the current branch. Destructive — requires confirmation.",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "{ ref = \"topic\" }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_MERGE_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.git.rebase",
        name: "rebase",
        since: "1.0",
        compatibility: "v1",
        doc: "Rebase the current branch onto `ref`. Destructive — requires confirmation.",
        params: &[ApiParam {
            name: "opts",
            lua_type: "table",
            required: true,
            doc: "{ ref = \"main\" }",
        }],
        returns: OK_ERR_RET,
        capabilities: GIT_WRITE_REBASE_CAP,
        validation: NO_VALIDATION,
    },
];

const ENV_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.env.get",
        name: "get",
        since: "1.0",
        compatibility: "v1",
        doc: "Read an environment variable. Returns (value, nil), (nil, nil), or (nil, err).",
        params: STRING_NAME,
        returns: NILABLE_STRING_ERR_RET,
        capabilities: ENV_CAP,
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.env.list",
        name: "list",
        since: "1.0",
        compatibility: "v1",
        doc: "List UTF-8 environment variables as a name/value table.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "table<string,string>",
            doc: "Environment map.",
        }],
        capabilities: ENV_CAP,
        validation: NO_VALIDATION,
    },
];

const TAB_REGISTRY_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.tab_registry.add",
        name: "add",
        since: "1.0",
        compatibility: "v1",
        doc: "Open or focus a tab for the given path.",
        params: STRING_PATH,
        returns: &[],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.tab_registry.remove",
        name: "remove",
        since: "1.0",
        compatibility: "v1",
        doc: "Close the tab at path.",
        params: STRING_PATH,
        returns: &[],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.tab_registry.select",
        name: "select",
        since: "1.0",
        compatibility: "v1",
        doc: "Focus the tab at path.",
        params: STRING_PATH,
        returns: &[],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.tab_registry.reorder",
        name: "reorder",
        since: "1.0",
        compatibility: "v1",
        doc: "Reorder tabs to match the given path list.",
        params: &[ApiParam {
            name: "paths",
            lua_type: "string[]",
            required: true,
            doc: "Tab paths in target order.",
        }],
        returns: &[],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
];

const SERVICES_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.services.register",
        name: "register",
        since: "1.0",
        compatibility: "v1",
        doc: "Publish a declared inter-plugin service.",
        params: &[
            ApiParam {
                name: "name_or_name_at_version",
                lua_type: "string",
                required: true,
                doc: "Service name (`greeter`) with a separate version, or legacy declaration (`greeter@1`).",
            },
            ApiParam {
                name: "version",
                lua_type: "integer",
                required: false,
                doc: "Integer service version when the first argument is a bare service name.",
            },
            ApiParam {
                name: "methods",
                lua_type: "table<string,function>",
                required: true,
                doc: "Service method table.",
            },
        ],
        returns: &[],
        capabilities: &[],
        validation: ApiValidation {
            args: &[
                "service key must be declared in provides_services",
                "methods values must be functions",
            ],
            returns: &[],
            notes: &["Each method is stored in the resource ledger as a Lua registry key."],
        },
    },
    ApiFunction {
        path: "leviathan.services.get",
        name: "get",
        since: "1.0",
        compatibility: "v1",
        doc: "Look up a declared inter-plugin service proxy.",
        params: &[
            ApiParam {
                name: "name_or_name_at_version",
                lua_type: "string",
                required: true,
                doc: "Service name (`greeter`) with a separate version, or legacy declaration (`greeter@1`).",
            },
            ApiParam {
                name: "version",
                lua_type: "integer",
                required: false,
                doc: "Integer service version when the first argument is a bare service name.",
            },
        ],
        returns: &[ApiReturn {
            lua_type: "table|nil",
            doc: "Service proxy, or nil for declared optional consumers when the provider is absent.",
        }],
        capabilities: &[],
        validation: ApiValidation {
            args: &["service key must be declared in consumes_services"],
            returns: &["table", "nil for optional missing services"],
            notes: &[
                "Proxy method arguments and results are converted through JSON values.",
                "Required consumers are validated at plugin load/reload time.",
                "Provider method calls are traced and run under the caller's active capabilities.",
            ],
        },
    },
];

const PERSIST_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.persist.open",
        name: "open",
        since: "1.0",
        compatibility: "v1",
        doc: "Open or create a versioned per-plugin key-value store.",
        params: &[
            ApiParam {
                name: "name",
                lua_type: "string",
                required: true,
                doc: "Plugin-local store name.",
            },
            ApiParam {
                name: "opts",
                lua_type: "LeviathanPersistOpenOpts|nil",
                required: false,
                doc: "Version, surface, repository key, and migrations.",
            },
        ],
        returns: &[ApiReturn {
            lua_type: "LeviathanPersistStore",
            doc: "Store userdata.",
        }],
        capabilities: &[],
        validation: ApiValidation {
            args: &["name must be a string", "opts.version defaults to 1"],
            returns: &["PersistStore userdata"],
            notes: &["The default state surface preserves the v1 state-dir path."],
        },
    },
    ApiFunction {
        path: "leviathan.persist.transaction",
        name: "transaction",
        since: "1.13",
        compatibility: "v1",
        doc: "Run a persistence transaction and atomically commit only if the callback succeeds.",
        params: &[ApiParam {
            name: "callback_or_opts",
            lua_type: "function|LeviathanPersistOpenOpts",
            required: true,
            doc: "Callback, or options followed by callback.",
        }],
        returns: &[ApiReturn {
            lua_type: "boolean",
            doc: "True after commit.",
        }],
        capabilities: &[],
        validation: ApiValidation {
            args: &["callback receives a transaction userdata"],
            returns: &["boolean"],
            notes: &["Callback errors roll back every staged write."],
        },
    },
];

const SETTINGS_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.settings.define_schema",
        name: "define_schema",
        since: "1.13",
        compatibility: "v1",
        doc: "Declare a schema used to validate plugin settings before save.",
        params: &[ApiParam {
            name: "schema",
            lua_type: "table",
            required: true,
            doc: "Settings schema table.",
        }],
        returns: OK_ERR_RET,
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.settings.get",
        name: "get",
        since: "1.13",
        compatibility: "v1",
        doc: "Return current settings with schema defaults applied.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "table",
            doc: "Settings table.",
        }],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.settings.set",
        name: "set",
        since: "1.13",
        compatibility: "v1",
        doc: "Validate and save settings, then fire on_change callbacks.",
        params: &[ApiParam {
            name: "values",
            lua_type: "table",
            required: true,
            doc: "Settings to merge.",
        }],
        returns: OK_ERR_RET,
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.settings.on_change",
        name: "on_change",
        since: "1.13",
        compatibility: "v1",
        doc: "Register a callback fired after validated settings are saved.",
        params: &[ApiParam {
            name: "callback",
            lua_type: "fun(new_settings: table)",
            required: true,
            doc: "Change callback.",
        }],
        returns: &[],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
];

const SECRETS_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.secrets.set",
        name: "set",
        since: "1.13",
        compatibility: "v1",
        doc: "Store a plugin-local secret in the secrets surface.",
        params: &[
            ApiParam {
                name: "key",
                lua_type: "string",
                required: true,
                doc: "Secret key.",
            },
            ApiParam {
                name: "value",
                lua_type: "string",
                required: true,
                doc: "Secret value.",
            },
        ],
        returns: &[],
        capabilities: &["credentials"],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.secrets.get",
        name: "get",
        since: "1.13",
        compatibility: "v1",
        doc: "Read a plugin-local secret value.",
        params: &[ApiParam {
            name: "key",
            lua_type: "string",
            required: true,
            doc: "Secret key.",
        }],
        returns: &[ApiReturn {
            lua_type: "string|nil",
            doc: "Secret value, or nil.",
        }],
        capabilities: &["credentials"],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.secrets.delete",
        name: "delete",
        since: "1.13",
        compatibility: "v1",
        doc: "Delete a plugin-local secret.",
        params: &[ApiParam {
            name: "key",
            lua_type: "string",
            required: true,
            doc: "Secret key.",
        }],
        returns: &[],
        capabilities: &["credentials"],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.secrets.list",
        name: "list",
        since: "1.13",
        compatibility: "v1",
        doc: "List plugin-local secret keys without exposing values.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "string[]",
            doc: "Secret keys.",
        }],
        capabilities: &["credentials"],
        validation: NO_VALIDATION,
    },
];

const RUNTIME_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.runtime.path",
        name: "path",
        since: "1.0",
        compatibility: "v1",
        doc:
            "Ordered runtime path entries the host will search for `require(\"plugin_id.module\")`.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "LeviathanRuntimePathEntry[]",
            doc: "Entries in search order: own first, then declared dependencies.",
        }],
        capabilities: &[],
        validation: ApiValidation {
            args: &[],
            returns: &["array of {plugin, kind, root}"],
            notes: &[
                "Order is deterministic: own root, then `requires_plugins` in manifest order.",
            ],
        },
    },
    ApiFunction {
        path: "leviathan.runtime.find",
        name: "find",
        since: "1.0",
        compatibility: "v1",
        doc: "Resolve a module name against the runtime path without loading it.",
        params: &[ApiParam {
            name: "module",
            lua_type: "string",
            required: true,
            doc: "Module name (e.g. \"plugin_id.foo\").",
        }],
        returns: &[ApiReturn {
            lua_type: "LeviathanRuntimeMatch|nil",
            doc: "Match info or nil when not found / name is invalid.",
        }],
        capabilities: &[],
        validation: ApiValidation {
            args: &["module must be a syntactically valid dotted name"],
            returns: &["{plugin, kind, source} or nil"],
            notes: &["Returns nil for invalid names; never raises."],
        },
    },
    ApiFunction {
        path: "leviathan.runtime.module_graph",
        name: "module_graph",
        since: "1.0",
        compatibility: "v1",
        doc: "Per-plugin list of modules currently cached in this generation.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "LeviathanRuntimeModuleGraphEntry[]",
            doc: "One entry per plugin contributing cached modules.",
        }],
        capabilities: &[],
        validation: ApiValidation {
            args: &[],
            returns: &["array of {plugin, modules}"],
            notes: &[
                "Reload drops the whole cache, so the graph reflects the live generation only.",
            ],
        },
    },
];

const HEALTH_FUNCTIONS: &[ApiFunction] = &[ApiFunction {
    path: "leviathan.health.register",
    name: "register",
    since: "1.0",
    compatibility: "v1",
    doc: "Register a plugin health-check callback.",
    params: &[ApiParam {
        name: "callback",
        lua_type: "fun(ctx: LeviathanHealthContext)",
        required: true,
        doc: "Health check callback.",
    }],
    returns: &[],
    capabilities: &[],
    validation: ApiValidation {
        args: &["callback must be a function"],
        returns: &[],
        notes: &["Host passes a fresh HealthContext userdata when health checks run."],
    },
}];

const ASYNC_FUNCTIONS: &[ApiFunction] = &[ApiFunction {
    path: "leviathan.async.spawn",
    name: "spawn",
    since: "1.12",
    compatibility: "v1",
    doc: "Spawn a host-managed worker thread running a Lua body. Returns a handle with `:cancel()`.",
    params: &[
        ApiParam {
            name: "body",
            lua_type: "fun(ctx: LeviathanJobContext): any",
            required: true,
            doc: "Worker body. Runs on a background thread on a fresh Lua state. Result must be JSON-serialisable.",
        },
        ApiParam {
            name: "on_complete",
            lua_type: "fun(ok: boolean, value: any)|nil",
            required: false,
            doc: "Optional main-thread callback invoked when the worker finishes.",
        },
    ],
    returns: &[ApiReturn {
        lua_type: "LeviathanJobHandle",
        doc: "Job handle with `:cancel()` and `:id()`.",
    }],
    capabilities: &["async:spawn"],
    validation: ApiValidation {
        args: &["body must be a Lua function (no upvalues)"],
        returns: &["JobHandle"],
        notes: &["Body sees a `ctx:cancelled()` projection of the cancellation token."],
    },
}];

const TIMER_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.timer.after",
        name: "after",
        since: "1.12",
        compatibility: "v1",
        doc: "Schedule a one-shot callback after `ms` milliseconds.",
        params: &[
            ApiParam {
                name: "ms",
                lua_type: "integer",
                required: true,
                doc: "Delay in milliseconds.",
            },
            ApiParam {
                name: "callback",
                lua_type: "fun()",
                required: true,
                doc: "Callback to invoke once.",
            },
        ],
        returns: &[ApiReturn {
            lua_type: "LeviathanTimerHandle",
            doc: "Handle with `:cancel()`.",
        }],
        capabilities: &["timer:create"],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.timer.every",
        name: "every",
        since: "1.12",
        compatibility: "v1",
        doc: "Schedule a repeating callback every `ms` milliseconds.",
        params: &[
            ApiParam {
                name: "ms",
                lua_type: "integer",
                required: true,
                doc: "Period in milliseconds.",
            },
            ApiParam {
                name: "callback",
                lua_type: "fun()",
                required: true,
                doc: "Callback invoked on each fire.",
            },
        ],
        returns: &[ApiReturn {
            lua_type: "LeviathanTimerHandle",
            doc: "Handle with `:cancel()`.",
        }],
        capabilities: &["timer:create"],
        validation: ApiValidation {
            args: &["ms must be > 0"],
            returns: &["TimerHandle"],
            notes: &["Repeating timers re-arm on each tick after firing."],
        },
    },
];

const COMMAND_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.command.create",
        name: "create",
        since: "1.8",
        compatibility: "v1",
        doc: "Register a typed user command in the host registry.",
        params: &[
            ApiParam {
                name: "name",
                lua_type: "string",
                required: true,
                doc: "Command identifier; unique per plugin.",
            },
            ApiParam {
                name: "spec",
                lua_type: "LeviathanCommandSpec",
                required: true,
                doc: "Command descriptor including title, args, run.",
            },
        ],
        returns: &[],
        capabilities: &[],
        validation: ApiValidation {
            args: &[
                "name must be a non-empty string",
                "spec.run must be a function",
                "spec.context, when present, must be a known context name",
                "spec.args must declare valid types",
            ],
            returns: &[],
            notes: &[
                "Commands are owned by the plugin generation and dropped on reload/unload.",
                "Re-registering the same name within a generation replaces the previous spec.",
            ],
        },
    },
    ApiFunction {
        path: "leviathan.command.invoke",
        name: "invoke",
        since: "1.8",
        compatibility: "v1",
        doc: "Invoke a registered command by name through the host dispatcher.",
        params: &[
            ApiParam {
                name: "name",
                lua_type: "string",
                required: true,
                doc: "Command name to invoke.",
            },
            ApiParam {
                name: "args",
                lua_type: "table|nil",
                required: false,
                doc: "Argument table; validated against the command's arg schema.",
            },
        ],
        returns: &[ApiReturn {
            lua_type: "boolean",
            doc: "True when dispatch succeeded; false on validation/capability/run failure.",
        }],
        capabilities: &[],
        validation: ApiValidation {
            args: &[
                "name must be a registered command",
                "args must be a table or nil",
            ],
            returns: &["boolean"],
            notes: &["Argument validation, capability checks, and dispatch happen host-side."],
        },
    },
    ApiFunction {
        path: "leviathan.command.list",
        name: "list",
        since: "1.8",
        compatibility: "v1",
        doc: "List every registered command (host + plugin) as descriptor tables.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "LeviathanCommandSummary[]",
            doc: "Array of command summaries.",
        }],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
];

const KEYMAP_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.keymap.set",
        name: "set",
        since: "1.9",
        compatibility: "v1",
        doc: "Bind a key chord to a registered command in a context.",
        params: &[
            ApiParam {
                name: "context",
                lua_type: "string",
                required: true,
                doc: "Activation context (`global`, `repository`, `repository.graph`, `tab_bar`, etc.).",
            },
            ApiParam {
                name: "key",
                lua_type: "string",
                required: true,
                doc: "Vim-style key sequence (e.g. `gl`, `<C-r>`, `<leader>gh`).",
            },
            ApiParam {
                name: "command",
                lua_type: "string",
                required: true,
                doc: "Command name resolved by the host command registry at dispatch time.",
            },
            ApiParam {
                name: "opts",
                lua_type: "LeviathanKeymapOpts",
                required: false,
                doc: "Keymap options (description, args).",
            },
        ],
        returns: &[],
        capabilities: &[],
        validation: ApiValidation {
            args: &[
                "context must be a non-empty string",
                "key must parse as a vim-style key sequence",
                "command must be a string (resolved at dispatch time)",
            ],
            returns: &[],
            notes: &[
                "Plugin keymaps are owned by the calling generation and dropped on reload/unload.",
                "Conflicts are resolved deterministically: built-in > user > plugin (lex by plugin id, then registration order).",
            ],
        },
    },
    ApiFunction {
        path: "leviathan.keymap.del",
        name: "del",
        since: "1.9",
        compatibility: "v1",
        doc: "Remove a previously-registered keymap owned by the calling plugin generation.",
        params: &[
            ApiParam {
                name: "context",
                lua_type: "string",
                required: true,
                doc: "Activation context the keymap was bound under.",
            },
            ApiParam {
                name: "key",
                lua_type: "string",
                required: true,
                doc: "Vim-style key sequence.",
            },
        ],
        returns: &[],
        capabilities: &[],
        validation: ApiValidation {
            args: &["context must be a string", "key must be a string"],
            returns: &[],
            notes: &["Plugins can only delete their own bindings; user / built-in rows are unaffected."],
        },
    },
    ApiFunction {
        path: "leviathan.keymap.list",
        name: "list",
        since: "1.9",
        compatibility: "v1",
        doc: "List every registered keymap (built-in, user, plugin) including conflict losers.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "LeviathanKeymapSummary[]",
            doc: "Array of keymap summaries with conflict status.",
        }],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
];

const AUTOCMD_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.autocmd.group",
        name: "group",
        since: "1.7",
        compatibility: "v1",
        doc: "Create or fetch an autocmd group handle. Pass `{ clear = true }` to remove any prior autocmds in the group before returning the handle.",
        params: &[
            ApiParam {
                name: "name",
                lua_type: "string",
                required: true,
                doc: "Group name; the same string maps to the same handle for the lifetime of the plugin generation.",
            },
            ApiParam {
                name: "opts",
                lua_type: "LeviathanAutocmdGroupOpts",
                required: false,
                doc: "Group creation options.",
            },
        ],
        returns: &[ApiReturn {
            lua_type: "integer",
            doc: "Stable group handle to pass into `leviathan.autocmd.create`.",
        }],
        capabilities: &[],
        validation: ApiValidation {
            args: &[
                "name must be a string",
                "opts.clear, when present, must be a boolean",
            ],
            returns: &["integer"],
            notes: &[
                "Groups belong to the calling plugin's generation; reload drops them.",
            ],
        },
    },
    ApiFunction {
        path: "leviathan.autocmd.create",
        name: "create",
        since: "1.7",
        compatibility: "v1",
        doc: "Register an autocmd for a single typed event.",
        params: &[
            ApiParam {
                name: "event",
                lua_type: "string|string[]",
                required: true,
                doc: "Canonical event name, alias, or array of names.",
            },
            ApiParam {
                name: "opts",
                lua_type: "LeviathanAutocmdOpts",
                required: true,
                doc: "Autocmd options including the callback.",
            },
        ],
        returns: &[],
        capabilities: &[],
        validation: ApiValidation {
            args: &[
                "event must name a registered host event",
                "opts.callback must be a function",
                "opts.pattern, when present, must be a non-empty string",
                "opts.debounce_ms, when present, must be a non-negative integer",
                "opts.priority, when present, must be an integer",
            ],
            returns: &[],
            notes: &[
                "Callbacks are owned by the plugin generation and dropped on reload or unload.",
                "Failing callbacks are disabled after 5 consecutive failures (devtools-visible).",
            ],
        },
    },
    ApiFunction {
        path: "leviathan.autocmd.clear",
        name: "clear",
        since: "1.7",
        compatibility: "v1",
        doc: "Remove every autocmd registered against a group handle.",
        params: &[ApiParam {
            name: "group",
            lua_type: "integer",
            required: true,
            doc: "Group handle returned by `leviathan.autocmd.group`.",
        }],
        returns: &[],
        capabilities: &[],
        validation: ApiValidation {
            args: &["group must be an integer returned by leviathan.autocmd.group"],
            returns: &[],
            notes: &["The group handle itself remains valid after clear."],
        },
    },
];

pub const API_MODULES: &[ApiModule] = &[
    ApiModule {
        name: "root",
        table: "leviathan",
        version: V1,
        doc: "Root host API table.",
        functions: ROOT_FUNCTIONS,
        events: &[],
        types: &["leviathan"],
        capabilities: &[],
    },
    ApiModule {
        name: "api",
        table: "leviathan.api",
        version: V1,
        doc: "Descriptor, event, scheduling, and command registration APIs.",
        functions: API_FUNCTIONS,
        events: &[
            "BranchChanged",
            "FetchStart",
            "FetchEnd",
            "TabAdded",
            "TabRemoved",
            "TabReordered",
            "TabSwitched",
        ],
        types: &["leviathan.api", "LeviathanAutocmdOpts", "LeviathanAutocmdEvent"],
        capabilities: &[],
    },
    ApiModule {
        name: "command",
        table: "leviathan.command",
        version: V1,
        doc: "Phase 8 typed command registry and palette dispatch.",
        functions: COMMAND_FUNCTIONS,
        events: &["CommandExecuted"],
        types: &[
            "leviathan.command",
            "LeviathanCommandSpec",
            "LeviathanCommandArg",
            "LeviathanCommandSummary",
        ],
        capabilities: &[],
    },
    ApiModule {
        name: "keymap",
        table: "leviathan.keymap",
        version: V1,
        doc: "Phase 9 context-aware keymap registry.",
        functions: KEYMAP_FUNCTIONS,
        events: &["KeymapTriggered"],
        types: &[
            "leviathan.keymap",
            "LeviathanKeymapOpts",
            "LeviathanKeymapSummary",
            "LeviathanKeymapConflictRef",
        ],
        capabilities: &[],
    },
    ApiModule {
        name: "autocmd",
        table: "leviathan.autocmd",
        version: V1,
        doc: "Phase 7 autocmd group, typed-event registration namespace.",
        functions: AUTOCMD_FUNCTIONS,
        events: &[
            "AppStarted",
            "AppWillQuit",
            "RepositoryOpened",
            "RepositoryClosed",
            "RepositoryChanged",
            "RefsChanged",
            "HeadChanged",
            "BranchChanged",
            "CommitSelected",
            "CommitListChanged",
            "DiffLoaded",
            "WorktreeChanged",
            "FetchStarted",
            "FetchFinished",
            "PushStarted",
            "PushFinished",
            "TabAdded",
            "TabRemoved",
            "TabSelected",
            "TabMoved",
            "ThemeChanged",
            "SettingsChanged",
            "CommandExecuted",
            "KeymapTriggered",
        ],
        types: &[
            "leviathan.autocmd",
            "LeviathanAutocmdOpts",
            "LeviathanAutocmdGroupOpts",
            "LeviathanAutocmdEvent",
        ],
        capabilities: &[],
    },
    ApiModule {
        name: "ui",
        table: "leviathan.ui",
        version: V1,
        doc: "UI region and plugin screen APIs.",
        functions: UI_FUNCTIONS,
        events: &[],
        types: &[
            "leviathan.ui",
            "LeviathanScreenSpec",
            "LeviathanSlotSpec",
            "LeviathanSlotTarget",
        ],
        capabilities: &[],
    },
    ApiModule {
        name: "ui.main_bar",
        table: "leviathan.ui.main_bar",
        version: V1,
        doc: "Main bar region handle.",
        functions: MAIN_BAR_FUNCTIONS,
        events: &[],
        types: &["leviathan.ui.main_bar"],
        capabilities: &[],
    },
    ApiModule {
        name: "ui.tab_bar",
        table: "leviathan.ui.tab_bar",
        version: V1,
        doc: "Tab bar region handle.",
        functions: TAB_BAR_FUNCTIONS,
        events: &[],
        types: &["leviathan.ui.tab_bar"],
        capabilities: &[],
    },
    ApiModule {
        name: "ui.repository",
        table: "leviathan.ui.repository",
        version: V1,
        doc: "Repository content region handle.",
        functions: REPOSITORY_REGION_FUNCTIONS,
        events: &[],
        types: &["leviathan.ui.repository"],
        capabilities: &[],
    },
    // Phase 17 region modules.
    ApiModule {
        name: "ui.status_bar",
        table: "leviathan.ui.status_bar",
        version: V1,
        doc: "Status bar chrome region handle (left / center / right).",
        functions: STATUS_BAR_FUNCTIONS,
        events: &[],
        types: &["leviathan.ui.status_bar"],
        capabilities: &[],
    },
    ApiModule {
        name: "ui.repository.graph",
        table: "leviathan.ui.repository.graph",
        version: V1,
        doc: "Repository graph region handle (rows, decorations, context menu).",
        functions: REPOSITORY_GRAPH_FUNCTIONS,
        events: &[],
        types: &["leviathan.ui.repository.graph"],
        capabilities: &[],
    },
    ApiModule {
        name: "ui.repository.details",
        table: "leviathan.ui.repository.details",
        version: V1,
        doc: "Repository details region handle (commit header, files).",
        functions: REPOSITORY_DETAILS_FUNCTIONS,
        events: &[],
        types: &["leviathan.ui.repository.details"],
        capabilities: &[],
    },
    ApiModule {
        name: "ui.repository.diff",
        table: "leviathan.ui.repository.diff",
        version: V1,
        doc: "Repository diff region handle (toolbar, hunks, lines, context menu).",
        functions: REPOSITORY_DIFF_FUNCTIONS,
        events: &[],
        types: &["leviathan.ui.repository.diff"],
        capabilities: &[],
    },
    ApiModule {
        name: "fs",
        table: "leviathan.fs",
        version: V1,
        doc: "Filesystem and path helper APIs.",
        functions: FS_FUNCTIONS,
        events: &[],
        types: &["leviathan.fs", "LeviathanFsEntry"],
        capabilities: &["fs:read", "fs:write:*"],
    },
    ApiModule {
        name: "env",
        table: "leviathan.env",
        version: V1,
        doc: "Process environment access.",
        functions: ENV_FUNCTIONS,
        events: &[],
        types: &["leviathan.env"],
        capabilities: &["env"],
    },
    ApiModule {
        name: "repository",
        table: "leviathan.repository",
        version: V1,
        doc: "Active repository snapshot plus typed read APIs (Phase 11).",
        functions: REPOSITORY_FUNCTIONS,
        events: &["BranchChanged", "RefsChanged", "HeadChanged", "CommitListChanged"],
        types: &[
            "leviathan.repository",
            "LeviathanLocalBranch",
            "LeviathanRemoteBranch",
            "LeviathanTag",
        ],
        capabilities: &[
            "git:read:status",
            "git:read:log",
            "git:read:diff",
            "git:read:show",
            "git:read:blame",
        ],
    },
    ApiModule {
        name: "git",
        table: "leviathan.git",
        version: V1,
        doc: "Phase 11 typed Git write APIs. All routes capability-checked, audited, and (when destructive) gated by the host's confirmation policy.",
        functions: GIT_FUNCTIONS,
        events: &[
            "HeadChanged",
            "BranchChanged",
            "RefsChanged",
            "FetchStarted",
            "FetchFinished",
            "PushStarted",
            "PushFinished",
        ],
        types: &["leviathan.git"],
        capabilities: &[
            "git:write:checkout",
            "git:write:branch",
            "git:write:tag",
            "git:write:commit",
            "git:write:stash",
            "git:write:reset",
            "git:write:fetch",
            "git:write:push",
            "git:write:merge",
            "git:write:rebase",
        ],
    },
    ApiModule {
        name: "tab_registry",
        table: "leviathan.tab_registry",
        version: V1,
        doc: "Read-only tab snapshot plus queued tab mutation APIs.",
        functions: TAB_REGISTRY_FUNCTIONS,
        events: &["TabAdded", "TabRemoved", "TabReordered", "TabSwitched"],
        types: &["leviathan.tab_registry", "LeviathanTab"],
        capabilities: &[],
    },
    ApiModule {
        name: "services",
        table: "leviathan.services",
        version: V1,
        doc: "Inter-plugin service registry APIs.",
        functions: SERVICES_FUNCTIONS,
        events: &[],
        types: &["leviathan.services"],
        capabilities: &[],
    },
    ApiModule {
        name: "persist",
        table: "leviathan.persist",
        version: V1,
        doc: "Versioned per-plugin persistence APIs.",
        functions: PERSIST_FUNCTIONS,
        events: &[],
        types: &[
            "leviathan.persist",
            "LeviathanPersistStore",
            "LeviathanPersistOpenOpts",
        ],
        capabilities: &[],
    },
    ApiModule {
        name: "settings",
        table: "leviathan.settings",
        version: V1,
        doc: "Schema-backed plugin settings APIs.",
        functions: SETTINGS_FUNCTIONS,
        events: &["SettingsChanged"],
        types: &["leviathan.settings", "LeviathanSettingsSchema"],
        capabilities: &[],
    },
    ApiModule {
        name: "secrets",
        table: "leviathan.secrets",
        version: V1,
        doc: "Plugin-local secret APIs. Devtools exposes metadata only.",
        functions: SECRETS_FUNCTIONS,
        events: &[],
        types: &["leviathan.secrets"],
        capabilities: &[],
    },
    ApiModule {
        name: "health",
        table: "leviathan.health",
        version: V1,
        doc: "Plugin health-check APIs.",
        functions: HEALTH_FUNCTIONS,
        events: &[],
        types: &["leviathan.health", "LeviathanHealthContext"],
        capabilities: &[],
    },
    ApiModule {
        name: "runtime",
        table: "leviathan.runtime",
        version: V1,
        doc: "Runtimepath, module-loader, and after-directory introspection.",
        functions: RUNTIME_FUNCTIONS,
        events: &[],
        types: &[
            "leviathan.runtime",
            "LeviathanRuntimePathEntry",
            "LeviathanRuntimeMatch",
            "LeviathanRuntimeModuleGraphEntry",
        ],
        capabilities: &[],
    },
    ApiModule {
        name: "async",
        table: "leviathan.async",
        version: V1,
        doc: "Phase 12 host-managed background workers.",
        functions: ASYNC_FUNCTIONS,
        events: &[],
        types: &[
            "leviathan.async",
            "LeviathanJobHandle",
            "LeviathanJobContext",
        ],
        capabilities: &["async:spawn"],
    },
    ApiModule {
        name: "timer",
        table: "leviathan.timer",
        version: V1,
        doc: "Phase 12 one-shot and repeating timers.",
        functions: TIMER_FUNCTIONS,
        events: &[],
        types: &["leviathan.timer", "LeviathanTimerHandle"],
        capabilities: &["timer:create"],
    },
];

/// Empty payload (callback sees `{}` plus the `event` field the host
/// always injects). Used for lifecycle events that carry no extra
/// data of their own.
const PAYLOAD_EMPTY: &[ApiTypeField] = &[];

const PAYLOAD_REPOSITORY_REF: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Repository display name.",
    },
    ApiTypeField {
        name: "path",
        lua_type: "string",
        required: true,
        doc: "Repository workdir path.",
    },
];

const PAYLOAD_BRANCH: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Branch name.",
    },
    ApiTypeField {
        name: "head_hash",
        lua_type: "string",
        required: true,
        doc: "Resolved HEAD commit hash.",
    },
];

const PAYLOAD_REFS: &[ApiTypeField] = &[ApiTypeField {
    name: "count",
    lua_type: "integer",
    required: true,
    doc: "Number of refs after the change.",
}];

const PAYLOAD_HEAD: &[ApiTypeField] = &[ApiTypeField {
    name: "hash",
    lua_type: "string",
    required: true,
    doc: "New HEAD commit hash.",
}];

const PAYLOAD_COMMIT: &[ApiTypeField] = &[ApiTypeField {
    name: "hash",
    lua_type: "string",
    required: true,
    doc: "Selected commit hash.",
}];

const PAYLOAD_COMMIT_LIST: &[ApiTypeField] = &[ApiTypeField {
    name: "count",
    lua_type: "integer",
    required: true,
    doc: "Number of commits in the new list.",
}];

const PAYLOAD_DIFF: &[ApiTypeField] = &[ApiTypeField {
    name: "hash",
    lua_type: "string",
    required: true,
    doc: "Hash of the diff target.",
}];

const PAYLOAD_WORKTREE: &[ApiTypeField] = &[ApiTypeField {
    name: "path",
    lua_type: "string",
    required: true,
    doc: "Worktree path that changed.",
}];

const PAYLOAD_FETCH: &[ApiTypeField] = &[ApiTypeField {
    name: "remote",
    lua_type: "string",
    required: true,
    doc: "Remote name involved in the fetch.",
}];

const PAYLOAD_PUSH: &[ApiTypeField] = &[ApiTypeField {
    name: "remote",
    lua_type: "string",
    required: true,
    doc: "Remote name involved in the push.",
}];

const PAYLOAD_TAB: &[ApiTypeField] = &[
    ApiTypeField {
        name: "tab_id",
        lua_type: "integer",
        required: true,
        doc: "Tab id.",
    },
    ApiTypeField {
        name: "path",
        lua_type: "string",
        required: false,
        doc: "Repository path the tab is bound to, when known.",
    },
];

const PAYLOAD_TAB_MOVED: &[ApiTypeField] = &[ApiTypeField {
    name: "count",
    lua_type: "integer",
    required: true,
    doc: "Number of open tabs after the move.",
}];

const PAYLOAD_THEME: &[ApiTypeField] = &[ApiTypeField {
    name: "name",
    lua_type: "string",
    required: true,
    doc: "Active theme identifier.",
}];

const PAYLOAD_SETTINGS: &[ApiTypeField] = &[ApiTypeField {
    name: "key",
    lua_type: "string",
    required: true,
    doc: "Dotted setting key that changed.",
}];

const PAYLOAD_COMMAND: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Executed command name.",
    },
    ApiTypeField {
        name: "plugin_id",
        lua_type: "string",
        required: false,
        doc: "Owning plugin id when known.",
    },
];

const PAYLOAD_KEYMAP: &[ApiTypeField] = &[
    ApiTypeField {
        name: "context",
        lua_type: "string",
        required: true,
        doc: "Active context the chord matched in.",
    },
    ApiTypeField {
        name: "key",
        lua_type: "string",
        required: true,
        doc: "Rendered chord (vim-style notation).",
    },
    ApiTypeField {
        name: "command",
        lua_type: "string",
        required: true,
        doc: "Command the chord dispatched to.",
    },
    ApiTypeField {
        name: "plugin_id",
        lua_type: "string",
        required: true,
        doc: "Owning plugin id of the matched keymap; `<host>` for built-ins, `<user>` for user-config.",
    },
    ApiTypeField {
        name: "ok",
        lua_type: "boolean",
        required: true,
        doc: "True when the underlying command dispatch returned `Ok`.",
    },
];

pub const API_EVENTS: &[ApiEvent] = &[
    // ---- Lifecycle ----
    ApiEvent {
        name: "AppStarted",
        since: "1.7",
        doc: "Fired once after the host app finishes starting up.",
        payload_type: "table",
        payload_fields: PAYLOAD_EMPTY,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "AppWillQuit",
        since: "1.7",
        doc: "Fired immediately before the host app shuts down.",
        payload_type: "table",
        payload_fields: PAYLOAD_EMPTY,
        aliases: &[],
        is_alias: false,
    },
    // ---- Repository ----
    ApiEvent {
        name: "RepositoryOpened",
        since: "1.7",
        doc: "Fired after a repository is opened.",
        payload_type: "table",
        payload_fields: PAYLOAD_REPOSITORY_REF,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "RepositoryClosed",
        since: "1.7",
        doc: "Fired after a repository is closed.",
        payload_type: "table",
        payload_fields: PAYLOAD_REPOSITORY_REF,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "RepositoryChanged",
        since: "1.7",
        doc: "Fired after the active repository changes.",
        payload_type: "table",
        payload_fields: PAYLOAD_REPOSITORY_REF,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "RefsChanged",
        since: "1.7",
        doc: "Fired after the repository ref set changes (push/fetch/branch ops).",
        payload_type: "table",
        payload_fields: PAYLOAD_REFS,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "HeadChanged",
        since: "1.7",
        doc: "Fired after the resolved HEAD commit hash changes.",
        payload_type: "table",
        payload_fields: PAYLOAD_HEAD,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "BranchChanged",
        since: "1.0",
        doc: "Fired after the current branch or its head hash changes.",
        payload_type: "table",
        payload_fields: PAYLOAD_BRANCH,
        aliases: &[],
        is_alias: false,
    },
    // ---- Commits and diffs ----
    ApiEvent {
        name: "CommitSelected",
        since: "1.7",
        doc: "Fired after the user selects a commit in the history view.",
        payload_type: "table",
        payload_fields: PAYLOAD_COMMIT,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "CommitListChanged",
        since: "1.7",
        doc: "Fired after the visible commit list changes.",
        payload_type: "table",
        payload_fields: PAYLOAD_COMMIT_LIST,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "DiffLoaded",
        since: "1.7",
        doc: "Fired after a diff finishes loading.",
        payload_type: "table",
        payload_fields: PAYLOAD_DIFF,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "WorktreeChanged",
        since: "1.7",
        doc: "Fired when the worktree contents change.",
        payload_type: "table",
        payload_fields: PAYLOAD_WORKTREE,
        aliases: &[],
        is_alias: false,
    },
    // ---- Network ops ----
    ApiEvent {
        name: "FetchStarted",
        since: "1.7",
        doc: "Fired when a repository fetch starts.",
        payload_type: "table",
        payload_fields: PAYLOAD_FETCH,
        aliases: &["FetchStart"],
        is_alias: false,
    },
    ApiEvent {
        name: "FetchFinished",
        since: "1.7",
        doc: "Fired when a repository fetch finishes (success or failure).",
        payload_type: "table",
        payload_fields: PAYLOAD_FETCH,
        aliases: &["FetchEnd"],
        is_alias: false,
    },
    ApiEvent {
        name: "PushStarted",
        since: "1.7",
        doc: "Fired when a push starts.",
        payload_type: "table",
        payload_fields: PAYLOAD_PUSH,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "PushFinished",
        since: "1.7",
        doc: "Fired when a push finishes (success or failure).",
        payload_type: "table",
        payload_fields: PAYLOAD_PUSH,
        aliases: &[],
        is_alias: false,
    },
    // ---- Tabs ----
    ApiEvent {
        name: "TabAdded",
        since: "1.0",
        doc: "Fired after a tab is added.",
        payload_type: "table",
        payload_fields: PAYLOAD_TAB,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "TabRemoved",
        since: "1.0",
        doc: "Fired after a tab is removed.",
        payload_type: "table",
        payload_fields: PAYLOAD_TAB,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "TabSelected",
        since: "1.7",
        doc: "Fired after the active tab changes.",
        payload_type: "table",
        payload_fields: PAYLOAD_TAB,
        aliases: &["TabSwitched"],
        is_alias: false,
    },
    ApiEvent {
        name: "TabMoved",
        since: "1.7",
        doc: "Fired after tabs are reordered.",
        payload_type: "table",
        payload_fields: PAYLOAD_TAB_MOVED,
        aliases: &["TabReordered"],
        is_alias: false,
    },
    // ---- App state ----
    ApiEvent {
        name: "ThemeChanged",
        since: "1.7",
        doc: "Fired after the active theme changes.",
        payload_type: "table",
        payload_fields: PAYLOAD_THEME,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "SettingsChanged",
        since: "1.7",
        doc: "Fired after a settings entry is updated.",
        payload_type: "table",
        payload_fields: PAYLOAD_SETTINGS,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "CommandExecuted",
        since: "1.7",
        doc: "Fired after a host or plugin command finishes executing.",
        payload_type: "table",
        payload_fields: PAYLOAD_COMMAND,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "KeymapTriggered",
        since: "1.9",
        doc: "Fired after a keymap chord matches and the host dispatches the underlying command.",
        payload_type: "table",
        payload_fields: PAYLOAD_KEYMAP,
        aliases: &[],
        is_alias: false,
    },
    // ---- v1 alias entries ----
    // These are the legacy v1 names retained for plugins that still
    // subscribe through the original `leviathan.api.create_autocmd`
    // shim. The runtime fires them as side-effects of their canonical
    // event so old listeners keep working unchanged.
    ApiEvent {
        name: "FetchStart",
        since: "1.0",
        doc: "Compatibility alias of `FetchStarted`.",
        payload_type: "table",
        payload_fields: PAYLOAD_FETCH,
        aliases: &[],
        is_alias: true,
    },
    ApiEvent {
        name: "FetchEnd",
        since: "1.0",
        doc: "Compatibility alias of `FetchFinished`.",
        payload_type: "table",
        payload_fields: PAYLOAD_FETCH,
        aliases: &[],
        is_alias: true,
    },
    ApiEvent {
        name: "TabReordered",
        since: "1.0",
        doc: "Compatibility alias of `TabMoved`.",
        payload_type: "table",
        payload_fields: PAYLOAD_TAB_MOVED,
        aliases: &[],
        is_alias: true,
    },
    ApiEvent {
        name: "TabSwitched",
        since: "1.0",
        doc: "Compatibility alias of `TabSelected`.",
        payload_type: "table",
        payload_fields: PAYLOAD_TAB,
        aliases: &[],
        is_alias: true,
    },
];

/// Look up an event descriptor by name. Includes alias entries.
pub fn event_descriptor(name: &str) -> Option<&'static ApiEvent> {
    API_EVENTS.iter().find(|event| event.name == name)
}

/// Iterator over canonical (non-alias) events only.
pub fn canonical_events() -> impl Iterator<Item = &'static ApiEvent> {
    API_EVENTS.iter().filter(|event| !event.is_alias)
}

pub const API_CAPABILITIES: &[ApiCapability] = &[
    ApiCapability {
        name: "fs:read",
        since: "1.0",
        doc: "Compatibility alias for fs:read:plugin.",
    },
    ApiCapability {
        name: "fs:read:plugin",
        since: "1.0",
        doc: "Read paths under the plugin directory.",
    },
    ApiCapability {
        name: "fs:read:state",
        since: "1.0",
        doc: "Read paths under the plugin state directory.",
    },
    ApiCapability {
        name: "fs:read:config",
        since: "1.0",
        doc: "Read paths under the plugin config directory.",
    },
    ApiCapability {
        name: "fs:read:workdir",
        since: "1.0",
        doc: "Read paths under the active workdir when configured.",
    },
    ApiCapability {
        name: "fs:read:any",
        since: "1.0",
        doc: "Read any host path.",
    },
    ApiCapability {
        name: "fs:read:scope:<dir>",
        since: "1.0",
        doc: "Read paths under an explicit user-chosen directory (canonicalised; symlinks escaping the scope are denied).",
    },
    ApiCapability {
        name: "fs:write:plugin",
        since: "1.0",
        doc: "Write paths under the plugin directory.",
    },
    ApiCapability {
        name: "fs:write:state",
        since: "1.0",
        doc: "Write paths under the plugin state directory.",
    },
    ApiCapability {
        name: "fs:write:config",
        since: "1.0",
        doc: "Write paths under the plugin config directory.",
    },
    ApiCapability {
        name: "fs:write:workdir",
        since: "1.0",
        doc: "Write paths under the active workdir when configured.",
    },
    ApiCapability {
        name: "fs:write:any",
        since: "1.0",
        doc: "Write any host path.",
    },
    ApiCapability {
        name: "fs:write:scope:<dir>",
        since: "1.0",
        doc: "Write paths under an explicit user-chosen directory (canonicalised; symlinks escaping the scope are denied).",
    },
    ApiCapability {
        name: "process:spawn",
        since: "1.0",
        doc: "Reserved process-spawn capability (any binary).",
    },
    ApiCapability {
        name: "process:spawn:<binary>",
        since: "1.0",
        doc: "Spawn a specific binary by basename (e.g. `process:spawn:git`). Phase 12 will enforce.",
    },
    ApiCapability {
        name: "net:fetch",
        since: "1.0",
        doc: "Reserved network-fetch capability (any host).",
    },
    ApiCapability {
        name: "net:fetch:<domain>",
        since: "1.0",
        doc: "Fetch from a specific domain (e.g. `net:fetch:github.com`). Phase 12 will enforce.",
    },
    ApiCapability {
        name: "clipboard",
        since: "1.0",
        doc: "Compatibility alias for clipboard read+write.",
    },
    ApiCapability {
        name: "clipboard:read",
        since: "1.0",
        doc: "Read the system clipboard.",
    },
    ApiCapability {
        name: "clipboard:write",
        since: "1.0",
        doc: "Write to the system clipboard.",
    },
    ApiCapability {
        name: "notify",
        since: "1.0",
        doc: "Surface a host notification banner.",
    },
    ApiCapability {
        name: "env",
        since: "1.0",
        doc: "Read every process environment variable.",
    },
    ApiCapability {
        name: "env:<glob>",
        since: "1.0",
        doc: "Read environment variables whose name matches the glob (e.g. `env:GIT_*`).",
    },
    ApiCapability {
        name: "credentials",
        since: "1.0",
        doc: "Read host-stored credentials (Phase 13 secrets).",
    },
    ApiCapability {
        name: "repo:read",
        since: "1.0",
        doc: "Observe the active repository projection (refs, head, status). Implicit for repository slot widgets in v1.",
    },
    ApiCapability {
        name: "git:read:status",
        since: "1.0",
        doc: "Read working tree status. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:read:log",
        since: "1.0",
        doc: "Read commit history. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:read:diff",
        since: "1.0",
        doc: "Read diffs between commits or against the index. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:read:show",
        since: "1.0",
        doc: "Read a commit's tree or a file at a commit. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:read:blame",
        since: "1.0",
        doc: "Read line-level blame for a tracked file. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:write:checkout",
        since: "1.0",
        doc: "Move HEAD to a ref or commit. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:write:branch",
        since: "1.0",
        doc: "Create / delete / rename branches. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:write:tag",
        since: "1.0",
        doc: "Create / delete tags. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:write:commit",
        since: "1.0",
        doc: "Create commits. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:write:stash",
        since: "1.0",
        doc: "Push / pop / drop stashes. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:write:reset",
        since: "1.0",
        doc: "Reset the index or working tree. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:write:fetch",
        since: "1.0",
        doc: "Fetch from a remote. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:write:push",
        since: "1.0",
        doc: "Push to a remote. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:write:merge",
        since: "1.0",
        doc: "Merge refs into HEAD. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "git:write:rebase",
        since: "1.0",
        doc: "Rebase HEAD. Phase 11 wires the typed API.",
    },
    ApiCapability {
        name: "ui:region:<region>",
        since: "1.0",
        doc: "Restrict slot registrations to a specific region (e.g. `ui:region:repository.sidebar`). Backwards-compat: omitting it leaves the legacy unrestricted v1 access in place.",
    },
    ApiCapability {
        name: "services:provide:<service@version>",
        since: "1.0",
        doc: "Provide a versioned service to other plugins (Phase 14).",
    },
    ApiCapability {
        name: "services:consume:<service@version>",
        since: "1.0",
        doc: "Consume a versioned service from another plugin (Phase 14).",
    },
    ApiCapability {
        name: "async:spawn",
        since: "1.12",
        doc: "Spawn a host-managed background worker thread (Phase 12).",
    },
    ApiCapability {
        name: "timer:create",
        since: "1.12",
        doc: "Schedule one-shot or repeating timers (Phase 12).",
    },
    ApiCapability {
        name: "fs:watch",
        since: "1.12",
        doc: "Watch paths for filesystem events (Phase 12, plugin scope).",
    },
    ApiCapability {
        name: "fs:watch:scope:<dir>",
        since: "1.12",
        doc: "Watch paths under an explicit user-chosen directory (Phase 12).",
    },
    ApiCapability {
        name: "ui:overlay",
        since: "1.0",
        doc: "Register modal overlays that the host renders above the active screen (Phase 17).",
    },
    ApiCapability {
        name: "ui:context_menu",
        since: "1.0",
        doc: "Contribute items to host-rendered context menus at extension points (Phase 17).",
    },
    ApiCapability {
        name: "ui:graph_decoration",
        since: "1.0",
        doc: "Attach badges / icons / markers / lanes to commit rows in the graph (Phase 17).",
    },
    ApiCapability {
        name: "ui:diff_decoration",
        since: "1.0",
        doc: "Attach line hints / hunk badges / line gutters to the diff view (Phase 17).",
    },
];

const TYPE_LEVIATHAN_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "api",
        lua_type: "leviathan.api",
        required: true,
        doc: "API utility namespace.",
    },
    ApiTypeField {
        name: "ui",
        lua_type: "leviathan.ui",
        required: true,
        doc: "UI namespace.",
    },
    ApiTypeField {
        name: "fs",
        lua_type: "leviathan.fs",
        required: true,
        doc: "Filesystem namespace.",
    },
    ApiTypeField {
        name: "env",
        lua_type: "leviathan.env",
        required: true,
        doc: "Environment namespace.",
    },
    ApiTypeField {
        name: "repository",
        lua_type: "leviathan.repository",
        required: true,
        doc: "Active repository snapshot.",
    },
    ApiTypeField {
        name: "tab_registry",
        lua_type: "leviathan.tab_registry",
        required: true,
        doc: "Open tab snapshot.",
    },
    ApiTypeField {
        name: "services",
        lua_type: "leviathan.services",
        required: true,
        doc: "Service registry namespace.",
    },
    ApiTypeField {
        name: "persist",
        lua_type: "leviathan.persist",
        required: true,
        doc: "Persistence namespace.",
    },
    ApiTypeField {
        name: "settings",
        lua_type: "leviathan.settings",
        required: true,
        doc: "Settings namespace.",
    },
    ApiTypeField {
        name: "secrets",
        lua_type: "leviathan.secrets",
        required: true,
        doc: "Secrets namespace.",
    },
    ApiTypeField {
        name: "health",
        lua_type: "leviathan.health",
        required: true,
        doc: "Health namespace.",
    },
];

const TYPE_TAB_REGISTRY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "list",
        lua_type: "LeviathanTab[]",
        required: true,
        doc: "Open tabs.",
    },
    ApiTypeField {
        name: "current",
        lua_type: "LeviathanTab|nil",
        required: true,
        doc: "Currently focused tab.",
    },
];

const TYPE_REPOSITORY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Repository display name.",
    },
    ApiTypeField {
        name: "workdir_path",
        lua_type: "string",
        required: true,
        doc: "Workdir path, empty when unavailable.",
    },
    ApiTypeField {
        name: "current_branch_name",
        lua_type: "string",
        required: true,
        doc: "Current branch display name.",
    },
    ApiTypeField {
        name: "current_branch",
        lua_type: "LeviathanLocalBranch|nil",
        required: true,
        doc: "Current local branch table.",
    },
    ApiTypeField {
        name: "is_open",
        lua_type: "boolean",
        required: true,
        doc: "Whether a repository is loaded.",
    },
    ApiTypeField {
        name: "is_detached",
        lua_type: "boolean",
        required: true,
        doc: "Whether HEAD is detached.",
    },
    ApiTypeField {
        name: "is_unborn",
        lua_type: "boolean",
        required: true,
        doc: "Whether HEAD points at an unborn branch.",
    },
    ApiTypeField {
        name: "is_bare",
        lua_type: "boolean",
        required: true,
        doc: "Whether the repository has no worktree.",
    },
    ApiTypeField {
        name: "head_hash",
        lua_type: "string",
        required: true,
        doc: "Current HEAD hash or empty string.",
    },
    ApiTypeField {
        name: "default_remote_name",
        lua_type: "string",
        required: true,
        doc: "Default remote name or empty string.",
    },
    ApiTypeField {
        name: "local_branches",
        lua_type: "LeviathanLocalBranch[]",
        required: true,
        doc: "Local branches.",
    },
    ApiTypeField {
        name: "remote_branches",
        lua_type: "LeviathanRemoteBranch[]",
        required: true,
        doc: "Remote branches.",
    },
    ApiTypeField {
        name: "tags",
        lua_type: "LeviathanTag[]",
        required: true,
        doc: "Tags.",
    },
];

const TYPE_LOCAL_BRANCH_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Branch name.",
    },
    ApiTypeField {
        name: "hash",
        lua_type: "string",
        required: true,
        doc: "Target hash.",
    },
    ApiTypeField {
        name: "is_current",
        lua_type: "boolean",
        required: true,
        doc: "Whether this branch is current.",
    },
    ApiTypeField {
        name: "upstream_branch",
        lua_type: "LeviathanRemoteBranch|nil",
        required: true,
        doc: "Upstream remote branch.",
    },
];

const TYPE_REMOTE_BRANCH_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Branch name.",
    },
    ApiTypeField {
        name: "remote_name",
        lua_type: "string",
        required: true,
        doc: "Remote name.",
    },
    ApiTypeField {
        name: "hash",
        lua_type: "string",
        required: true,
        doc: "Target hash.",
    },
];

const TYPE_TAG_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Tag name.",
    },
    ApiTypeField {
        name: "hash",
        lua_type: "string",
        required: true,
        doc: "Target hash.",
    },
];

const TYPE_TAB_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "path",
        lua_type: "string",
        required: true,
        doc: "Tab repository path.",
    },
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Tab display name.",
    },
];

const TYPE_FS_ENTRY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Entry name.",
    },
    ApiTypeField {
        name: "path",
        lua_type: "string",
        required: true,
        doc: "Full path.",
    },
    ApiTypeField {
        name: "is_dir",
        lua_type: "boolean",
        required: true,
        doc: "Whether the entry is a directory.",
    },
    ApiTypeField {
        name: "is_symlink",
        lua_type: "boolean",
        required: true,
        doc: "Whether the entry is a symlink.",
    },
    ApiTypeField {
        name: "size",
        lua_type: "integer",
        required: true,
        doc: "Entry size.",
    },
    ApiTypeField {
        name: "modified",
        lua_type: "integer",
        required: true,
        doc: "Unix modification time.",
    },
];

const TYPE_AUTOCMD_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "callback",
        lua_type: "fun(event: LeviathanAutocmdEvent)",
        required: true,
        doc: "Event callback. Receives a typed payload table.",
    },
    ApiTypeField {
        name: "group",
        lua_type: "integer",
        required: false,
        doc: "Optional autocmd group handle returned by `leviathan.autocmd.group`.",
    },
    ApiTypeField {
        name: "once",
        lua_type: "boolean",
        required: false,
        doc: "When true, auto-removes the autocmd after its first fire.",
    },
    ApiTypeField {
        name: "pattern",
        lua_type: "string",
        required: false,
        doc: "Glob pattern matched against a payload string field.",
    },
    ApiTypeField {
        name: "debounce_ms",
        lua_type: "integer",
        required: false,
        doc: "Coalesce rapid fires within this many host-clock milliseconds.",
    },
    ApiTypeField {
        name: "priority",
        lua_type: "integer",
        required: false,
        doc: "Higher priority runs first; ties fall back to declaration order.",
    },
];

const TYPE_AUTOCMD_EVENT_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "event",
        lua_type: "string",
        required: true,
        doc: "Canonical event name.",
    },
    ApiTypeField {
        name: "alias",
        lua_type: "string",
        required: false,
        doc: "Alias the listener subscribed under, when different from `event`.",
    },
    ApiTypeField {
        name: "payload",
        lua_type: "table",
        required: true,
        doc: "Typed payload table for the event.",
    },
];

const TYPE_AUTOCMD_GROUP_OPTS_FIELDS: &[ApiTypeField] = &[ApiTypeField {
    name: "clear",
    lua_type: "boolean",
    required: false,
    doc: "When true, removes any prior autocmds in the group before returning the handle.",
}];
const TYPE_SCREEN_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "id",
        lua_type: "string",
        required: true,
        doc: "Screen id.",
    },
    ApiTypeField {
        name: "init",
        lua_type: "fun(): table",
        required: true,
        doc: "Initial state callback.",
    },
    ApiTypeField {
        name: "view",
        lua_type: "fun(state: table): LeviathanWidget",
        required: true,
        doc: "View callback.",
    },
    ApiTypeField {
        name: "update",
        lua_type: "fun(state: table, event: string, value: LeviathanJson): table",
        required: true,
        doc: "Update callback.",
    },
    ApiTypeField {
        name: "serialize",
        lua_type: "fun(state: table): LeviathanJson",
        required: false,
        doc: "Reload state serializer.",
    },
    ApiTypeField {
        name: "deserialize",
        lua_type: "fun(value: LeviathanJson): table",
        required: false,
        doc: "Reload state deserializer.",
    },
];
const TYPE_SLOT_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "region",
        lua_type: "string",
        required: false,
        doc: "Region name; inferred by direct handles.",
    },
    ApiTypeField {
        name: "pane",
        lua_type: "string",
        required: false,
        doc: "Content region pane.",
    },
    ApiTypeField {
        name: "section",
        lua_type: "string",
        required: true,
        doc: "Region section.",
    },
    ApiTypeField {
        name: "id",
        lua_type: "string",
        required: true,
        doc: "Slot id.",
    },
    ApiTypeField {
        name: "priority",
        lua_type: "integer",
        required: true,
        doc: "Ordering priority.",
    },
    ApiTypeField {
        name: "widget",
        lua_type: "LeviathanWidget|fun(): LeviathanWidget",
        required: true,
        doc: "Static or dynamic widget.",
    },
    ApiTypeField {
        name: "on_click",
        lua_type: "fun(slot_id: string, event: string, value: LeviathanJson): table|nil",
        required: false,
        doc: "Slot callback.",
    },
];
const TYPE_SLOT_TARGET_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "pane",
        lua_type: "string",
        required: false,
        doc: "Content region pane.",
    },
    ApiTypeField {
        name: "section",
        lua_type: "string",
        required: true,
        doc: "Region section.",
    },
    ApiTypeField {
        name: "id",
        lua_type: "string",
        required: true,
        doc: "Slot id.",
    },
];
const TYPE_PERSIST_OPEN_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "version",
        lua_type: "integer",
        required: false,
        doc: "Target store version.",
    },
    ApiTypeField {
        name: "migrations",
        lua_type: "table[]",
        required: false,
        doc: "Migration descriptors.",
    },
    ApiTypeField {
        name: "surface",
        lua_type: "string",
        required: false,
        doc: "Storage surface: state, config, cache, or repo.",
    },
    ApiTypeField {
        name: "repo",
        lua_type: "string",
        required: false,
        doc: "Per-repo state key when surface is repo.",
    },
];

const PERSIST_METHODS: &[ApiTypeMethod] = &[
    ApiTypeMethod {
        name: "get",
        doc: "Read a stored JSON value.",
        params: &[ApiParam {
            name: "key",
            lua_type: "string",
            required: true,
            doc: "Storage key.",
        }],
        returns: &[ApiReturn {
            lua_type: "LeviathanJson|nil",
            doc: "Stored value or nil.",
        }],
    },
    ApiTypeMethod {
        name: "set",
        doc: "Write a stored JSON value.",
        params: &[
            ApiParam {
                name: "key",
                lua_type: "string",
                required: true,
                doc: "Storage key.",
            },
            ApiParam {
                name: "value",
                lua_type: "LeviathanJson",
                required: true,
                doc: "JSON-like value.",
            },
        ],
        returns: &[],
    },
    ApiTypeMethod {
        name: "version",
        doc: "Return the current store version.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "integer",
            doc: "Store version.",
        }],
    },
    ApiTypeMethod {
        name: "delete",
        doc: "Delete a stored key.",
        params: &[ApiParam {
            name: "key",
            lua_type: "string",
            required: true,
            doc: "Storage key.",
        }],
        returns: &[],
    },
];

const TYPE_COMMAND_SPEC_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "title",
        lua_type: "string",
        required: false,
        doc: "Human-friendly palette title; defaults to the command name.",
    },
    ApiTypeField {
        name: "description",
        lua_type: "string",
        required: false,
        doc: "Long-form description shown in the palette and devtools.",
    },
    ApiTypeField {
        name: "context",
        lua_type: "string",
        required: false,
        doc: "Activation context name; defaults to `global`.",
    },
    ApiTypeField {
        name: "args",
        lua_type: "LeviathanCommandArg[]",
        required: false,
        doc: "Argument schema validated at invocation time.",
    },
    ApiTypeField {
        name: "destructive",
        lua_type: "boolean",
        required: false,
        doc: "When true, the palette filters this command behind a destructive-actions toggle.",
    },
    ApiTypeField {
        name: "capabilities",
        lua_type: "string[]",
        required: false,
        doc: "Capability names the dispatcher checks at invocation time.",
    },
    ApiTypeField {
        name: "run",
        lua_type: "fun(args: table)",
        required: true,
        doc: "Body of the command; the host runs it inside a coroutine.",
    },
];

const TYPE_COMMAND_ARG_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Arg key in the args table.",
    },
    ApiTypeField {
        name: "type",
        lua_type: "string",
        required: true,
        doc: "One of `string`, `boolean`, `integer`, `number`, or `enum:a,b,c`.",
    },
    ApiTypeField {
        name: "required",
        lua_type: "boolean",
        required: false,
        doc: "Defaults to false. Required args without a default reject the invocation.",
    },
    ApiTypeField {
        name: "default",
        lua_type: "LeviathanJson",
        required: false,
        doc: "Default value used when the caller omits the arg.",
    },
    ApiTypeField {
        name: "doc",
        lua_type: "string",
        required: false,
        doc: "Documentation surfaced in the palette and devtools.",
    },
];

const TYPE_COMMAND_SUMMARY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Command identifier.",
    },
    ApiTypeField {
        name: "title",
        lua_type: "string",
        required: true,
        doc: "Palette title.",
    },
    ApiTypeField {
        name: "description",
        lua_type: "string",
        required: true,
        doc: "Description text.",
    },
    ApiTypeField {
        name: "plugin_id",
        lua_type: "string",
        required: true,
        doc: "Owning plugin id; `<host>` for built-in commands.",
    },
    ApiTypeField {
        name: "context",
        lua_type: "string",
        required: true,
        doc: "Activation context.",
    },
    ApiTypeField {
        name: "destructive",
        lua_type: "boolean",
        required: true,
        doc: "Destructive flag.",
    },
];

const TYPE_KEYMAP_OPTS_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "description",
        lua_type: "string",
        required: false,
        doc: "Human-friendly description shown in devtools and the keymap inspector.",
    },
    ApiTypeField {
        name: "args",
        lua_type: "LeviathanJson",
        required: false,
        doc: "Args table forwarded verbatim to the underlying command at dispatch time.",
    },
];

const TYPE_KEYMAP_SUMMARY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "context",
        lua_type: "string",
        required: true,
        doc: "Activation context.",
    },
    ApiTypeField {
        name: "key",
        lua_type: "string",
        required: true,
        doc: "Rendered chord (vim-style).",
    },
    ApiTypeField {
        name: "command",
        lua_type: "string",
        required: true,
        doc: "Command the keymap dispatches to.",
    },
    ApiTypeField {
        name: "plugin_id",
        lua_type: "string",
        required: true,
        doc: "Owning plugin id; `<host>` for built-ins, `<user>` for user-config rows.",
    },
    ApiTypeField {
        name: "source",
        lua_type: "string",
        required: true,
        doc: "One of `built-in`, `user`, `plugin`.",
    },
    ApiTypeField {
        name: "status",
        lua_type: "string",
        required: true,
        doc: "One of `active`, `conflict_lost`.",
    },
    ApiTypeField {
        name: "description",
        lua_type: "string",
        required: true,
        doc: "Description text supplied at registration.",
    },
    ApiTypeField {
        name: "conflict_with",
        lua_type: "LeviathanKeymapConflictRef|nil",
        required: false,
        doc: "When `status == conflict_lost`, points to the winning binding.",
    },
];

const TYPE_KEYMAP_CONFLICT_REF_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "plugin_id",
        lua_type: "string",
        required: true,
        doc: "Winning binding's plugin id.",
    },
    ApiTypeField {
        name: "source",
        lua_type: "string",
        required: true,
        doc: "Winning binding's source tier.",
    },
];

const HEALTH_METHODS: &[ApiTypeMethod] = &[
    ApiTypeMethod {
        name: "ok",
        doc: "Record an OK health item.",
        params: &[ApiParam {
            name: "message",
            lua_type: "string",
            required: true,
            doc: "Message.",
        }],
        returns: &[],
    },
    ApiTypeMethod {
        name: "info",
        doc: "Record an informational health item.",
        params: &[ApiParam {
            name: "message",
            lua_type: "string",
            required: true,
            doc: "Message.",
        }],
        returns: &[],
    },
    ApiTypeMethod {
        name: "warn",
        doc: "Record a warning health item.",
        params: &[ApiParam {
            name: "message",
            lua_type: "string",
            required: true,
            doc: "Message.",
        }],
        returns: &[],
    },
    ApiTypeMethod {
        name: "error",
        doc: "Record an error health item.",
        params: &[ApiParam {
            name: "message",
            lua_type: "string",
            required: true,
            doc: "Message.",
        }],
        returns: &[],
    },
];

pub const API_TYPES: &[ApiType] = &[
    ApiType {
        name: "leviathan",
        since: "1.0",
        doc: "Root host API table.",
        fields: TYPE_LEVIATHAN_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "leviathan.api",
        since: "1.0",
        doc: "API utility namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.ui",
        since: "1.0",
        doc: "UI namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.ui.main_bar",
        since: "1.0",
        doc: "Main bar region handle.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.ui.tab_bar",
        since: "1.0",
        doc: "Tab bar region handle.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.ui.repository",
        since: "1.0",
        doc: "Repository content region handle.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.fs",
        since: "1.0",
        doc: "Filesystem namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.env",
        since: "1.0",
        doc: "Environment namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.tab_registry",
        since: "1.0",
        doc: "Tab registry namespace.",
        fields: TYPE_TAB_REGISTRY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "leviathan.repository",
        since: "1.0",
        doc: "Active repository snapshot.",
        fields: TYPE_REPOSITORY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "leviathan.services",
        since: "1.0",
        doc: "Service registry namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.persist",
        since: "1.0",
        doc: "Persistence namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.settings",
        since: "1.13",
        doc: "Settings namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanSettingsSchema",
        since: "1.13",
        doc: "Settings schema table.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.secrets",
        since: "1.13",
        doc: "Secrets namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.health",
        since: "1.0",
        doc: "Health namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanJson",
        since: "1.0",
        doc: "JSON-like Lua value accepted at host/plugin boundaries.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanAutocmdOpts",
        since: "1.0",
        doc: "Autocmd options.",
        fields: TYPE_AUTOCMD_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanAutocmdEvent",
        since: "1.7",
        doc: "Typed event payload table handed to autocmd callbacks.",
        fields: TYPE_AUTOCMD_EVENT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanAutocmdGroupOpts",
        since: "1.7",
        doc: "Options accepted by `leviathan.autocmd.group`.",
        fields: TYPE_AUTOCMD_GROUP_OPTS_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "leviathan.autocmd",
        since: "1.7",
        doc: "Autocmd group and typed-event namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.command",
        since: "1.8",
        doc: "Phase 8 typed command registry namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanCommandSpec",
        since: "1.8",
        doc: "Descriptor accepted by `leviathan.command.create`.",
        fields: TYPE_COMMAND_SPEC_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanCommandArg",
        since: "1.8",
        doc: "One entry in a command's `args` schema.",
        fields: TYPE_COMMAND_ARG_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanCommandSummary",
        since: "1.8",
        doc: "Compact view of a registered command, returned by `leviathan.command.list`.",
        fields: TYPE_COMMAND_SUMMARY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "leviathan.keymap",
        since: "1.9",
        doc: "Phase 9 keymap registry namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanKeymapOpts",
        since: "1.9",
        doc: "Options table accepted by `leviathan.keymap.set`.",
        fields: TYPE_KEYMAP_OPTS_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanKeymapSummary",
        since: "1.9",
        doc: "Compact view of a registered keymap, returned by `leviathan.keymap.list`.",
        fields: TYPE_KEYMAP_SUMMARY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanKeymapConflictRef",
        since: "1.9",
        doc: "Reference to the winning binding for a conflict-lost keymap row.",
        fields: TYPE_KEYMAP_CONFLICT_REF_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanScreenSpec",
        since: "1.0",
        doc: "Plugin screen descriptor.",
        fields: TYPE_SCREEN_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanSlotSpec",
        since: "1.0",
        doc: "UI slot descriptor.",
        fields: TYPE_SLOT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanSlotTarget",
        since: "1.0",
        doc: "UI slot target descriptor.",
        fields: TYPE_SLOT_TARGET_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanWidget",
        since: "1.0",
        doc: "Tagged widget tree node; see widget descriptors and widget schema.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanFsEntry",
        since: "1.0",
        doc: "Filesystem entry metadata.",
        fields: TYPE_FS_ENTRY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanTab",
        since: "1.0",
        doc: "Open tab descriptor.",
        fields: TYPE_TAB_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanLocalBranch",
        since: "1.0",
        doc: "Local branch descriptor.",
        fields: TYPE_LOCAL_BRANCH_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanRemoteBranch",
        since: "1.0",
        doc: "Remote branch descriptor.",
        fields: TYPE_REMOTE_BRANCH_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanTag",
        since: "1.0",
        doc: "Git tag descriptor.",
        fields: TYPE_TAG_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanPersistOpenOpts",
        since: "1.0",
        doc: "Persistence open options.",
        fields: TYPE_PERSIST_OPEN_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanPersistStore",
        since: "1.0",
        doc: "Persistence store userdata.",
        fields: &[],
        methods: PERSIST_METHODS,
    },
    ApiType {
        name: "LeviathanHealthContext",
        since: "1.0",
        doc: "Health check context userdata.",
        fields: &[],
        methods: HEALTH_METHODS,
    },
    ApiType {
        name: "leviathan.runtime",
        since: "1.0",
        doc: "Runtimepath introspection namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanRuntimePathEntry",
        since: "1.0",
        doc: "One entry in the plugin's runtime path.",
        fields: TYPE_RUNTIME_PATH_ENTRY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanRuntimeMatch",
        since: "1.0",
        doc: "A successful module name resolution.",
        fields: TYPE_RUNTIME_MATCH_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanRuntimeModuleGraphEntry",
        since: "1.0",
        doc: "Per-plugin grouping of currently-cached modules.",
        fields: TYPE_RUNTIME_MODULE_GRAPH_ENTRY_FIELDS,
        methods: &[],
    },
];

const TYPE_RUNTIME_PATH_ENTRY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "plugin",
        lua_type: "string",
        required: true,
        doc: "Plugin id this entry contributes modules for.",
    },
    ApiTypeField {
        name: "kind",
        lua_type: "string",
        required: true,
        doc: "\"own\" for the calling plugin, \"dependency\" otherwise.",
    },
    ApiTypeField {
        name: "root",
        lua_type: "string",
        required: true,
        doc: "Absolute filesystem path to the contributing `lua/<plugin>/` directory.",
    },
];

const TYPE_RUNTIME_MATCH_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "plugin",
        lua_type: "string",
        required: true,
        doc: "Owning plugin id.",
    },
    ApiTypeField {
        name: "kind",
        lua_type: "string",
        required: true,
        doc: "\"own\" or \"dependency\".",
    },
    ApiTypeField {
        name: "source",
        lua_type: "string",
        required: true,
        doc: "Absolute filesystem path to the matched .lua file.",
    },
];

const TYPE_RUNTIME_MODULE_GRAPH_ENTRY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "plugin",
        lua_type: "string",
        required: true,
        doc: "Plugin id.",
    },
    ApiTypeField {
        name: "modules",
        lua_type: "string[]",
        required: true,
        doc: "Module names currently cached for this plugin in this generation.",
    },
];

pub fn describe() -> HostApiDescription {
    HostApiDescription {
        host_api_version: ApiVersionInfo {
            major: HOST_API_VERSION.major,
            minor: HOST_API_VERSION.minor,
            label: "1.0",
            compatibility: "v1",
        },
        modules: API_MODULES,
        events: API_EVENTS,
        types: API_TYPES,
        capabilities: API_CAPABILITIES,
        regions: describe_regions(),
        widgets: WIDGETS.iter().collect(),
    }
}

pub fn all_functions() -> impl Iterator<Item = &'static ApiFunction> {
    API_MODULES
        .iter()
        .flat_map(|module| module.functions.iter())
}

pub fn function_paths() -> Vec<&'static str> {
    all_functions().map(|function| function.path).collect()
}

pub fn has_feature(feature: &str) -> bool {
    let Some((name, version)) = feature.rsplit_once('@') else {
        return false;
    };
    let Ok(major) = version.parse::<u32>() else {
        return false;
    };
    if major != HOST_API_VERSION.major {
        return false;
    }
    let name = name.strip_prefix("leviathan.").unwrap_or(name);

    API_MODULES
        .iter()
        .any(|module| module.name == name || module.table.strip_prefix("leviathan.") == Some(name))
        || all_functions().any(|function| function_feature_name(function) == name)
        || API_EVENTS
            .iter()
            .any(|event| event.name == name || format!("event.{}", event.name) == name)
        || API_TYPES
            .iter()
            .any(|ty| ty.name == name || ty.name.strip_prefix("leviathan.") == Some(name))
        || API_CAPABILITIES.iter().any(|capability| {
            capability.name == name || format!("capability.{}", capability.name) == name
        })
        || REGIONS
            .iter()
            .any(|region| format!("region.{}", region.name) == name)
        || WIDGETS
            .iter()
            .any(|widget| format!("widget.{}", widget.kind) == name)
}

pub fn module_names() -> Vec<&'static str> {
    API_MODULES.iter().map(|module| module.name).collect()
}

fn function_feature_name(function: &ApiFunction) -> &str {
    function
        .path
        .strip_prefix("leviathan.")
        .unwrap_or(function.path)
}

fn describe_regions() -> Vec<ApiRegion> {
    REGIONS
        .iter()
        .map(|region| match &region.kind {
            RegionKind::Chrome {
                sections,
                dynamic_section_prefixes,
            } => ApiRegion {
                name: region.name,
                kind: "chrome",
                sections: sections.to_vec(),
                panes: Vec::new(),
                dynamic_section_prefixes: dynamic_section_prefixes.to_vec(),
            },
            RegionKind::Content { panes } => ApiRegion {
                name: region.name,
                kind: "content",
                sections: Vec::new(),
                panes: panes
                    .iter()
                    .map(|pane| ApiRegionPane {
                        name: pane.name,
                        sections: pane.sections.to_vec(),
                        dynamic_section_prefixes: pane.dynamic_section_prefixes.to_vec(),
                    })
                    .collect(),
                dynamic_section_prefixes: Vec::new(),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_feature_accepts_current_v1_functions() {
        assert!(has_feature("fs.read_file@1"));
        assert!(has_feature("leviathan.fs.read_file@1"));
        assert!(has_feature("api.describe@1"));
        assert!(has_feature("ui.main_bar.add@1"));
        assert!(has_feature("log@1"));
    }

    #[test]
    fn has_feature_rejects_unknown_or_wrong_major() {
        assert!(!has_feature("fs.nope@1"));
        assert!(!has_feature("fs.read_file@2"));
        assert!(!has_feature("fs.read_file"));
        assert!(!has_feature("fs.read_file@nope"));
    }

    #[test]
    fn every_region_has_compat_module_descriptors() {
        let modules = module_names();
        for region in REGIONS.iter() {
            let expected = format!("ui.{}", region.name);
            assert!(
                modules.iter().any(|module| *module == expected),
                "missing module descriptor for region {expected}"
            );
        }
    }

    #[test]
    fn widget_descriptors_cover_runtime_kinds() {
        let names = WIDGETS.names();
        for kind in [
            "text",
            "button",
            "row",
            "column",
            "container",
            "padding",
            "space",
            "icon",
            "image",
            "scrollable",
            "mouse_area",
            "tablist",
            "resizable_split",
        ] {
            assert!(names.contains(&kind), "missing widget descriptor {kind}");
        }
    }

    #[test]
    fn description_serializes() {
        let value = serde_json::to_value(describe()).unwrap();
        assert_eq!(value["host_api_version"]["label"], "1.0");
        assert!(value["modules"].as_array().unwrap().len() > 5);
        assert!(value["widgets"].as_array().unwrap().len() > 5);
    }
}

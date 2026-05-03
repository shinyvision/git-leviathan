use super::super::schema::*;
use super::function_common::*;

pub(super) const ROOT_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.has",
        name: "has",
        since: "1.0",
        compatibility: "v1",
        doc: "Return true when the host exposes a feature such as `fs.read_file@1`.",
        params: &[ApiParam {
            name: "feature",
            lua_type: "string",
            required: true,
            doc: "`module.feature@major` query.",
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

pub(super) const API_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.api.describe",
        name: "describe",
        since: "1.0",
        compatibility: "v1",
        doc: "Return the full host API table.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "table",
            doc: "Host API table.",
        }],
        capabilities: &[],
        validation: ApiValidation {
            args: &[],
            returns: &["HostApiDescription"],
            notes: &["Shape is generated from git_leviathan_plugin_api::descriptor."],
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
];

pub(super) const UI_FUNCTIONS: &[ApiFunction] = &[
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
    // extension-point APIs (overlay / context_menu /
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
        path: "leviathan.ui.remove_overlay",
        name: "remove_overlay",
        since: "1.0",
        compatibility: "v1",
        doc: "Remove an overlay owned by the calling plugin.",
        params: &[ApiParam {
            name: "id",
            lua_type: "string",
            required: true,
            doc: "Overlay id to remove.",
        }],
        returns: &[],
        capabilities: UI_OVERLAY_CAP,
        validation: ApiValidation {
            args: &["id must be a string"],
            returns: &[],
            notes: &["Only removes an overlay owned by the calling plugin."],
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

const REGION_ADD_SLOT_PARAM: &[ApiParam] = &[ApiParam {
    name: "spec",
    lua_type: "LeviathanSlotSpec",
    required: true,
    doc: "Slot spec including region, section/pane, id, priority, and widget.",
}];

const REGION_REMOVE_SLOT_PARAM: &[ApiParam] = &[ApiParam {
    name: "target",
    lua_type: "LeviathanSlotTarget",
    required: true,
    doc: "Slot address including region, section/pane, and id.",
}];

const REGION_REPLACE_SLOT_PARAMS: &[ApiParam] = &[
    ApiParam {
        name: "target",
        lua_type: "LeviathanSlotTarget",
        required: true,
        doc: "Existing slot address including region and id.",
    },
    ApiParam {
        name: "spec",
        lua_type: "LeviathanSlotSpec",
        required: true,
        doc: "Replacement slot spec.",
    },
];

const REGION_VALIDATION: ApiValidation = ApiValidation {
    args: &[
        "section must match the region descriptor",
        "content regions require pane",
        "widget must validate as a LeviathanWidget or be a function",
    ],
    returns: &[],
    notes: &["Slot ownership is recorded in the resource ledger."],
};

pub(super) const UI_REGIONS_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.ui.regions.add_slot",
        name: "add_slot",
        since: "1.0",
        compatibility: "v1",
        doc: "Add a slot to a UI region.",
        params: REGION_ADD_SLOT_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.regions.remove_slot",
        name: "remove_slot",
        since: "1.0",
        compatibility: "v1",
        doc: "Remove a slot from a UI region.",
        params: REGION_REMOVE_SLOT_PARAM,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.regions.replace_slot",
        name: "replace_slot",
        since: "1.0",
        compatibility: "v1",
        doc: "Replace a slot in a UI region.",
        params: REGION_REPLACE_SLOT_PARAMS,
        returns: &[],
        capabilities: &[],
        validation: REGION_VALIDATION,
    },
];

// extension points: cross-region extension-point APIs (overlays, context menu
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

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
        capabilities: &["ui:screen"],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.schedule",
        name: "schedule",
        since: "1.0",
        compatibility: "v1",
        doc: "Defer a callback to the next tick (top-level alias of `leviathan.api.schedule`).",
        params: &[ApiParam {
            name: "callback",
            lua_type: "fun()",
            required: true,
            doc: "Function invoked on the next tick.",
        }],
        returns: &[],
        capabilities: &["ui:region:<region>"],
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
        capabilities: &[
            "ui:region:<region>",
            "ui:remove:builtin",
            "ui:remove:<region>:<container>:<id>",
        ],
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
        capabilities: &[
            "ui:region:<region>",
            "ui:replace:builtin",
            "ui:replace:<region>:<container>:<id>",
        ],
        validation: ApiValidation {
            args: &["ms must be an integer", "callback must be a function"],
            returns: &[],
            notes: &["The callback is recorded in the resource ledger as a timer."],
        },
    },
];

pub(super) const UI_SCREEN_FUNCTIONS: &[ApiFunction] = &[ApiFunction {
    path: "leviathan.ui.screen.register",
    name: "register",
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
    capabilities: &["ui:screen"],
    validation: ApiValidation {
        args: &[
            "spec.id must be a string",
            "spec.init/view/update must be functions",
        ],
        returns: &[],
        notes: &["serialize, deserialize, and can_close are optional functions."],
    },
}];

pub(super) const UI_FUNCTIONS: &[ApiFunction] = &[
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
                "spec.key_events must be an array of supported keys when present",
            ],
            returns: &[],
            notes: &[
                "Host owns Esc / click-outside dismissal.",
                "Opted-in key events call on_event(id, \"key\", value) before keymaps or screen input.",
            ],
        },
    },
    ApiFunction {
        path: "leviathan.ui.dialog",
        name: "dialog",
        since: "1.0",
        compatibility: "v1",
        doc: "Compatibility callable alias for `leviathan.ui.dialog.open`; opens a repository-owned toolbar dialog on the active repository screen.",
        params: UI_DIALOG_PARAM,
        returns: &[],
        capabilities: UI_OVERLAY_CAP,
        validation: ApiValidation {
            args: &[
                "spec.id must be a string",
                "spec.text must be a string",
                "spec.buttons must contain one or more button specs",
                "button.style must be red, green, blue, or white",
                "button.text must be a string",
                "button.on_click must be a function",
                "button.keys must be an array of supported keys when present",
            ],
            returns: &[],
            notes: &[
                "Requires an active repository screen.",
                "Button clicks call the matching button function.",
                "Configured keys call the same button function as a click.",
                "Escape follows dialog data: a button key binding handles it first, otherwise dismissible dialogs close.",
                "The dialog is rendered in the repository toolbar band.",
            ],
        },
    },
    ApiFunction {
        path: "leviathan.ui.dialog.open",
        name: "dialog.open",
        since: "1.0",
        compatibility: "v1",
        doc: "Open a repository-owned toolbar dialog on the active repository screen; button keys, including Escape, are data-driven.",
        params: UI_DIALOG_PARAM,
        returns: &[],
        capabilities: UI_OVERLAY_CAP,
        validation: ApiValidation {
            args: &[
                "spec.id must be a string",
                "spec.text must be a string",
                "spec.buttons must contain one or more button specs",
                "button.style must be red, green, blue, or white",
                "button.text must be a string",
                "button.on_click must be a function",
                "button.keys must be an array of supported keys when present",
            ],
            returns: &[],
            notes: &[
                "Requires an active repository screen.",
                "Button clicks call the matching button function.",
                "Configured keys call the same button function as a click.",
                "Escape follows dialog data: a button key binding handles it first, otherwise dismissible dialogs close.",
                "The dialog is rendered in the repository toolbar band.",
            ],
        },
    },
    ApiFunction {
        path: "leviathan.ui.dialog.focus_control",
        name: "dialog.focus_control",
        since: "1.0",
        compatibility: "v1",
        doc: "Focus a text input control in the active repository toolbar dialog.",
        params: UI_DIALOG_CONTROL_PARAMS,
        returns: BOOL_NIL_ERR_RET,
        capabilities: UI_OVERLAY_CAP,
        validation: ApiValidation {
            args: &["dialog_id must be a string", "control_id must be a string"],
            returns: &["true plus nil error, or nil plus error string"],
            notes: &["No-ops when the active toolbar dialog or control does not match."],
        },
    },
    ApiFunction {
        path: "leviathan.ui.dialog.press_button",
        name: "dialog.press_button",
        since: "1.0",
        compatibility: "v1",
        doc: "Press a button in the active repository toolbar dialog.",
        params: UI_DIALOG_BUTTON_PARAMS,
        returns: BOOL_NIL_ERR_RET,
        capabilities: UI_OVERLAY_CAP,
        validation: ApiValidation {
            args: &["dialog_id must be a string", "button_id must be a string"],
            returns: &["true plus nil error, or nil plus error string"],
            notes: &[
                "Routes through the same dialog dispatcher as a click.",
                "Disabled buttons are ignored by the host dialog dispatcher.",
            ],
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
        path: "leviathan.ui.contribute",
        name: "contribute",
        since: "1.0",
        compatibility: "v1",
        doc: "Contribute to a typed UI extension point and return a ledger-backed handle.",
        params: UI_CONTRIBUTE_PARAMS,
        returns: CONTRIBUTION_HANDLE_ERR_RET,
        capabilities: UI_CONTRIBUTE_CAP,
        validation: ApiValidation {
            args: &[
                "point_id must be a known extension point",
                "spec.id must be a string",
                "spec.provider may be a function for dynamic decorations",
            ],
            returns: &["LeviathanContributionHandle plus nil error, or nil plus error string"],
            notes: &["Convenience wrappers call this contribution path."],
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

const SLOT_HANDLE_ERR_RET: &[ApiReturn] = &[
    ApiReturn {
        lua_type: "LeviathanSlotHandle|nil",
        doc: "Slot handle on success, nil on failure.",
    },
    ApiReturn {
        lua_type: "string|nil",
        doc: "Error message on failure.",
    },
];

const BOOL_NIL_ERR_RET: &[ApiReturn] = &[
    ApiReturn {
        lua_type: "boolean|nil",
        doc: "True on success, nil on failure.",
    },
    ApiReturn {
        lua_type: "string|nil",
        doc: "Error message on failure.",
    },
];

const REGION_LIST_RET: &[ApiReturn] = &[
    ApiReturn {
        lua_type: "string[]",
        doc: "Region names in descriptor order.",
    },
    ApiReturn {
        lua_type: "string|nil",
        doc: "Error message on failure.",
    },
];

const REGION_DESCRIBE_RET: &[ApiReturn] = &[
    ApiReturn {
        lua_type: "table|nil",
        doc: "Region descriptor on success, nil on failure.",
    },
    ApiReturn {
        lua_type: "string|nil",
        doc: "Error message on failure.",
    },
];

const CONTEXT_CURRENT_RET: &[ApiReturn] = &[
    ApiReturn {
        lua_type: "LeviathanUiContext|nil",
        doc: "Typed current UI context.",
    },
    ApiReturn {
        lua_type: "string|nil",
        doc: "Error message on failure.",
    },
];

const CONTRIBUTION_HANDLE_ERR_RET: &[ApiReturn] = &[
    ApiReturn {
        lua_type: "LeviathanContributionHandle|nil",
        doc: "Contribution handle on success, nil on failure.",
    },
    ApiReturn {
        lua_type: "string|nil",
        doc: "Error message on failure.",
    },
];

const SLOT_VALIDATION: ApiValidation = ApiValidation {
    args: &[
        "region must be a known region",
        "section must match the region descriptor",
        "content regions require pane",
        "widget must validate as a LeviathanWidget or be a function",
        "depends_on entries must be known UI dependencies when present",
    ],
    returns: &["success value plus nil error, or nil plus error string"],
    notes: &["Use handle:remove() and handle:replace(spec) for owned slot resources."],
};

pub(super) const UI_SLOT_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.ui.slot.add",
        name: "add",
        since: "1.0",
        compatibility: "v1",
        doc: "Add a slot and return a ledger-backed handle.",
        params: REGION_ADD_SLOT_PARAM,
        returns: SLOT_HANDLE_ERR_RET,
        capabilities: &["ui:region:<region>"],
        validation: SLOT_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.slot.remove",
        name: "remove",
        since: "1.0",
        compatibility: "v1",
        doc: "Remove a slot by full address.",
        params: REGION_REMOVE_SLOT_PARAM,
        returns: BOOL_NIL_ERR_RET,
        capabilities: &[
            "ui:region:<region>",
            "ui:remove:builtin",
            "ui:remove:<region>:<container>:<id>",
        ],
        validation: SLOT_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.slot.replace",
        name: "replace",
        since: "1.0",
        compatibility: "v1",
        doc: "Replace a slot by full address and return a handle.",
        params: REGION_REPLACE_SLOT_PARAMS,
        returns: SLOT_HANDLE_ERR_RET,
        capabilities: &[
            "ui:region:<region>",
            "ui:replace:builtin",
            "ui:replace:<region>:<container>:<id>",
        ],
        validation: SLOT_VALIDATION,
    },
];

pub(super) const UI_REGION_FUNCTIONS: &[ApiFunction] = &[
    ApiFunction {
        path: "leviathan.ui.region.list",
        name: "list",
        since: "1.0",
        compatibility: "v1",
        doc: "List mounted UI regions.",
        params: &[],
        returns: REGION_LIST_RET,
        capabilities: &[],
        validation: NO_VALIDATION,
    },
    ApiFunction {
        path: "leviathan.ui.region.describe",
        name: "describe",
        since: "1.0",
        compatibility: "v1",
        doc: "Describe one mounted UI region.",
        params: &[ApiParam {
            name: "name",
            lua_type: "string",
            required: true,
            doc: "Region name.",
        }],
        returns: REGION_DESCRIBE_RET,
        capabilities: &[],
        validation: NO_VALIDATION,
    },
];

pub(super) const UI_CONTEXT_FUNCTIONS: &[ApiFunction] = &[ApiFunction {
    path: "leviathan.ui.context.current",
    name: "current",
    since: "1.0",
    compatibility: "v1",
    doc: "Return the current typed UI context.",
    params: &[],
    returns: CONTEXT_CURRENT_RET,
    capabilities: &[],
    validation: NO_VALIDATION,
}];

pub(super) const UI_DOCK_FUNCTIONS: &[ApiFunction] = &[ApiFunction {
    path: "leviathan.ui.dock.register",
    name: "register",
    since: "1.0",
    compatibility: "v1",
    doc: "Register a persistent dock panel with host-owned layout state.",
    params: &[ApiParam {
        name: "spec",
        lua_type: "LeviathanDockPanelSpec",
        required: true,
        doc: "Dock panel descriptor.",
    }],
    returns: &[
        ApiReturn {
            lua_type: "LeviathanDockPanelHandle|nil",
            doc: "Dock panel handle on success.",
        },
        ApiReturn {
            lua_type: "string|nil",
            doc: "Error message on failure.",
        },
    ],
    capabilities: &["ui:dock"],
    validation: ApiValidation {
        args: &[
            "spec.id/title/area must be strings",
            "spec.view must be a function",
            "spec.update is optional",
        ],
        returns: &["handle plus nil error, or nil plus error string"],
        notes: &["view(ctx) returns a LeviathanWidget; update(state, event, value) may return { state = new_state }."],
    },
}];

pub(super) const UI_SETTINGS_FUNCTIONS: &[ApiFunction] = &[ApiFunction {
    path: "leviathan.ui.settings.register",
    name: "register",
    since: "1.0",
    compatibility: "v1",
    doc: "Register a custom settings panel view for the plugin.",
    params: &[ApiParam {
        name: "spec",
        lua_type: "LeviathanSettingsPanelSpec",
        required: true,
        doc: "Settings panel descriptor.",
    }],
    returns: &[],
    capabilities: &[],
    validation: ApiValidation {
        args: &["spec.view must be a function"],
        returns: &[],
        notes: &[
            "Schema-only plugins get a generated settings form from leviathan.settings.define_schema.",
        ],
    },
}];

pub(super) const ASSETS_FUNCTIONS: &[ApiFunction] = &[ApiFunction {
    path: "leviathan.assets.load_svg",
    name: "load_svg",
    since: "1.0",
    compatibility: "v1",
    doc: "Return an SVG asset handle rooted in the plugin assets directory.",
    params: &[ApiParam {
        name: "path",
        lua_type: "string",
        required: true,
        doc: "Relative asset path.",
    }],
    returns: &[
        ApiReturn {
            lua_type: "LeviathanAssetHandle|nil",
            doc: "Asset handle on success.",
        },
        ApiReturn {
            lua_type: "string|nil",
            doc: "Error string on failure.",
        },
    ],
    capabilities: &[],
    validation: ApiValidation {
        args: &["path must be a safe relative path under the plugin directory"],
        returns: &["asset handle plus nil error, or nil plus error string"],
        notes: &["Use the returned handle in widget asset fields."],
    },
}];

// extension points: cross-region extension-point APIs (overlays, context menu
// items, graph decorations, diff decorations). Each surface is
// capability-gated: see `UI_EXT_*_CAP`.
const UI_OVERLAY_CAP: &[&str] = &["ui:overlay"];
const UI_CONTEXT_MENU_CAP: &[&str] = &["ui:context_menu:<region>"];
const UI_GRAPH_DECORATION_CAP: &[&str] = &["ui:decoration:graph"];
const UI_DIFF_DECORATION_CAP: &[&str] = &["ui:decoration:diff"];
const UI_CONTRIBUTE_CAP: &[&str] = &[
    "ui:region:<region>",
    "ui:context_menu:<region>",
    "ui:decoration:graph",
    "ui:decoration:diff",
    "ui:overlay",
    "ui:screen",
    "ui:dock",
];

const UI_CONTRIBUTE_PARAMS: &[ApiParam] = &[
    ApiParam {
        name: "point_id",
        lua_type: "string",
        required: true,
        doc: "Extension point id such as `repository.diff.context_menu`.",
    },
    ApiParam {
        name: "spec",
        lua_type: "LeviathanContributionSpec",
        required: true,
        doc: "Contribution spec for the selected point.",
    },
];

const UI_OVERLAY_PARAM: &[ApiParam] = &[ApiParam {
    name: "spec",
    lua_type: "LeviathanOverlaySpec",
    required: true,
    doc: "Overlay descriptor (id, widget, dismissible, priority, key_events).",
}];

const UI_DIALOG_PARAM: &[ApiParam] = &[ApiParam {
    name: "spec",
    lua_type: "LeviathanDialogSpec",
    required: true,
    doc: "Repository toolbar dialog descriptor (id, text, optional controls, buttons).",
}];

const UI_DIALOG_CONTROL_PARAMS: &[ApiParam] = &[
    ApiParam {
        name: "dialog_id",
        lua_type: "string",
        required: true,
        doc: "Active toolbar dialog id.",
    },
    ApiParam {
        name: "control_id",
        lua_type: "string",
        required: true,
        doc: "Dialog control id to focus.",
    },
];

const UI_DIALOG_BUTTON_PARAMS: &[ApiParam] = &[
    ApiParam {
        name: "dialog_id",
        lua_type: "string",
        required: true,
        doc: "Active toolbar dialog id.",
    },
    ApiParam {
        name: "button_id",
        lua_type: "string",
        required: true,
        doc: "Dialog button id to press.",
    },
];

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

use super::schema::*;

mod field_sets;
mod shell;

use field_sets::{
    TYPE_ASSET_HANDLE_FIELDS, TYPE_DOCK_PANEL_HANDLE_FIELDS, TYPE_DOCK_PANEL_SPEC_FIELDS,
    TYPE_SCREEN_FIELDS, TYPE_SETTINGS_CONTEXT_FIELDS, TYPE_SETTINGS_PANEL_SPEC_FIELDS,
    TYPE_SLOT_FIELDS, TYPE_SLOT_HANDLE_FIELDS, TYPE_SLOT_TARGET_FIELDS, TYPE_UI_FIELDS,
};
use shell::{
    TYPE_SHELL_JOB_HANDLE_METHODS, TYPE_SHELL_NAMESPACE_FIELDS, TYPE_SHELL_OPEN_SPEC_FIELDS,
    TYPE_SHELL_RUN_RESULT_FIELDS, TYPE_SHELL_RUN_SPEC_FIELDS,
};

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
        name: "assets",
        lua_type: "leviathan.assets",
        required: true,
        doc: "Asset handle namespace.",
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
        name: "shell",
        lua_type: "leviathan.shell",
        required: true,
        doc: "Host default shell namespace.",
    },
    ApiTypeField {
        name: "bash",
        lua_type: "leviathan.bash",
        required: true,
        doc: "Bash shell namespace.",
    },
    ApiTypeField {
        name: "zsh",
        lua_type: "leviathan.zsh",
        required: true,
        doc: "Zsh shell namespace.",
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
const TYPE_OVERLAY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "id",
        lua_type: "string",
        required: true,
        doc: "Overlay id.",
    },
    ApiTypeField {
        name: "priority",
        lua_type: "integer",
        required: false,
        doc: "Higher priority renders above lower priority overlays.",
    },
    ApiTypeField {
        name: "dismissible",
        lua_type: "boolean",
        required: false,
        doc: "When true, Escape sends the overlay an `escape` event.",
    },
    ApiTypeField {
        name: "key_events",
        lua_type: "string[]",
        required: false,
        doc: "Named keys this overlay captures as `key` events, e.g. `Tab`, `ArrowUp`, `ArrowDown`.",
    },
    ApiTypeField {
        name: "widget",
        lua_type: "LeviathanWidget",
        required: true,
        doc: "Overlay widget tree.",
    },
    ApiTypeField {
        name: "on_event",
        lua_type: "fun(id: string, event: string, value: LeviathanJson|LeviathanOverlayKeyEvent): table|nil",
        required: false,
        doc: "Overlay callback.",
    },
    ApiTypeField {
        name: "update",
        lua_type: "fun(id: string, event: string, value: LeviathanJson|LeviathanOverlayKeyEvent): table|nil",
        required: false,
        doc: "Alias for `on_event`.",
    },
];
const TYPE_OVERLAY_KEY_EVENT_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "key",
        lua_type: "string",
        required: true,
        doc: "Normalized key name: `tab`, `up`, `down`, etc.",
    },
    ApiTypeField {
        name: "ctrl",
        lua_type: "boolean",
        required: true,
        doc: "Whether Ctrl was held.",
    },
    ApiTypeField {
        name: "shift",
        lua_type: "boolean",
        required: true,
        doc: "Whether Shift was held.",
    },
    ApiTypeField {
        name: "alt",
        lua_type: "boolean",
        required: true,
        doc: "Whether Alt was held.",
    },
    ApiTypeField {
        name: "logo",
        lua_type: "boolean",
        required: true,
        doc: "Whether the platform logo key was held.",
    },
    ApiTypeField {
        name: "command",
        lua_type: "boolean",
        required: true,
        doc: "Whether the platform command modifier was held.",
    },
];
const TYPE_CONTEXT_MENU_ITEM_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "id",
        lua_type: "string",
        required: true,
        doc: "Contribution id.",
    },
    ApiTypeField {
        name: "label",
        lua_type: "string",
        required: true,
        doc: "Menu label.",
    },
    ApiTypeField {
        name: "command",
        lua_type: "string",
        required: true,
        doc: "Command id invoked by the item.",
    },
    ApiTypeField {
        name: "priority",
        lua_type: "integer",
        required: false,
        doc: "Lower values render earlier.",
    },
];

const TYPE_GRAPH_CONTRIBUTION_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "id",
        lua_type: "string",
        required: true,
        doc: "Contribution id.",
    },
    ApiTypeField {
        name: "commit_hash",
        lua_type: "string",
        required: false,
        doc: "Static target commit hash.",
    },
    ApiTypeField {
        name: "decoration",
        lua_type: "LeviathanGraphDecoration",
        required: false,
        doc: "Static graph decoration.",
    },
    ApiTypeField {
        name: "provider",
        lua_type: "fun(ctx: RepositoryGraphRowContext): LeviathanGraphDecoration|LeviathanGraphDecoration[]|nil",
        required: false,
        doc: "Dynamic provider called per graph row.",
    },
];

const TYPE_DIFF_CONTRIBUTION_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "id",
        lua_type: "string",
        required: true,
        doc: "Contribution id.",
    },
    ApiTypeField {
        name: "decoration",
        lua_type: "LeviathanDiffDecoration",
        required: false,
        doc: "Static diff decoration.",
    },
    ApiTypeField {
        name: "provider",
        lua_type: "fun(ctx: RepositoryDiffLineContext): LeviathanDiffDecoration|LeviathanDiffDecoration[]|nil",
        required: false,
        doc: "Dynamic provider called per diff line.",
    },
];

const TYPE_CONTRIBUTION_HANDLE_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "plugin_id",
        lua_type: "string",
        required: true,
        doc: "Owner plugin id.",
    },
    ApiTypeField {
        name: "point_id",
        lua_type: "string",
        required: true,
        doc: "Extension point id.",
    },
    ApiTypeField {
        name: "id",
        lua_type: "string",
        required: true,
        doc: "Contribution id.",
    },
];

const SLOT_HANDLE_METHODS: &[ApiTypeMethod] = &[
    ApiTypeMethod {
        name: "remove",
        doc: "Remove this slot if the handle is still live.",
        params: &[],
        returns: &[
            ApiReturn {
                lua_type: "boolean|nil",
                doc: "True on success, nil on failure.",
            },
            ApiReturn {
                lua_type: "string|nil",
                doc: "Error message on failure.",
            },
        ],
    },
    ApiTypeMethod {
        name: "replace",
        doc: "Replace this slot if the handle is still live.",
        params: &[ApiParam {
            name: "spec",
            lua_type: "LeviathanSlotSpec",
            required: true,
            doc: "Replacement spec. Address fields may be omitted.",
        }],
        returns: &[
            ApiReturn {
                lua_type: "LeviathanSlotHandle|nil",
                doc: "The same handle on success, nil on failure.",
            },
            ApiReturn {
                lua_type: "string|nil",
                doc: "Error message on failure.",
            },
        ],
    },
];

const CONTRIBUTION_HANDLE_METHODS: &[ApiTypeMethod] = &[ApiTypeMethod {
    name: "remove",
    doc: "Remove this contribution if the handle is still live.",
    params: &[],
    returns: &[
        ApiReturn {
            lua_type: "boolean|nil",
            doc: "True on success, nil on failure.",
        },
        ApiReturn {
            lua_type: "string|nil",
            doc: "Error message on failure.",
        },
    ],
}];

const TYPE_UI_CONTEXT_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "version",
        lua_type: "integer",
        required: true,
        doc: "Context schema version.",
    },
    ApiTypeField {
        name: "plugin_id",
        lua_type: "string",
        required: true,
        doc: "Current plugin id.",
    },
    ApiTypeField {
        name: "generation_id",
        lua_type: "integer",
        required: true,
        doc: "Current plugin generation.",
    },
    ApiTypeField {
        name: "type",
        lua_type: "string",
        required: true,
        doc: "Concrete context type name.",
    },
    ApiTypeField {
        name: "surface",
        lua_type: "string",
        required: true,
        doc: "Current coarse UI surface.",
    },
    ApiTypeField {
        name: "features",
        lua_type: "table<string, boolean>",
        required: true,
        doc: "Relevant feature gates.",
    },
    ApiTypeField {
        name: "theme",
        lua_type: "LeviathanUiThemeTokens",
        required: true,
        doc: "Stable host theme tokens.",
    },
    ApiTypeField {
        name: "repository",
        lua_type: "LeviathanRepositorySummary",
        required: true,
        doc: "Active repository summary without commit lists or diffs.",
    },
    ApiTypeField {
        name: "tab",
        lua_type: "LeviathanTabSummary",
        required: true,
        doc: "Active tab summary.",
    },
    ApiTypeField {
        name: "selection",
        lua_type: "LeviathanSelectionSummary",
        required: true,
        doc: "Selection summary when the host has one for the surface.",
    },
    ApiTypeField {
        name: "focus",
        lua_type: "LeviathanFocusSummary",
        required: true,
        doc: "Focused surface, region, pane, and section where available.",
    },
    ApiTypeField {
        name: "viewport",
        lua_type: "LeviathanViewportSummary",
        required: true,
        doc: "Viewport constraints when known.",
    },
    ApiTypeField {
        name: "payload",
        lua_type: "table",
        required: true,
        doc: "Surface-specific ids and lightweight metadata.",
    },
];

const TYPE_UI_THEME_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Theme id.",
    },
    ApiTypeField {
        name: "colors",
        lua_type: "table<string, string>",
        required: true,
        doc: "Hex color tokens.",
    },
    ApiTypeField {
        name: "dimensions",
        lua_type: "table<string, number>",
        required: true,
        doc: "Dimension tokens in pixels.",
    },
    ApiTypeField {
        name: "fonts",
        lua_type: "table<string, number>",
        required: true,
        doc: "Font size tokens in pixels.",
    },
];

const TYPE_REPOSITORY_SUMMARY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "is_open",
        lua_type: "boolean",
        required: true,
        doc: "Whether a repository is active.",
    },
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
        doc: "Active workdir path.",
    },
    ApiTypeField {
        name: "current_branch_name",
        lua_type: "string",
        required: true,
        doc: "Current branch display name.",
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
        name: "has_remote",
        lua_type: "boolean",
        required: true,
        doc: "Whether a default remote is available.",
    },
];

const TYPE_TAB_SUMMARY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "is_open",
        lua_type: "boolean",
        required: true,
        doc: "Whether a tab is active.",
    },
    ApiTypeField {
        name: "id",
        lua_type: "integer|nil",
        required: true,
        doc: "Active tab id.",
    },
    ApiTypeField {
        name: "path",
        lua_type: "string",
        required: true,
        doc: "Active tab repository path.",
    },
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Active tab display name.",
    },
    ApiTypeField {
        name: "index",
        lua_type: "integer|nil",
        required: true,
        doc: "Zero-based active tab index.",
    },
    ApiTypeField {
        name: "count",
        lua_type: "integer",
        required: true,
        doc: "Open tab count.",
    },
];

const TYPE_SELECTION_SUMMARY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "available",
        lua_type: "boolean",
        required: true,
        doc: "Whether selection data is available for this surface.",
    },
    ApiTypeField {
        name: "kind",
        lua_type: "string",
        required: true,
        doc: "Selection kind.",
    },
    ApiTypeField {
        name: "selected_commit_id",
        lua_type: "string|nil",
        required: true,
        doc: "Selected commit id when available.",
    },
    ApiTypeField {
        name: "selected_file_path",
        lua_type: "string|nil",
        required: true,
        doc: "Selected file path when available.",
    },
];

const TYPE_FOCUS_SUMMARY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "surface",
        lua_type: "string",
        required: true,
        doc: "Focused surface.",
    },
    ApiTypeField {
        name: "region",
        lua_type: "string",
        required: false,
        doc: "Focused region.",
    },
    ApiTypeField {
        name: "pane",
        lua_type: "string",
        required: false,
        doc: "Focused pane.",
    },
    ApiTypeField {
        name: "section",
        lua_type: "string",
        required: false,
        doc: "Focused section.",
    },
];

const TYPE_VIEWPORT_SUMMARY_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "known",
        lua_type: "boolean",
        required: true,
        doc: "Whether concrete viewport dimensions are known.",
    },
    ApiTypeField {
        name: "width",
        lua_type: "number|nil",
        required: true,
        doc: "Viewport width.",
    },
    ApiTypeField {
        name: "height",
        lua_type: "number|nil",
        required: true,
        doc: "Viewport height.",
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
        doc: "Migrations.",
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
        doc: "Description shown in the palette and devtools.",
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
        doc: "Description.",
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
    ApiTypeField {
        name: "capabilities",
        lua_type: "string[]",
        required: true,
        doc: "Capabilities checked against the command owner.",
    },
    ApiTypeField {
        name: "plugin_invocation_capabilities",
        lua_type: "string[]",
        required: true,
        doc: "Capabilities checked against a plugin invoking this command.",
    },
    ApiTypeField {
        name: "enabled",
        lua_type: "boolean",
        required: true,
        doc: "Current static enabled status.",
    },
    ApiTypeField {
        name: "disabled_reason",
        lua_type: "string|nil",
        required: false,
        doc: "Reason shown when disabled.",
    },
    ApiTypeField {
        name: "keymap_eligible",
        lua_type: "boolean",
        required: true,
        doc: "Whether keymaps should dispatch this command.",
    },
    ApiTypeField {
        name: "palette_visible",
        lua_type: "boolean",
        required: true,
        doc: "Whether palettes should list this command.",
    },
    ApiTypeField {
        name: "hooks",
        lua_type: "table",
        required: true,
        doc: "Supported hook metadata: before, after, veto, replace.",
    },
];

const TYPE_KEYMAP_OPTS_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "description",
        lua_type: "string",
        required: false,
        doc: "Description shown in devtools and the keymap inspector.",
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
        doc: "Description supplied at registration.",
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
        fields: TYPE_UI_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "leviathan.ui.slot",
        since: "1.0",
        doc: "UI slot namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.ui.region",
        since: "1.0",
        doc: "UI region namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.ui.context",
        since: "1.0",
        doc: "UI context namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.ui.screen",
        since: "1.0",
        doc: "Plugin screen namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.ui.dock",
        since: "1.0",
        doc: "Persistent dock panel namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.assets",
        since: "1.0",
        doc: "Plugin asset namespace.",
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
        name: "leviathan.shell",
        since: "1.0",
        doc: "Host default shell namespace.",
        fields: TYPE_SHELL_NAMESPACE_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "leviathan.bash",
        since: "1.0",
        doc: "Bash shell namespace.",
        fields: TYPE_SHELL_NAMESPACE_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "leviathan.zsh",
        since: "1.0",
        doc: "Zsh shell namespace.",
        fields: TYPE_SHELL_NAMESPACE_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanShellRunSpec",
        since: "1.0",
        doc: "Shell command execution spec.",
        fields: TYPE_SHELL_RUN_SPEC_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanShellOpenSpec",
        since: "1.0",
        doc: "Interactive PTY shell session spec.",
        fields: TYPE_SHELL_OPEN_SPEC_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanShellRunResult",
        since: "1.0",
        doc: "Completed shell command result.",
        fields: TYPE_SHELL_RUN_RESULT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanShellJobHandle",
        since: "1.0",
        doc: "Cancellable shell job handle.",
        fields: &[],
        methods: TYPE_SHELL_JOB_HANDLE_METHODS,
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
        since: "1.0",
        doc: "Settings namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanSettingsSchema",
        since: "1.0",
        doc: "Settings schema table.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.secrets",
        since: "1.0",
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
        since: "1.0",
        doc: "Typed event payload table handed to autocmd callbacks.",
        fields: TYPE_AUTOCMD_EVENT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanAutocmdGroupOpts",
        since: "1.0",
        doc: "Options accepted by `leviathan.autocmd.group`.",
        fields: TYPE_AUTOCMD_GROUP_OPTS_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "leviathan.autocmd",
        since: "1.0",
        doc: "Autocmd group and typed-event namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.command",
        since: "1.0",
        doc: "Typed command registry namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanCommandSpec",
        since: "1.0",
        doc: "Descriptor accepted by `leviathan.command.create`.",
        fields: TYPE_COMMAND_SPEC_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanCommandArg",
        since: "1.0",
        doc: "One entry in a command's `args` schema.",
        fields: TYPE_COMMAND_ARG_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanCommandSummary",
        since: "1.0",
        doc: "Compact view of a registered command, returned by `leviathan.command.list`.",
        fields: TYPE_COMMAND_SUMMARY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "leviathan.keymap",
        since: "1.0",
        doc: "Context-aware keymap registry namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanKeymapOpts",
        since: "1.0",
        doc: "Options table accepted by `leviathan.keymap.set`.",
        fields: TYPE_KEYMAP_OPTS_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanKeymapSummary",
        since: "1.0",
        doc: "Compact view of a registered keymap, returned by `leviathan.keymap.list`.",
        fields: TYPE_KEYMAP_SUMMARY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanKeymapConflictRef",
        since: "1.0",
        doc: "Reference to the winning binding for a conflict-lost keymap row.",
        fields: TYPE_KEYMAP_CONFLICT_REF_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanOverlaySpec",
        since: "1.0",
        doc: "Plugin overlay.",
        fields: TYPE_OVERLAY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanOverlayKeyEvent",
        since: "1.0",
        doc: "Payload passed to overlay `on_event` when `event == \"key\"`.",
        fields: TYPE_OVERLAY_KEY_EVENT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanScreenSpec",
        since: "1.0",
        doc: "Plugin screen.",
        fields: TYPE_SCREEN_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanSlotSpec",
        since: "1.0",
        doc: "UI slot.",
        fields: TYPE_SLOT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanSlotTarget",
        since: "1.0",
        doc: "UI slot target.",
        fields: TYPE_SLOT_TARGET_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanSlotHandle",
        since: "1.0",
        doc: "Ledger-backed UI slot handle.",
        fields: TYPE_SLOT_HANDLE_FIELDS,
        methods: SLOT_HANDLE_METHODS,
    },
    ApiType {
        name: "LeviathanContextMenuItem",
        since: "1.0",
        doc: "Context-menu contribution spec.",
        fields: TYPE_CONTEXT_MENU_ITEM_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanGraphDecorationContribution",
        since: "1.0",
        doc: "Static or dynamic graph-row decoration contribution.",
        fields: TYPE_GRAPH_CONTRIBUTION_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanDiffDecorationContribution",
        since: "1.0",
        doc: "Static or dynamic diff decoration contribution.",
        fields: TYPE_DIFF_CONTRIBUTION_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanContributionSpec",
        since: "1.0",
        doc: "Base spec accepted by `leviathan.ui.contribute`.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanContributionHandle",
        since: "1.0",
        doc: "Ledger-backed UI contribution handle.",
        fields: TYPE_CONTRIBUTION_HANDLE_FIELDS,
        methods: CONTRIBUTION_HANDLE_METHODS,
    },
    ApiType {
        name: "LeviathanUiContext",
        since: "1.0",
        doc: "Base UI context.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "MainBarContext",
        since: "1.0",
        doc: "Context passed to main-bar dynamic widgets.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "TabBarContext",
        since: "1.0",
        doc: "Context passed to tab-bar dynamic widgets.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "StatusBarContext",
        since: "1.0",
        doc: "Reserved status-bar context schema.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "RepositorySidebarContext",
        since: "1.0",
        doc: "Context passed to repository sidebar slot widgets.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "RepositoryGraphContext",
        since: "1.0",
        doc: "Context passed to repository graph slot widgets.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "RepositoryGraphRowContext",
        since: "1.0",
        doc: "Reserved repository graph row context schema.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "RepositoryDetailsContext",
        since: "1.0",
        doc: "Context passed to repository details slot widgets.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "RepositoryDiffContext",
        since: "1.0",
        doc: "Reserved repository diff context schema.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "RepositoryDiffLineContext",
        since: "1.0",
        doc: "Reserved repository diff line context schema.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "OverlayContext",
        since: "1.0",
        doc: "Reserved overlay context schema.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "ScreenContext",
        since: "1.0",
        doc: "Context shape returned outside a mounted slot render.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "SettingsContext",
        since: "1.0",
        doc: "Context passed to settings panel render callbacks.",
        fields: TYPE_SETTINGS_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "DockPanelContext",
        since: "1.0",
        doc: "Context passed to dock panel render callbacks.",
        fields: TYPE_UI_CONTEXT_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanUiThemeTokens",
        since: "1.0",
        doc: "Stable theme tokens in a UI context.",
        fields: TYPE_UI_THEME_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanRepositorySummary",
        since: "1.0",
        doc: "Active repository summary in a UI context.",
        fields: TYPE_REPOSITORY_SUMMARY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanTabSummary",
        since: "1.0",
        doc: "Active tab summary in a UI context.",
        fields: TYPE_TAB_SUMMARY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanSelectionSummary",
        since: "1.0",
        doc: "Selection summary in a UI context.",
        fields: TYPE_SELECTION_SUMMARY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanFocusSummary",
        since: "1.0",
        doc: "Focus summary in a UI context.",
        fields: TYPE_FOCUS_SUMMARY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanViewportSummary",
        since: "1.0",
        doc: "Viewport summary in a UI context.",
        fields: TYPE_VIEWPORT_SUMMARY_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanWidget",
        since: "1.0",
        doc: "Tagged widget tree node.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "leviathan.ui.settings",
        since: "1.0",
        doc: "Plugin settings panel namespace.",
        fields: &[],
        methods: &[],
    },
    ApiType {
        name: "LeviathanDockPanelSpec",
        since: "1.0",
        doc: "Persistent dock panel registration spec.",
        fields: TYPE_DOCK_PANEL_SPEC_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanSettingsPanelSpec",
        since: "1.0",
        doc: "Custom settings panel registration spec.",
        fields: TYPE_SETTINGS_PANEL_SPEC_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanDockPanelHandle",
        since: "1.0",
        doc: "Dock panel registration handle.",
        fields: TYPE_DOCK_PANEL_HANDLE_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanAssetHandle",
        since: "1.0",
        doc: "Opaque plugin asset handle.",
        fields: TYPE_ASSET_HANDLE_FIELDS,
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
        doc: "Open tab.",
        fields: TYPE_TAB_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanLocalBranch",
        since: "1.0",
        doc: "Local branch.",
        fields: TYPE_LOCAL_BRANCH_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanRemoteBranch",
        since: "1.0",
        doc: "Remote branch.",
        fields: TYPE_REMOTE_BRANCH_FIELDS,
        methods: &[],
    },
    ApiType {
        name: "LeviathanTag",
        since: "1.0",
        doc: "Git tag.",
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

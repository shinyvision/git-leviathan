use super::schema::*;

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
        doc: "Typed command registry namespace.",
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
        doc: "Context-aware keymap registry namespace.",
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
        name: "LeviathanWidget",
        since: "1.0",
        doc: "Tagged widget tree node.",
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

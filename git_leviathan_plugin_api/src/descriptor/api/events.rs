use super::schema::*;

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
    ApiTypeField {
        name: "ok",
        lua_type: "boolean",
        required: true,
        doc: "True when command dispatch succeeded.",
    },
    ApiTypeField {
        name: "duration_ms",
        lua_type: "integer",
        required: false,
        doc: "Dispatch duration in milliseconds.",
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
        since: "1.0",
        doc: "Fired once after the host app finishes starting up.",
        payload_type: "table",
        payload_fields: PAYLOAD_EMPTY,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "AppWillQuit",
        since: "1.0",
        doc: "Fired immediately before the host app shuts down.",
        payload_type: "table",
        payload_fields: PAYLOAD_EMPTY,
        aliases: &[],
        is_alias: false,
    },
    // ---- Repository ----
    ApiEvent {
        name: "RepositoryOpened",
        since: "1.0",
        doc: "Fired after a repository is opened.",
        payload_type: "table",
        payload_fields: PAYLOAD_REPOSITORY_REF,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "RepositoryClosed",
        since: "1.0",
        doc: "Fired after a repository is closed.",
        payload_type: "table",
        payload_fields: PAYLOAD_REPOSITORY_REF,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "RepositoryChanged",
        since: "1.0",
        doc: "Fired after the active repository changes.",
        payload_type: "table",
        payload_fields: PAYLOAD_REPOSITORY_REF,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "RefsChanged",
        since: "1.0",
        doc: "Fired after the repository ref set changes (push/fetch/branch ops).",
        payload_type: "table",
        payload_fields: PAYLOAD_REFS,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "HeadChanged",
        since: "1.0",
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
        since: "1.0",
        doc: "Fired after the user selects a commit in the history view.",
        payload_type: "table",
        payload_fields: PAYLOAD_COMMIT,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "CommitListChanged",
        since: "1.0",
        doc: "Fired after the visible commit list changes.",
        payload_type: "table",
        payload_fields: PAYLOAD_COMMIT_LIST,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "DiffLoaded",
        since: "1.0",
        doc: "Fired after a diff finishes loading.",
        payload_type: "table",
        payload_fields: PAYLOAD_DIFF,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "WorktreeChanged",
        since: "1.0",
        doc: "Fired when the worktree contents change.",
        payload_type: "table",
        payload_fields: PAYLOAD_WORKTREE,
        aliases: &[],
        is_alias: false,
    },
    // ---- Network ops ----
    ApiEvent {
        name: "FetchStarted",
        since: "1.0",
        doc: "Fired when a repository fetch starts.",
        payload_type: "table",
        payload_fields: PAYLOAD_FETCH,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "FetchFinished",
        since: "1.0",
        doc: "Fired when a repository fetch finishes (success or failure).",
        payload_type: "table",
        payload_fields: PAYLOAD_FETCH,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "PushStarted",
        since: "1.0",
        doc: "Fired when a push starts.",
        payload_type: "table",
        payload_fields: PAYLOAD_PUSH,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "PushFinished",
        since: "1.0",
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
        since: "1.0",
        doc: "Fired after the active tab changes.",
        payload_type: "table",
        payload_fields: PAYLOAD_TAB,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "TabMoved",
        since: "1.0",
        doc: "Fired after tabs are reordered.",
        payload_type: "table",
        payload_fields: PAYLOAD_TAB_MOVED,
        aliases: &[],
        is_alias: false,
    },
    // ---- App state ----
    ApiEvent {
        name: "ThemeChanged",
        since: "1.0",
        doc: "Fired after the active theme changes.",
        payload_type: "table",
        payload_fields: PAYLOAD_THEME,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "SettingsChanged",
        since: "1.0",
        doc: "Fired after a settings entry is updated.",
        payload_type: "table",
        payload_fields: PAYLOAD_SETTINGS,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "CommandExecuted",
        since: "1.0",
        doc: "Fired after a host or plugin command finishes executing.",
        payload_type: "table",
        payload_fields: PAYLOAD_COMMAND,
        aliases: &[],
        is_alias: false,
    },
    ApiEvent {
        name: "KeymapTriggered",
        since: "1.0",
        doc: "Fired after a keymap chord matches and the host dispatches the underlying command.",
        payload_type: "table",
        payload_fields: PAYLOAD_KEYMAP,
        aliases: &[],
        is_alias: false,
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

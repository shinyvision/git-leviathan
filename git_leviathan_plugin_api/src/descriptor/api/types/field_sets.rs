use super::super::schema::*;

pub(super) const TYPE_ASSET_HANDLE_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "path",
        lua_type: "string",
        required: true,
        doc: "Relative asset path.",
    },
    ApiTypeField {
        name: "kind",
        lua_type: "string",
        required: true,
        doc: "Asset kind.",
    },
    ApiTypeField {
        name: "handle",
        lua_type: "string",
        required: true,
        doc: "Opaque host handle.",
    },
];

pub(super) const TYPE_UI_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "slot",
        lua_type: "leviathan.ui.slot",
        required: true,
        doc: "UI slot namespace.",
    },
    ApiTypeField {
        name: "region",
        lua_type: "leviathan.ui.region",
        required: true,
        doc: "UI region namespace.",
    },
    ApiTypeField {
        name: "context",
        lua_type: "leviathan.ui.context",
        required: true,
        doc: "UI context namespace.",
    },
    ApiTypeField {
        name: "dock",
        lua_type: "leviathan.ui.dock",
        required: true,
        doc: "Persistent dock panel namespace.",
    },
    ApiTypeField {
        name: "screen",
        lua_type: "leviathan.ui.screen",
        required: true,
        doc: "Plugin screen namespace.",
    },
    ApiTypeField {
        name: "settings",
        lua_type: "leviathan.ui.settings",
        required: true,
        doc: "Plugin settings panel namespace.",
    },
];

pub(super) const TYPE_SETTINGS_PANEL_SPEC_FIELDS: &[ApiTypeField] = &[ApiTypeField {
    name: "view",
    lua_type: "fun(ctx: SettingsContext): LeviathanWidget",
    required: true,
    doc: "Render callback for the plugin settings panel.",
}];

pub(super) const TYPE_SETTINGS_CONTEXT_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "plugin_id",
        lua_type: "string",
        required: true,
        doc: "Current plugin id.",
    },
    ApiTypeField {
        name: "schema",
        lua_type: "LeviathanSettingsSchema",
        required: true,
        doc: "Declared settings schema.",
    },
    ApiTypeField {
        name: "values",
        lua_type: "table",
        required: true,
        doc: "Current settings values with defaults applied.",
    },
];

pub(super) const TYPE_DOCK_PANEL_SPEC_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "id",
        lua_type: "string",
        required: true,
        doc: "Plugin-local panel id.",
    },
    ApiTypeField {
        name: "title",
        lua_type: "string",
        required: true,
        doc: "Panel title shown by host chrome.",
    },
    ApiTypeField {
        name: "area",
        lua_type: "string",
        required: true,
        doc: "Dock area: left, right, bottom, graph, diff, tab, or floating.",
    },
    ApiTypeField {
        name: "default_open",
        lua_type: "boolean",
        required: false,
        doc: "Initial open state when no user layout exists.",
    },
    ApiTypeField {
        name: "view",
        lua_type: "fun(ctx: DockPanelContext): LeviathanWidget",
        required: true,
        doc: "Render callback.",
    },
    ApiTypeField {
        name: "update",
        lua_type: "fun(state: table, event: string, value: LeviathanJson): table|nil",
        required: false,
        doc: "Event callback. Return `{ state = next_state }` to persist panel state.",
    },
];

pub(super) const TYPE_SLOT_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "region",
        lua_type: "string",
        required: true,
        doc: "Region name.",
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
        lua_type: "LeviathanWidget|fun(ctx: LeviathanUiContext): LeviathanWidget",
        required: true,
        doc: "Static widget or dynamic widget with context.",
    },
    ApiTypeField {
        name: "depends_on",
        lua_type: "string[]",
        required: false,
        doc: "Dynamic refresh dependencies: plugin_state, repository, tab, selection, diff, theme, layout.",
    },
    ApiTypeField {
        name: "on_click",
        lua_type: "fun(slot_id: string, event: string, value: LeviathanJson): table|nil",
        required: false,
        doc: "Slot callback.",
    },
];

pub(super) const TYPE_SLOT_TARGET_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "plugin_id",
        lua_type: "string",
        required: false,
        doc: "Target owner. Omit for the current plugin or builtin slots.",
    },
    ApiTypeField {
        name: "region",
        lua_type: "string",
        required: true,
        doc: "Region name.",
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
];

pub(super) const TYPE_SLOT_HANDLE_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "plugin_id",
        lua_type: "string",
        required: true,
        doc: "Target owner.",
    },
    ApiTypeField {
        name: "region",
        lua_type: "string",
        required: true,
        doc: "Region name.",
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
        name: "address",
        lua_type: "LeviathanSlotTarget",
        required: true,
        doc: "Full slot address.",
    },
];

pub(super) const TYPE_DOCK_PANEL_HANDLE_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "plugin_id",
        lua_type: "string",
        required: true,
        doc: "Owner plugin id.",
    },
    ApiTypeField {
        name: "id",
        lua_type: "string",
        required: true,
        doc: "Plugin-local panel id.",
    },
    ApiTypeField {
        name: "key",
        lua_type: "string",
        required: true,
        doc: "Stable host panel key.",
    },
    ApiTypeField {
        name: "title",
        lua_type: "string",
        required: true,
        doc: "Panel title.",
    },
    ApiTypeField {
        name: "area",
        lua_type: "string",
        required: true,
        doc: "Current dock area.",
    },
];

pub(super) const TYPE_SCREEN_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "id",
        lua_type: "string",
        required: true,
        doc: "Screen id.",
    },
    ApiTypeField {
        name: "title",
        lua_type: "string",
        required: false,
        doc: "Tab title shown by host chrome.",
    },
    ApiTypeField {
        name: "breadcrumbs",
        lua_type: "string[]",
        required: false,
        doc: "Navigation breadcrumbs for host chrome and diagnostics.",
    },
    ApiTypeField {
        name: "bind_repository",
        lua_type: "boolean",
        required: false,
        doc: "Bind the screen tab to the active repository when opened.",
    },
    ApiTypeField {
        name: "init",
        lua_type: "fun(ctx: ScreenContext): table",
        required: true,
        doc: "Initial state callback.",
    },
    ApiTypeField {
        name: "view",
        lua_type: "fun(state: table, ctx: ScreenContext): LeviathanWidget",
        required: true,
        doc: "View callback.",
    },
    ApiTypeField {
        name: "update",
        lua_type:
            "fun(state: table, event: string, value: LeviathanJson, ctx: ScreenContext): table",
        required: true,
        doc: "Update callback.",
    },
    ApiTypeField {
        name: "serialize",
        lua_type: "fun(state: table): LeviathanJson",
        required: false,
        doc: "Reload and restart state serializer.",
    },
    ApiTypeField {
        name: "deserialize",
        lua_type: "fun(value: LeviathanJson, ctx: ScreenContext): table",
        required: false,
        doc: "Reload and restart state deserializer.",
    },
    ApiTypeField {
        name: "can_close",
        lua_type: "fun(state: table, ctx: ScreenContext): boolean",
        required: false,
        doc: "Return false to block tab close.",
    },
];

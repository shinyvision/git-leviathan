use super::super::schema::*;
use super::function_common::*;

pub(super) const TAB_REGISTRY_FUNCTIONS: &[ApiFunction] = &[
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

pub(super) const SERVICES_FUNCTIONS: &[ApiFunction] = &[
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
                doc: "Service name (`greeter`) with a separate version, or combined declaration (`greeter@1`).",
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
                doc: "Service name (`greeter`) with a separate version, or combined declaration (`greeter@1`).",
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

pub(super) const PERSIST_FUNCTIONS: &[ApiFunction] = &[
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
            notes: &["The default state surface uses the plugin state directory."],
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

pub(super) const SETTINGS_FUNCTIONS: &[ApiFunction] = &[
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

pub(super) const SECRETS_FUNCTIONS: &[ApiFunction] = &[
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

pub(super) const RUNTIME_FUNCTIONS: &[ApiFunction] = &[
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

pub(super) const HEALTH_FUNCTIONS: &[ApiFunction] = &[ApiFunction {
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

pub(super) const ASYNC_FUNCTIONS: &[ApiFunction] = &[ApiFunction {
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

pub(super) const TIMER_FUNCTIONS: &[ApiFunction] = &[
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

pub(super) const COMMAND_FUNCTIONS: &[ApiFunction] = &[
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
                doc: "Command spec including title, args, run.",
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
        doc: "List every registered command.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "LeviathanCommandSummary[]",
            doc: "Array of command summaries.",
        }],
        capabilities: &[],
        validation: NO_VALIDATION,
    },
];

pub(super) const KEYMAP_FUNCTIONS: &[ApiFunction] = &[
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

pub(super) const AUTOCMD_FUNCTIONS: &[ApiFunction] = &[
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

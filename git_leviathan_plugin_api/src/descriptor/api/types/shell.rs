use super::super::schema::*;

pub(super) const TYPE_SHELL_NAMESPACE_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "name",
        lua_type: "string",
        required: true,
        doc: "Resolved shell display name.",
    },
    ApiTypeField {
        name: "is_available",
        lua_type: "boolean",
        required: true,
        doc: "Whether the shell executable is available on this host.",
    },
];

pub(super) const TYPE_SHELL_RUN_SPEC_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "command",
        lua_type: "string",
        required: true,
        doc: "Command string passed to the shell.",
    },
    ApiTypeField {
        name: "cwd",
        lua_type: "string|nil",
        required: false,
        doc: "Working directory.",
    },
    ApiTypeField {
        name: "env",
        lua_type: "table<string,string>|nil",
        required: false,
        doc: "Environment variables to add or override.",
    },
    ApiTypeField {
        name: "input",
        lua_type: "string|nil",
        required: false,
        doc: "Stdin payload.",
    },
    ApiTypeField {
        name: "timeout_ms",
        lua_type: "integer|nil",
        required: false,
        doc: "Optional timeout in milliseconds.",
    },
    ApiTypeField {
        name: "max_output_bytes",
        lua_type: "integer|nil",
        required: false,
        doc: "Per-stream output capture limit.",
    },
    ApiTypeField {
        name: "ansi",
        lua_type: "boolean|nil",
        required: false,
        doc: "Set color-friendly terminal environment variables.",
    },
];

pub(super) const TYPE_SHELL_OPEN_SPEC_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "cwd",
        lua_type: "string|nil",
        required: false,
        doc: "Working directory for the interactive shell.",
    },
    ApiTypeField {
        name: "env",
        lua_type: "table<string,string>|nil",
        required: false,
        doc: "Environment variables to add or override.",
    },
    ApiTypeField {
        name: "rows",
        lua_type: "integer|nil",
        required: false,
        doc: "Initial terminal row count.",
    },
    ApiTypeField {
        name: "cols",
        lua_type: "integer|nil",
        required: false,
        doc: "Initial terminal column count.",
    },
];

pub(super) const TYPE_SHELL_RUN_RESULT_FIELDS: &[ApiTypeField] = &[
    ApiTypeField {
        name: "shell",
        lua_type: "string",
        required: true,
        doc: "Shell name used for execution.",
    },
    ApiTypeField {
        name: "command",
        lua_type: "string",
        required: true,
        doc: "Command string.",
    },
    ApiTypeField {
        name: "cwd",
        lua_type: "string",
        required: true,
        doc: "Working directory or empty string.",
    },
    ApiTypeField {
        name: "success",
        lua_type: "boolean",
        required: true,
        doc: "Whether the process exited successfully.",
    },
    ApiTypeField {
        name: "status",
        lua_type: "string",
        required: true,
        doc: "success, failed, or timed_out.",
    },
    ApiTypeField {
        name: "exit_code",
        lua_type: "integer|nil",
        required: true,
        doc: "Process exit code when available.",
    },
    ApiTypeField {
        name: "stdout",
        lua_type: "string",
        required: true,
        doc: "Captured stdout.",
    },
    ApiTypeField {
        name: "stderr",
        lua_type: "string",
        required: true,
        doc: "Captured stderr.",
    },
    ApiTypeField {
        name: "combined",
        lua_type: "string",
        required: true,
        doc: "stdout and stderr concatenated for simple terminal rendering.",
    },
    ApiTypeField {
        name: "timed_out",
        lua_type: "boolean",
        required: true,
        doc: "Whether timeout killed the process.",
    },
];

pub(super) const TYPE_SHELL_JOB_HANDLE_METHODS: &[ApiTypeMethod] = &[
    ApiTypeMethod {
        name: "cancel",
        doc: "Request cancellation and kill the running process.",
        params: &[],
        returns: &[],
    },
    ApiTypeMethod {
        name: "id",
        doc: "Return host job id.",
        params: &[],
        returns: &[ApiReturn {
            lua_type: "integer",
            doc: "Job id.",
        }],
    },
];

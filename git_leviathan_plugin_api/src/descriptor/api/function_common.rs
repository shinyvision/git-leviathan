use super::super::schema::*;

pub(super) const V1: ApiVersionInfo = ApiVersionInfo {
    major: 1,
    minor: 0,
    label: "1.0",
    compatibility: "v1",
};

pub(super) const NO_VALIDATION: ApiValidation = ApiValidation {
    args: &[],
    returns: &[],
    notes: &[],
};

pub(super) const STRING_PATH: &[ApiParam] = &[ApiParam {
    name: "path",
    lua_type: "string",
    required: true,
    doc: "Path string.",
}];

pub(super) const STRING_NAME: &[ApiParam] = &[ApiParam {
    name: "name",
    lua_type: "string",
    required: true,
    doc: "Name string.",
}];

pub(super) const BOOL_RET: &[ApiReturn] = &[ApiReturn {
    lua_type: "boolean",
    doc: "Boolean result.",
}];

pub(super) const NILABLE_STRING_ERR_RET: &[ApiReturn] = &[
    ApiReturn {
        lua_type: "string|nil",
        doc: "Value on success, nil on failure.",
    },
    ApiReturn {
        lua_type: "string|nil",
        doc: "Error message on failure.",
    },
];

pub(super) const OK_ERR_RET: &[ApiReturn] = &[
    ApiReturn {
        lua_type: "boolean",
        doc: "True on success, false on failure.",
    },
    ApiReturn {
        lua_type: "string|nil",
        doc: "Error message on failure.",
    },
];

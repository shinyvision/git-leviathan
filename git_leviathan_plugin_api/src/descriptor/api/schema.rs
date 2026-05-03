use serde::Serialize;

use crate::descriptor::widget::WidgetDescriptor;

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
    /// callback. Always `"table"` for typed events.
    pub payload_type: &'static str,
    /// Schema for the payload table fields. `&[]` for events with no
    /// payload (the host still hands the callback an empty table so
    /// the call signature stays uniform).
    pub payload_fields: &'static [ApiTypeField],
    /// Additional dispatch names for this event.
    pub aliases: &'static [&'static str],
    /// True for descriptor aliases that should not fire as canonicals.
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
    /// extension points: dynamic section prefixes valid at the region (chrome)
    /// level. Each entry like `"section:"` allows `"section:<id>"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_section_prefixes: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiRegionPane {
    pub name: &'static str,
    pub sections: Vec<&'static str>,
    /// extension points: dynamic section prefixes valid in this pane.
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

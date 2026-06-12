//! Host Lua API descriptors.

mod capabilities;
mod events;
mod functions;
mod schema;
mod types;

use crate::api_version::{EXTENSION_POINT_SCHEMA_VERSION, HOST_API_VERSION, WIDGET_SCHEMA_VERSION};
use crate::descriptor::extension_point::EXTENSION_POINTS;
use crate::descriptor::region::{RegionKind, REGIONS};
use crate::descriptor::widget::WIDGETS;

pub use capabilities::API_CAPABILITIES;
pub use events::{canonical_events, event_descriptor, API_EVENTS};
pub use functions::API_MODULES;
pub use schema::*;
pub use types::API_TYPES;

pub fn describe() -> HostApiDescription {
    HostApiDescription {
        stability_policy: STABILITY_POLICY,
        compatibility_matrix: COMPATIBILITY_MATRIX,
        semver_policy: SEMVER_POLICY,
        experimental_apis: EXPERIMENTAL_APIS,
        host_api_version: ApiVersionInfo {
            major: HOST_API_VERSION.major,
            minor: HOST_API_VERSION.minor,
            label: "1.0",
            compatibility: "v1",
        },
        modules: API_MODULES,
        events: API_EVENTS,
        types: API_TYPES,
        capabilities: API_CAPABILITIES,
        regions: describe_regions(),
        extension_points: EXTENSION_POINTS,
        widgets: WIDGETS.iter().collect(),
    }
}

pub const STABILITY_POLICY: StabilityPolicy = StabilityPolicy {
    public_api_version: "1.0",
    descriptor_version: 1,
    widget_schema_version: WIDGET_SCHEMA_VERSION,
    extension_point_schema_version: EXTENSION_POINT_SCHEMA_VERSION,
    stability: "stable",
    migration_promise:
        "UI v1 descriptors keep source-compatible additions within host 1.x; removals require a later major API.",
};

pub const COMPATIBILITY_MATRIX: &[CompatibilityMatrixRow] = &[CompatibilityMatrixRow {
    host_version: "1.0",
    plugin_api_version: "1.0",
    widget_schema: WIDGET_SCHEMA_VERSION,
    extension_points: &[
        "main_bar.left",
        "main_bar.center",
        "main_bar.right",
        "tab_bar.left",
        "tab_bar.right",
        "repository.sidebar.top",
        "repository.sidebar.bottom",
        "repository.graph.top",
        "repository.graph.bottom",
        "repository.details.top",
        "repository.details.bottom",
        "repository.diff.context_menu",
        "repository.graph.context_menu",
        "repository.graph.row_badge",
        "repository.diff.line_gutter",
        "repository.sidebar.chrome",
        "repository.graph.chrome",
        "repository.details.chrome",
        "repository.diff.chrome",
        "overlays",
        "screens",
        "commands",
        "settings.panel",
        "dock.pane",
    ],
}];

pub const SEMVER_POLICY: SemverPolicy = SemverPolicy {
    descriptor_versioning:
        "Descriptor schema changes increment descriptor_version; plugin api major changes reset the stable descriptor contract.",
    can_add: &[
        "new functions, types, optional fields, widgets, capabilities, events, and extension points in host 1.x",
        "new enum values only when older plugins can ignore them",
    ],
    requires_deprecation: &[
        "renaming public functions, fields, widgets, capabilities, events, or extension points",
        "tightening validation for an accepted public v1 shape",
    ],
    can_remove: &[
        "nothing from public UI v1 during host 1.x",
        "experimental entries only after their descriptor marks the replacement promise",
    ],
};

pub const EXPERIMENTAL_APIS: &[ExperimentalApi] = &[];

pub fn all_functions() -> impl Iterator<Item = &'static ApiFunction> {
    API_MODULES
        .iter()
        .flat_map(|module| module.functions.iter())
}

pub fn function_paths() -> Vec<&'static str> {
    all_functions().map(|function| function.path).collect()
}

pub fn has_feature(feature: &str) -> bool {
    let Some((name, version)) = feature.rsplit_once('@') else {
        return false;
    };
    let Ok(major) = version.parse::<u32>() else {
        return false;
    };
    if major != HOST_API_VERSION.major {
        return false;
    }
    let name = name.strip_prefix("leviathan.").unwrap_or(name);
    if name == "ui.context.typed" {
        return true;
    }

    API_MODULES.iter().any(|module| {
        major >= module.version.major
            && (module.name == name || module.table.strip_prefix("leviathan.") == Some(name))
    }) || all_functions().any(|function| {
        major >= since_major(function.since) && function_feature_name(function) == name
    }) || API_EVENTS.iter().any(|event| {
        major >= since_major(event.since)
            && (event.name == name || format!("event.{}", event.name) == name)
    }) || API_TYPES
        .iter()
        .any(|ty| ty.name == name || ty.name.strip_prefix("leviathan.") == Some(name))
        || API_CAPABILITIES.iter().any(|capability| {
            capability.name == name || format!("capability.{}", capability.name) == name
        })
        || REGIONS
            .iter()
            .any(|region| format!("region.{}", region.name) == name)
        || EXTENSION_POINTS
            .iter()
            .any(|point| format!("extension.{}", point.id) == name)
        || WIDGETS
            .iter()
            .any(|widget| format!("widget.{}", widget.kind) == name)
}

fn since_major(since: &str) -> u32 {
    since
        .split_once('.')
        .and_then(|(major, _)| major.parse::<u32>().ok())
        .unwrap_or(1)
}

pub fn module_names() -> Vec<&'static str> {
    API_MODULES.iter().map(|module| module.name).collect()
}

fn function_feature_name(function: &ApiFunction) -> &str {
    function
        .path
        .strip_prefix("leviathan.")
        .unwrap_or(function.path)
}

fn describe_regions() -> Vec<ApiRegion> {
    REGIONS
        .iter()
        .map(|region| match &region.kind {
            RegionKind::Chrome {
                sections,
                dynamic_section_prefixes,
            } => ApiRegion {
                name: region.name,
                kind: "chrome",
                sections: sections.to_vec(),
                panes: Vec::new(),
                dynamic_section_prefixes: dynamic_section_prefixes.to_vec(),
            },
            RegionKind::Content { panes } => ApiRegion {
                name: region.name,
                kind: "content",
                sections: Vec::new(),
                panes: panes
                    .iter()
                    .map(|pane| ApiRegionPane {
                        name: pane.name,
                        sections: pane.sections.to_vec(),
                        dynamic_section_prefixes: pane.dynamic_section_prefixes.to_vec(),
                    })
                    .collect(),
                dynamic_section_prefixes: Vec::new(),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_feature_accepts_current_v1_functions() {
        assert!(has_feature("fs.read_file@1"));
        assert!(has_feature("leviathan.fs.read_file@1"));
        assert!(has_feature("api.describe@1"));
        assert!(has_feature("ui.slot@1"));
        assert!(has_feature("ui.slot.add@1"));
        assert!(has_feature("ui.context@1"));
        assert!(has_feature("ui.contribute@1"));
        assert!(has_feature("extension.repository.graph.row_badge@1"));
        assert!(has_feature("ui.context.typed@1"));
        assert!(!has_feature("ui.slot.add@2"));
        assert!(has_feature("log@1"));
    }

    #[test]
    fn has_feature_rejects_unknown_or_wrong_major() {
        assert!(!has_feature("fs.nope@1"));
        assert!(!has_feature("fs.read_file@3"));
        assert!(!has_feature("fs.read_file"));
        assert!(!has_feature("fs.read_file@nope"));
    }

    #[test]
    fn regions_are_described_without_direct_handle_modules() {
        let modules = module_names();
        let removed_module = ["ui", "main_bar"].join(".");
        assert!(!modules.iter().any(|module| *module == removed_module));
        let regions = describe_regions();
        assert!(regions.iter().any(|region| region.name == "main_bar"));
    }

    #[test]
    fn module_types_resolve_in_api_types() {
        for module in API_MODULES {
            for name in module.types {
                assert!(
                    API_TYPES.iter().any(|ty| ty.name == *name),
                    "module `{}` lists type `{name}` with no API_TYPES entry",
                    module.name
                );
            }
        }
    }

    #[test]
    fn description_serializes() {
        let value = serde_json::to_value(describe()).unwrap();
        assert_eq!(value["host_api_version"]["label"], "1.0");
        assert_eq!(value["stability_policy"]["stability"], "stable");
        assert_eq!(value["stability_policy"]["descriptor_version"], 1);
        assert!(value["experimental_apis"].as_array().unwrap().is_empty());
        assert!(value["modules"].as_array().unwrap().len() > 5);
        assert!(value["widgets"].as_array().unwrap().len() > 5);
    }
}

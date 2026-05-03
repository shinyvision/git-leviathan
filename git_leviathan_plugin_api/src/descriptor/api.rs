//! Host Lua API descriptors.

mod capabilities;
mod events;
mod functions;
mod schema;
mod types;

use crate::api_version::HOST_API_VERSION;
use crate::descriptor::region::{RegionKind, REGIONS};
use crate::descriptor::widget::WIDGETS;

pub use capabilities::API_CAPABILITIES;
pub use events::{canonical_events, event_descriptor, API_EVENTS};
pub use functions::API_MODULES;
pub use schema::*;
pub use types::API_TYPES;

pub fn describe() -> HostApiDescription {
    HostApiDescription {
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
        widgets: WIDGETS.iter().collect(),
    }
}

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
        assert!(has_feature("ui.regions.add_slot@1"));
        assert!(!has_feature("ui.regions.add_slot@2"));
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
    fn widget_descriptors_cover_runtime_kinds() {
        let names = WIDGETS.names();
        for kind in [
            "text",
            "button",
            "row",
            "column",
            "container",
            "padding",
            "space",
            "icon",
            "image",
            "scrollable",
            "mouse_area",
            "tablist",
            "resizable_split",
        ] {
            assert!(names.contains(&kind), "missing widget descriptor {kind}");
        }
    }

    #[test]
    fn description_serializes() {
        let value = serde_json::to_value(describe()).unwrap();
        assert_eq!(value["host_api_version"]["label"], "1.0");
        assert!(value["modules"].as_array().unwrap().len() > 5);
        assert!(value["widgets"].as_array().unwrap().len() > 5);
    }
}

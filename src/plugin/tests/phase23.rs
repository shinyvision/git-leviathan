//! Phase 23 compatibility and v2 migration tests.

use crate::plugin::diagnostic::DiagnosticSeverity;
use crate::plugin::tests::harness::MockHost;

#[test]
fn v1_plugin_still_loads_and_warns_on_legacy_apis() {
    let mut host = MockHost::new();
    host.load_inline(
        "v1_compat",
        r#"
id = "v1_compat"
name = "v1 compat"
version = "0.1.0"
api_version = "1.0"
"#,
        r#"
leviathan.api.create_user_command("v1_compat.hello", function() end)
leviathan.api.create_autocmd({ "FetchStart" }, { callback = function() end })
leviathan.ui.main_bar.add{
    id = "v1_compat.slot",
    section = "left",
    priority = 10,
    widget = { kind = "text", value = "v1" },
}
"#,
    )
    .expect("v1 plugin loads");

    assert!(host.has_slot("v1_compat", "main_bar", "left", "v1_compat.slot"));
    let deprecations = host.diagnostics().by_code("api.deprecated");
    assert!(deprecations
        .iter()
        .any(|d| d.plugin_id.as_str() == "v1_compat"
            && d.severity == DiagnosticSeverity::Warning
            && d.message.contains("leviathan.api.create_user_command")));
    assert!(deprecations
        .iter()
        .any(|d| d.plugin_id.as_str() == "v1_compat"
            && d.message.contains("leviathan.api.create_autocmd")));
    assert!(deprecations
        .iter()
        .any(|d| d.plugin_id.as_str() == "v1_compat"
            && d.message.contains("leviathan.ui.main_bar.add")));
}

#[test]
fn v2_plugin_uses_descriptor_region_api_without_deprecation() {
    let mut host = MockHost::new();
    host.load_inline(
        "v2_regions",
        r#"
id = "v2_regions"
name = "v2 regions"
version = "0.1.0"
api_version = "2.0"
"#,
        r#"
assert(leviathan.has("ui.regions.add_slot@2"))
assert(leviathan.has("autocmd.create@2"))
leviathan.autocmd.create("FetchStart", { callback = function() end })
leviathan.ui.regions.add_slot{
    region = "main_bar",
    id = "v2_regions.slot",
    section = "right",
    priority = 20,
    widget = { kind = "text", value = "v2" },
}
"#,
    )
    .expect("v2 plugin loads");

    assert!(host.has_slot("v2_regions", "main_bar", "right", "v2_regions.slot"));
    assert!(
        host.diagnostics().by_code("api.deprecated").is_empty(),
        "v2 descriptor APIs should not emit deprecations"
    );
}

#[test]
fn mixed_v1_and_v2_plugins_coexist() {
    let mut host = MockHost::new();
    host.load_inline_set(&[
        (
            "mixed_v1",
            r#"
id = "mixed_v1"
name = "mixed v1"
version = "0.1.0"
api_version = "1.0"
"#,
            r#"
leviathan.ui.main_bar.add{
    id = "mixed.v1",
    section = "left",
    priority = 1,
    widget = { kind = "text", value = "v1" },
}
"#,
        ),
        (
            "mixed_v2",
            r#"
id = "mixed_v2"
name = "mixed v2"
version = "0.1.0"
api_version = "2.0"
"#,
            r#"
leviathan.ui.regions.add_slot{
    region = "main_bar",
    id = "mixed.v2",
    section = "right",
    priority = 2,
    widget = { kind = "text", value = "v2" },
}
"#,
        ),
    ])
    .expect("fixtures written");

    assert!(host.has_slot("mixed_v1", "main_bar", "left", "mixed.v1"));
    assert!(host.has_slot("mixed_v2", "main_bar", "right", "mixed.v2"));
    let snap = host.introspect();
    assert!(snap
        .plugins
        .iter()
        .any(|p| p.id == "mixed_v1" && p.api_version == "1.0"));
    assert!(snap
        .plugins
        .iter()
        .any(|p| p.id == "mixed_v2" && p.api_version == "2.0"));
}

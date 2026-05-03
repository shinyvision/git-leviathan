//! Phase 23 v1 API boundary tests.

use crate::plugin::tests::harness::MockHost;

#[test]
fn v1_plugin_uses_descriptor_region_api() {
    let mut host = MockHost::new();
    host.load_inline(
        "v1_regions",
        r#"
id = "v1_regions"
name = "v1 regions"
version = "0.1.0"
api_version = "1.0"
"#,
        r#"
assert(leviathan.has("ui.regions.add_slot@1"))
assert(leviathan.has("autocmd.create@1"))
assert(not leviathan.has("ui.regions.add_slot@2"))
leviathan.autocmd.create("FetchStarted", { callback = function() end })
leviathan.ui.regions.add_slot{
    region = "main_bar",
    id = "v1_regions.slot",
    section = "right",
    priority = 20,
    widget = { kind = "text", value = "v1" },
}
"#,
    )
    .expect("v1 plugin loads");

    assert!(host.has_slot("v1_regions", "main_bar", "right", "v1_regions.slot"));
}

#[test]
fn multiple_v1_plugins_coexist() {
    let mut host = MockHost::new();
    host.load_inline_set(&[
        (
            "left",
            r#"
id = "left"
name = "left"
version = "0.1.0"
api_version = "1.0"
"#,
            r#"
leviathan.ui.regions.add_slot{
    region = "main_bar",
    id = "mixed.left",
    section = "left",
    priority = 1,
    widget = { kind = "text", value = "left" },
}
"#,
        ),
        (
            "right",
            r#"
id = "right"
name = "right"
version = "0.1.0"
api_version = "1.0"
"#,
            r#"
leviathan.ui.regions.add_slot{
    region = "main_bar",
    id = "mixed.right",
    section = "right",
    priority = 2,
    widget = { kind = "text", value = "right" },
}
"#,
        ),
    ])
    .expect("fixtures written");

    assert!(host.has_slot("left", "main_bar", "left", "mixed.left"));
    assert!(host.has_slot("right", "main_bar", "right", "mixed.right"));
    let snap = host.introspect();
    assert!(snap.plugins.iter().all(|p| p.api_version == "1.0"));
}

//! API boundary v1 API boundary tests.

use crate::plugin::tests::harness::MockHost;

#[test]
fn v1_plugin_uses_slot_api() {
    let mut host = MockHost::new();
    host.load_inline(
        "v1_slots",
        r#"
id = "v1_slots"
name = "v1 slots"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:region:main_bar"]
"#,
        r#"
assert(leviathan.has("ui.slot.add@1"))
assert(leviathan.has("autocmd.create@1"))
assert(not leviathan.has("ui.slot.add@2"))
leviathan.autocmd.create("FetchStarted", { callback = function() end })
leviathan.ui.slot.add{
    region = "main_bar",
    id = "v1_slots.slot",
    section = "right",
    priority = 20,
    widget = { kind = "text", value = "v1" },
}
"#,
    )
    .expect("v1 plugin loads");

    assert!(host.has_slot("v1_slots", "main_bar", "right", "v1_slots.slot"));
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
capabilities = ["ui:region:main_bar"]
"#,
            r#"
leviathan.ui.slot.add{
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
capabilities = ["ui:region:main_bar"]
"#,
            r#"
leviathan.ui.slot.add{
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

use crate::plugin::capability_grants::{DecidedBy, Decision};
use crate::plugin::tests::harness::MockHost;

const MANIFEST: &str = r#"
id = "certified"
name = "Certified"
version = "0.1.0"
api_version = "1.0"
capabilities = ["ui:region:main_bar"]
"#;

const INIT: &str = r#"
local handle, err = leviathan.ui.slot.add{
    region = "main_bar",
    section = "right",
    id = "plugin.certified.badge",
    priority = 40,
    widget = { kind = "text", value = "certified" },
}
assert(handle, err)
"#;

#[test]
fn certification_flow_loads_renders_reloads_unloads_and_audits_capabilities() {
    let mut host = MockHost::new();
    host.load_inline("certified", MANIFEST, INIT).expect("load");

    assert!(host.has_slot("certified", "main_bar", "right", "plugin.certified.badge"));
    assert!(host
        .introspect()
        .plugins
        .iter()
        .any(|plugin| plugin.id == "certified" && plugin.api_version == "1.0"));

    let grant = host
        .host()
        .grant_store()
        .lookup("certified", "0.1.0", "ui:region:main_bar")
        .expect("grant row");
    assert_eq!(grant.decision, Decision::Allow);
    assert_eq!(grant.decided_by, DecidedBy::Default);

    host.reload_plugin("certified").expect("reload");
    assert!(host.has_slot("certified", "main_bar", "right", "plugin.certified.badge"));

    host.unload_plugin("certified").expect("unload");
    assert!(!host.has_slot("certified", "main_bar", "right", "plugin.certified.badge"));
    assert!(host
        .introspect()
        .plugins
        .iter()
        .all(|plugin| plugin.id != "certified"));
}

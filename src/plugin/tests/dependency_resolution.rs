//! dependency resolution acceptance tests.

use crate::plugin::tests::harness::MockHost;

fn manifest(id: &str, version: &str, extra: &str) -> String {
    format!(
        r#"
id = "{id}"
name = "{id}"
version = "{version}"
api_version = "1.0"
{extra}
"#
    )
}

#[test]
fn resolves_load_order_for_linear_chain() {
    // c -> b -> a; resolver must load a, then b, then c.
    let mut host = MockHost::new();
    host.load_inline_set(&[
        ("a", &manifest("a", "1.0.0", ""), r#"_G.loaded = "a""#),
        (
            "b",
            &manifest("b", "1.0.0", "[dependencies]\na = \">=1.0\""),
            r#"_G.loaded = "b""#,
        ),
        (
            "c",
            &manifest("c", "1.0.0", "[dependencies]\nb = \">=1.0\""),
            r#"_G.loaded = "c""#,
        ),
    ])
    .expect("resolve and load");

    let snap = host.introspect();
    let ids: Vec<&str> = snap.plugins.iter().map(|p| p.id.as_str()).collect();
    assert!(ids.contains(&"a"));
    assert!(ids.contains(&"b"));
    assert!(ids.contains(&"c"));

    let edges = &snap.dependency_graph;
    assert_eq!(edges.len(), 2, "expected 2 dependency edges, got {edges:?}");
    let b_edge = edges
        .iter()
        .find(|e| e.consumer_plugin_id == "b")
        .expect("b->a edge");
    assert_eq!(b_edge.dependency_id, "a");
    assert_eq!(b_edge.kind, "required");
    assert_eq!(b_edge.status, "resolved");
}

#[test]
fn optional_missing_dependency_loads_consumer() {
    let mut host = MockHost::new();
    host.load_inline_set(&[(
        "consumer",
        &manifest(
            "consumer",
            "1.0.0",
            "[optional_dependencies]\nhelper = \">=1.0\"",
        ),
        r#"_G.loaded = "consumer""#,
    )])
    .expect("optional missing should not block");

    let snap = host.introspect();
    assert!(snap.plugins.iter().any(|p| p.id == "consumer"));
    assert!(host
        .diagnostics()
        .by_code("dependency.optional_missing")
        .iter()
        .any(|d| d.context["dependency"] == "helper"));

    let edge = snap
        .dependency_graph
        .iter()
        .find(|e| e.consumer_plugin_id == "consumer" && e.dependency_id == "helper")
        .expect("optional edge");
    assert_eq!(edge.kind, "optional");
    assert_eq!(edge.status, "missing");
}

#[test]
fn required_missing_dependency_blocks_load() {
    let mut host = MockHost::new();
    host.load_inline_set(&[(
        "consumer",
        &manifest(
            "consumer",
            "1.0.0",
            "[dependencies]\nmissing_dep = \">=1.0\"",
        ),
        r#"_G.loaded = "consumer""#,
    )])
    .expect("resolve_and_load swallows blocked plugins");

    let snap = host.introspect();
    assert!(
        !snap.plugins.iter().any(|p| p.id == "consumer"),
        "blocked plugin must not appear in the loaded set"
    );
    assert!(host
        .diagnostics()
        .by_code("dependency.missing_required")
        .iter()
        .any(|d| d.plugin_id.as_str() == "consumer"));
}

#[test]
fn incompatible_version_emits_conflict_diagnostic() {
    let mut host = MockHost::new();
    host.load_inline_set(&[
        ("dep", &manifest("dep", "1.0.0", ""), r#"-- v1"#),
        (
            "consumer",
            &manifest("consumer", "1.0.0", "[dependencies]\ndep = \">=2.0\""),
            r#"-- needs newer dependency"#,
        ),
    ])
    .expect("resolve_and_load should not error out on conflicts");

    let diags = host.diagnostics().by_code("dependency.conflict");
    assert!(
        diags
            .iter()
            .any(|d| d.plugin_id.as_str() == "consumer" && d.context["actual_version"] == "1.0.0"),
        "expected conflict diagnostic, got: {:?}",
        diags.iter().map(|d| d.message.clone()).collect::<Vec<_>>()
    );
    let snap = host.introspect();
    assert!(!snap.plugins.iter().any(|p| p.id == "consumer"));
    assert!(snap.plugins.iter().any(|p| p.id == "dep"));
}

#[test]
fn cycle_detection_blocks_all_in_cycle() {
    let mut host = MockHost::new();
    host.load_inline_set(&[
        (
            "a",
            &manifest("a", "1.0.0", "[dependencies]\nb = \">=1.0\""),
            r#"-- a"#,
        ),
        (
            "b",
            &manifest("b", "1.0.0", "[dependencies]\na = \">=1.0\""),
            r#"-- b"#,
        ),
    ])
    .expect("resolve_and_load swallows cycles");

    let snap = host.introspect();
    assert!(
        !snap.plugins.iter().any(|p| p.id == "a" || p.id == "b"),
        "neither plugin in cycle should load"
    );
    let cycle_diags = host.diagnostics().by_code("dependency.cycle");
    assert!(
        cycle_diags.iter().any(|d| d.plugin_id.as_str() == "a")
            && cycle_diags.iter().any(|d| d.plugin_id.as_str() == "b"),
        "cycle diagnostic must name both members"
    );
}

#[test]
fn dependency_devtools_graph_shape() {
    let mut host = MockHost::new();
    host.load_inline_set(&[
        ("a", &manifest("a", "1.0.0", ""), r#"-- a"#),
        (
            "b",
            &manifest(
                "b",
                "1.0.0",
                "[dependencies]\na = \">=1.0\"\n\n[optional_dependencies]\nz = \">=1.0\"",
            ),
            r#"-- b"#,
        ),
    ])
    .expect("load");

    let snap = host.introspect();
    let edges = &snap.dependency_graph;
    let req = edges
        .iter()
        .find(|e| e.consumer_plugin_id == "b" && e.dependency_id == "a")
        .expect("required edge");
    assert_eq!(req.kind, "required");
    assert_eq!(req.status, "resolved");
    assert_eq!(req.resolved_version.as_deref(), Some("1.0.0"));
    let opt = edges
        .iter()
        .find(|e| e.consumer_plugin_id == "b" && e.dependency_id == "z")
        .expect("optional edge");
    assert_eq!(opt.kind, "optional");
    assert_eq!(opt.status, "missing");
    assert!(opt.resolved_version.is_none());
}

//! Phase 14 acceptance tests: inter-plugin services.

use crate::plugin::tests::harness::MockHost;

fn manifest(id: &str, extra: &str) -> String {
    format!(
        r#"
id = "{id}"
name = "{id}"
version = "0.1.0"
api_version = "1.0"
{extra}
"#
    )
}

#[test]
fn service_version_matching_uses_new_name_version_api() {
    let mut host = MockHost::new();
    host.load_inline(
        "tracker",
        &manifest(
            "tracker",
            r#"provides_services = ["issue_tracker@1", "issue_tracker@2"]"#,
        ),
        r#"
        leviathan.services.register("issue_tracker", 1, {
          lookup = function(commit) return "v1:" .. commit end,
        })
        leviathan.services.register("issue_tracker", 2, {
          lookup = function(commit) return "service2:" .. commit end,
        })
        "#,
    )
    .expect("provider loads");

    host.load_inline(
        "consumer",
        &manifest("consumer", r#"consumes_services = ["issue_tracker@2"]"#),
        r#"
        local tracker = leviathan.services.get("issue_tracker", 2)
        _G.lookup = tracker.lookup("abc123")
        "#,
    )
    .expect("consumer loads");

    assert_eq!(
        host.read_global_string("consumer", "lookup").as_deref(),
        Some("service2:abc123")
    );
}

#[test]
fn required_missing_service_blocks_load() {
    let mut host = MockHost::new();
    let result = host.load_inline(
        "consumer",
        &manifest("consumer", r#"consumes_services = ["issue_tracker@1"]"#),
        "-- required dependency is validated before init work",
    );

    let err = result
        .expect_err("required missing service must block load")
        .to_string();
    assert!(err.contains("issue_tracker@1"), "got: {err}");
    assert!(
        host.diagnostics()
            .by_code("services.required_missing")
            .iter()
            .any(|diag| diag.context["service"] == "issue_tracker@1"),
        "missing required-service diagnostic"
    );
}

#[test]
fn optional_missing_service_returns_nil_and_is_visible_in_devtools() {
    let mut host = MockHost::new();
    host.load_inline(
        "consumer",
        &manifest(
            "consumer",
            r#"consumes_services = [{ service = "issue_tracker@1", optional = true }]"#,
        ),
        r#"
        local tracker = leviathan.services.get("issue_tracker", 1)
        _G.absent = tracker == nil and 1 or 0
        "#,
    )
    .expect("optional missing service should not block load");

    assert_eq!(host.read_global_i64("consumer", "absent"), Some(1));
    let snap = host.introspect();
    let edge = snap
        .service_graph
        .iter()
        .find(|edge| edge.consumer_plugin_id == "consumer" && edge.service_key == "issue_tracker@1")
        .expect("optional graph edge");
    assert_eq!(edge.status, "missing_optional");
    assert!(!edge.required);
    assert!(edge.provider_plugin_id.is_none());
}

#[test]
fn provider_error_isolated_and_traced() {
    let mut host = MockHost::new();
    host.load_inline(
        "provider",
        &manifest("provider", r#"provides_services = ["math@1"]"#),
        r#"
        leviathan.services.register("math", 1, {
          fail = function() error("boom") end,
          ok = function() return 7 end,
        })
        "#,
    )
    .expect("provider loads");

    host.load_inline(
        "consumer",
        &manifest("consumer", r#"consumes_services = ["math@1"]"#),
        r#"
        local math = leviathan.services.get("math@1")
        local ok, err = pcall(function() return math.fail() end)
        _G.failed = ok and 0 or 1
        _G.err = tostring(err)
        _G.after = math.ok()
        "#,
    )
    .expect("consumer handles provider error");

    assert_eq!(host.read_global_i64("consumer", "failed"), Some(1));
    assert_eq!(host.read_global_i64("consumer", "after"), Some(7));
    assert!(host
        .introspect()
        .services
        .iter()
        .any(|service| service.key == "math@1" && service.publisher_plugin_id == "provider"));

    let snap = host.introspect();
    assert!(snap.service_call_traces.iter().any(|trace| {
        trace.caller_plugin_id == "consumer"
            && trace.provider_plugin_id == "provider"
            && trace.service_key == "math@1"
            && trace.method == "fail"
            && !trace.success
            && trace.error.as_deref().unwrap_or("").contains("boom")
    }));
    assert!(snap.service_call_traces.iter().any(|trace| {
        trace.caller_plugin_id == "consumer"
            && trace.provider_plugin_id == "provider"
            && trace.service_key == "math@1"
            && trace.method == "ok"
            && trace.success
    }));
}

#[test]
fn service_graph_records_connected_consumers_and_traces() {
    let mut host = MockHost::new();
    host.load_inline(
        "provider",
        &manifest("provider", r#"provides_services = ["math@1"]"#),
        r#"
        leviathan.services.register("math@1", {
          add = function(a, b) return a + b end,
        })
        "#,
    )
    .expect("provider loads");
    host.load_inline(
        "consumer",
        &manifest("consumer", r#"consumes_services = ["math@1"]"#),
        r#"
        local math = leviathan.services.get("math", 1)
        _G.sum = math.add(2, 5)
        "#,
    )
    .expect("consumer loads");

    assert_eq!(host.read_global_i64("consumer", "sum"), Some(7));
    let snap = host.introspect();
    let edge = snap
        .service_graph
        .iter()
        .find(|edge| edge.consumer_plugin_id == "consumer" && edge.service_key == "math@1")
        .expect("service graph edge");
    assert_eq!(edge.provider_plugin_id.as_deref(), Some("provider"));
    assert_eq!(edge.status, "connected");
    assert!(edge.required);
    assert!(snap.service_call_traces.iter().any(|trace| {
        trace.caller_plugin_id == "consumer"
            && trace.provider_plugin_id == "provider"
            && trace.method == "add"
            && trace.success
            && trace.duration_ms <= trace.timestamp_unix_ms
    }));
}

#[test]
fn provider_work_observes_caller_capabilities() {
    let mut host = MockHost::new();
    host.load_inline(
        "provider",
        &manifest(
            "provider",
            r#"
capabilities = ["env"]
provides_services = ["env_reader@1"]
"#,
        ),
        r#"
        leviathan.services.register("env_reader", 1, {
          read_path = function()
            local value, err = leviathan.env.get("PATH")
            return value or err or "missing"
          end,
        })
        "#,
    )
    .expect("provider loads");

    host.load_inline(
        "consumer",
        &manifest("consumer", r#"consumes_services = ["env_reader@1"]"#),
        r#"
        local reader = leviathan.services.get("env_reader", 1)
        local ok, result = pcall(function() return reader.read_path() end)
        _G.allowed = ok and 1 or 0
        _G.err = tostring(result)
        "#,
    )
    .expect("consumer catches denied provider work");

    assert_eq!(host.read_global_i64("consumer", "allowed"), Some(0));
    assert!(
        host.read_global_string("consumer", "err")
            .unwrap_or_default()
            .contains("env"),
        "caller denial should mention env"
    );
    assert!(
        host.diagnostics()
            .by_code("capability.denied")
            .iter()
            .any(
                |diag| diag.plugin_id.as_str() == "consumer" && diag.context["capability"] == "env"
            ),
        "missing caller capability diagnostic"
    );

    host.load_inline(
        "consumer_with_env",
        &manifest(
            "consumer_with_env",
            r#"
capabilities = ["env"]
consumes_services = ["env_reader@1"]
"#,
        ),
        r#"
        local reader = leviathan.services.get("env_reader@1")
        local ok, _result = pcall(function() return reader.read_path() end)
        _G.allowed = ok and 1 or 0
        "#,
    )
    .expect("consumer with env can use provider work");

    assert_eq!(
        host.read_global_i64("consumer_with_env", "allowed"),
        Some(1)
    );
}

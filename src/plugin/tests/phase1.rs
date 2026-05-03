use crate::plugin::tests::harness::MockHost;

fn manifest(id: &str) -> String {
    format!(
        r#"
id = "{id}"
name = "{id}"
version = "0.1.0"
api_version = "1.0"
provides_services = ["math@1"]
"#
    )
}

#[test]
fn ledger_introspection_lists_phase_one_resources() {
    let mut host = MockHost::new();
    host.load_inline(
        "ledger",
        &manifest("ledger"),
        r#"
        local n = 0

        leviathan.autocmd.create({ "FetchStarted" }, {
            callback = function() n = n + 1 end,
        })

        leviathan.command.create("hello", {
            run = function()
                _G.command_ran = 1
            end,
        })

        leviathan.api.schedule(function() _G.scheduled = 1 end)
        leviathan.api.defer_fn(1000, function() _G.deferred = 1 end)

        leviathan.services.register("math@1", {
            add = function(a, b) return a + b end,
        })

        leviathan.health.register(function(ctx)
            ctx:ok("ok")
        end)

        leviathan.ui.regions.add_slot{ region = "main_bar",
            id = "ledger.slot",
            section = "left",
            priority = 10,
            widget = function()
                return { kind = "text", value = tostring(n) }
            end,
            on_click = function() end,
        }

        leviathan.ui.register_screen{
            id = "screen",
            init = function() return { n = 1 } end,
            view = function(s) return { kind = "text", value = tostring(s.n) } end,
            update = function(s) return s end,
        }
        "#,
    )
    .expect("load");
    host.open_screen("ledger", "screen");

    let snap = host.introspect();
    let resources: Vec<_> = snap
        .resources
        .iter()
        .filter(|resource| resource.plugin_id == "ledger")
        .collect();
    assert!(resources
        .iter()
        .any(|resource| resource.kind == "slot" && resource.handle == "main_bar:left:ledger.slot"));
    assert!(resources.iter().any(|resource| resource.kind == "screen"));
    assert!(resources.iter().any(|resource| resource.kind == "command"));
    assert!(resources.iter().any(|resource| resource.kind == "autocmd"));
    assert!(resources
        .iter()
        .any(|resource| resource.kind == "service_registration"));
    assert!(resources
        .iter()
        .any(|resource| resource.kind == "dynamic_widget_cache"));
    assert!(resources
        .iter()
        .any(|resource| resource.kind == "persisted_screen_state"));
    assert!(resources
        .iter()
        .any(|resource| resource.kind == "lua_registry_key"));
    assert!(resources.iter().all(|resource| resource.generation_id == 1));
    assert!(resources
        .iter()
        .any(|resource| resource.source_location.is_some()));
    assert!(resources
        .iter()
        .all(|resource| resource.created_at_unix_ms > 0));
}

#[test]
fn unload_removes_all_ledger_resources_and_host_state() {
    let mut host = MockHost::new();
    host.load_inline(
        "unload",
        &manifest("unload"),
        r#"
        leviathan.services.register("math@1", {
            add = function(a, b) return a + b end,
        })
        leviathan.autocmd.create({ "FetchStarted" }, {
            callback = function() end,
        })
        leviathan.ui.regions.add_slot{ region = "main_bar",
            id = "unload.slot",
            section = "left",
            priority = 10,
            widget = { kind = "text", value = "x" },
            on_click = function() error("click cleanup must not run") end,
        }
        "#,
    )
    .expect("load");

    let reload_result = host.reload_with_str("unload", &manifest("unload"), "not valid lua >>>");
    assert!(reload_result.is_err());
    assert!(host.host().last_reload_error("unload").is_some());

    host.host_mut().unload_plugin("unload").expect("unload");

    let snap = host.introspect();
    assert!(snap.plugins.iter().all(|plugin| plugin.id != "unload"));
    assert!(snap
        .resources
        .iter()
        .all(|resource| resource.plugin_id != "unload"));
    assert!(snap
        .slots
        .iter()
        .all(|slot| slot.owner_plugin_id != "unload"));
    assert!(snap
        .services
        .iter()
        .all(|service| service.publisher_plugin_id != "unload"));
    assert!(host.host().last_reload_error("unload").is_none());
}

#[test]
fn failed_reload_restores_parked_service_handles() {
    let mut host = MockHost::new();
    host.load_inline(
        "publisher",
        &manifest("publisher"),
        r#"
        leviathan.services.register("math@1", {
            add = function(a, b) return a + b end,
        })
        "#,
    )
    .expect("load publisher");

    let reload_result = host.reload_with_str("publisher", &manifest("publisher"), "not lua >>>");
    assert!(reload_result.is_err());

    host.load_inline(
        "consumer",
        r#"
        id = "consumer"
        name = "consumer"
        version = "0.1.0"
        api_version = "1.0"
        consumes_services = ["math@1"]
        "#,
        r#"
        local math = leviathan.services.get("math@1")
        _G.sum = math.add(20, 22)
        "#,
    )
    .expect("consumer can still call parked service");

    assert_eq!(host.read_global_i64("consumer", "sum"), Some(42));
}

#[test]
fn unload_clears_split_state_and_pending_callbacks_without_running_them() {
    let mut host = MockHost::new();
    host.load_inline(
        "pending",
        &manifest("pending"),
        r#"
        leviathan.api.schedule(function() error("scheduled callback should not run") end)
        leviathan.api.defer_fn(0, function() error("timer callback should not run") end)
        leviathan.command.create("later", {
            run = function()
                coroutine.yield()
                error("coroutine should not resume")
            end,
        })
        "#,
    )
    .expect("load");
    host.invoke_user_command("pending", "later")
        .expect("park coroutine");

    host.host_mut().split_drag_begin(
        "pending:screen:split",
        0,
        2,
        false,
        vec![(80.0, 4000.0), (80.0, 4000.0)],
    );
    assert!(!host.host().split_sizes().is_empty());
    assert!(host.host().is_dragging_split());

    host.host_mut().unload_plugin("pending").expect("unload");
    host.tick();

    let snap = host.introspect();
    assert!(snap
        .resources
        .iter()
        .all(|resource| resource.plugin_id != "pending"));
    assert!(host.host().split_sizes().is_empty());
    assert!(!host.host().is_dragging_split());
}

#[test]
fn reload_cleanup_survives_failing_lua_serialize_callback() {
    let mut host = MockHost::new();
    host.load_inline(
        "reload_cleanup",
        &manifest("reload_cleanup"),
        r#"
        leviathan.services.register("math@1", { id = function() return 1 end })
        leviathan.ui.regions.add_slot{ region = "main_bar",
            id = "old.slot",
            section = "left",
            priority = 10,
            widget = { kind = "text", value = "old" },
        }
        leviathan.ui.register_screen{
            id = "screen",
            init = function() return { n = 1 } end,
            view = function(s) return { kind = "text", value = tostring(s.n) } end,
            update = function(s) return s end,
            serialize = function() error("serialize failed") end,
            deserialize = function(t) return t end,
        }
        "#,
    )
    .expect("load");
    host.open_screen("reload_cleanup", "screen");

    host.reload_with_str(
        "reload_cleanup",
        &manifest("reload_cleanup"),
        r#"
        leviathan.services.register("math@1", { id = function() return 2 end })
        leviathan.ui.regions.add_slot{ region = "main_bar",
            id = "new.slot",
            section = "left",
            priority = 10,
            widget = { kind = "text", value = "new" },
        }
        "#,
    )
    .expect("reload must complete despite serialize failure");

    let snap = host.introspect();
    assert!(snap.slots.iter().any(|slot| slot.id == "new.slot"));
    assert!(snap.slots.iter().all(|slot| slot.id != "old.slot"));
    assert!(snap
        .resources
        .iter()
        .filter(|resource| resource.plugin_id == "reload_cleanup")
        .all(|resource| resource.generation_id == 2));
    assert!(snap
        .resources
        .iter()
        .all(|resource| resource.handle != "main_bar:left:old.slot"));
}

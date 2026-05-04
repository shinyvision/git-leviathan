use crate::plugin::slots::Container;
use crate::plugin::tests::harness::MockHost;
use crate::widgets::chrome::main_bar::{builtins as main_bar_builtins, MainBarRegistry};

fn replay_main_bar(host: &MockHost) -> MainBarRegistry {
    let mut registry = MainBarRegistry::new();
    main_bar_builtins::register_all(&mut registry);
    host.host().apply_main_bar_slots(&mut registry);
    registry
}

fn main_bar_slot_priority(registry: &MainBarRegistry, id: &str) -> Option<i32> {
    for section in ["left", "center", "right"] {
        if let Some(slot) = registry
            .iter_container(Container::Section(section.to_string()))
            .find(|slot| slot.id == id)
        {
            return Some(slot.priority);
        }
    }
    None
}

fn builtin_main_bar_slot_priority(id: &str) -> Option<i32> {
    let mut registry = MainBarRegistry::new();
    main_bar_builtins::register_all(&mut registry);
    main_bar_slot_priority(&registry, id)
}

fn slot_owner(host: &MockHost, region: &str, container: &str, id: &str) -> Option<String> {
    host.introspect()
        .slots
        .into_iter()
        .find(|slot| slot.region == region && slot.container == container && slot.id == id)
        .map(|slot| slot.owner_plugin_id)
}

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

fn bare_manifest(id: &str) -> String {
    format!(
        r#"
id = "{id}"
name = "{id}"
version = "0.1.0"
api_version = "1.0"
"#
    )
}

#[test]
fn ledger_introspection_lists_resource_lifecycle_entries() {
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
fn unload_removes_owned_remove_op_so_peer_slot_replays() {
    let mut host = MockHost::new();
    host.load_inline(
        "peer",
        &bare_manifest("peer"),
        r#"
        leviathan.ui.regions.add_slot{ region = "main_bar",
            id = "peer.slot",
            section = "left",
            priority = 10,
            widget = { kind = "text", value = "peer" },
        }
        "#,
    )
    .expect("load peer");

    host.load_inline(
        "remover",
        &bare_manifest("remover"),
        r#"
        leviathan.ui.regions.remove_slot{
            region = "main_bar",
            section = "left",
            id = "peer.slot",
        }
        "#,
    )
    .expect("load remover");

    assert!(!host.has_slot("peer", "main_bar", "left", "peer.slot"));
    let mut hidden = MainBarRegistry::new();
    host.host().apply_main_bar_slots(&mut hidden);
    assert!(!hidden.contains("peer.slot"));

    host.host_mut().unload_plugin("remover").expect("unload");

    assert!(host.has_slot("peer", "main_bar", "left", "peer.slot"));
    let mut replayed = MainBarRegistry::new();
    host.host().apply_main_bar_slots(&mut replayed);
    assert!(replayed.contains("peer.slot"));
}

#[test]
fn replace_over_replace_restores_previous_replacement_then_builtin() {
    let mut host = MockHost::new();
    let builtin_fetch_priority =
        builtin_main_bar_slot_priority("builtin.fetch_indicator").expect("fetch indicator builtin");

    host.load_inline(
        "replace_a",
        &bare_manifest("replace_a"),
        r#"
        leviathan.ui.regions.replace_slot(
            { region = "main_bar", section = "left", id = "builtin.fetch_indicator" },
            {
                region = "main_bar",
                section = "left",
                id = "builtin.fetch_indicator",
                priority = 401,
                widget = { kind = "text", value = "A" },
            }
        )
        "#,
    )
    .expect("load replace_a");
    host.load_inline(
        "replace_b",
        &bare_manifest("replace_b"),
        r#"
        leviathan.ui.regions.replace_slot(
            { region = "main_bar", section = "left", id = "builtin.fetch_indicator" },
            {
                region = "main_bar",
                section = "left",
                id = "builtin.fetch_indicator",
                priority = 402,
                widget = { kind = "text", value = "B" },
            }
        )
        "#,
    )
    .expect("load replace_b");

    let replayed = replay_main_bar(&host);
    assert_eq!(
        main_bar_slot_priority(&replayed, "builtin.fetch_indicator"),
        Some(402)
    );
    assert_eq!(
        slot_owner(&host, "main_bar", "left", "builtin.fetch_indicator").as_deref(),
        Some("replace_b")
    );

    assert!(host.host_mut().disable_plugin("replace_b"));

    let replayed = replay_main_bar(&host);
    assert_eq!(
        main_bar_slot_priority(&replayed, "builtin.fetch_indicator"),
        Some(401)
    );
    assert_eq!(
        slot_owner(&host, "main_bar", "left", "builtin.fetch_indicator").as_deref(),
        Some("replace_a")
    );

    host.unload_plugin("replace_a").expect("unload replace_a");

    let replayed = replay_main_bar(&host);
    assert_eq!(
        main_bar_slot_priority(&replayed, "builtin.fetch_indicator"),
        Some(builtin_fetch_priority)
    );
    assert_eq!(
        slot_owner(&host, "main_bar", "left", "builtin.fetch_indicator"),
        None
    );
}

#[test]
fn remove_over_add_restores_peer_add_then_removes_with_owner() {
    let mut host = MockHost::new();
    host.load_inline(
        "add_owner",
        &bare_manifest("add_owner"),
        r#"
        leviathan.ui.regions.add_slot{
            region = "main_bar",
            section = "left",
            id = "plugin.a.slot",
            priority = 411,
            widget = { kind = "text", value = "A" },
        }
        "#,
    )
    .expect("load add_owner");
    host.load_inline(
        "remove_owner",
        &bare_manifest("remove_owner"),
        r#"
        leviathan.ui.regions.remove_slot{
            region = "main_bar",
            section = "left",
            id = "plugin.a.slot",
        }
        "#,
    )
    .expect("load remove_owner");

    let replayed = replay_main_bar(&host);
    assert!(!replayed.contains("plugin.a.slot"));
    assert_eq!(slot_owner(&host, "main_bar", "left", "plugin.a.slot"), None);

    assert!(host.host_mut().disable_plugin("remove_owner"));

    let replayed = replay_main_bar(&host);
    assert_eq!(
        main_bar_slot_priority(&replayed, "plugin.a.slot"),
        Some(411)
    );
    assert_eq!(
        slot_owner(&host, "main_bar", "left", "plugin.a.slot").as_deref(),
        Some("add_owner")
    );

    host.unload_plugin("add_owner").expect("unload add_owner");

    let replayed = replay_main_bar(&host);
    assert!(!replayed.contains("plugin.a.slot"));
    assert_eq!(slot_owner(&host, "main_bar", "left", "plugin.a.slot"), None);
}

#[test]
fn remove_builtin_reappears_after_remover_unloads() {
    let mut host = MockHost::new();
    let builtin_fetch_priority =
        builtin_main_bar_slot_priority("builtin.fetch_indicator").expect("fetch indicator builtin");

    host.load_inline(
        "builtin_remover",
        &bare_manifest("builtin_remover"),
        r#"
        leviathan.ui.regions.remove_slot{
            region = "main_bar",
            section = "left",
            id = "builtin.fetch_indicator",
        }
        "#,
    )
    .expect("load builtin_remover");

    let replayed = replay_main_bar(&host);
    assert!(!replayed.contains("builtin.fetch_indicator"));

    host.unload_plugin("builtin_remover")
        .expect("unload builtin_remover");

    let replayed = replay_main_bar(&host);
    assert_eq!(
        main_bar_slot_priority(&replayed, "builtin.fetch_indicator"),
        Some(builtin_fetch_priority)
    );
}

#[test]
fn replace_over_plugin_add_restores_add_when_replacer_unloads() {
    let mut host = MockHost::new();
    host.load_inline(
        "plugin_add",
        &bare_manifest("plugin_add"),
        r#"
        leviathan.ui.regions.add_slot{
            region = "main_bar",
            section = "left",
            id = "plugin.a.slot",
            priority = 421,
            widget = { kind = "text", value = "A" },
        }
        "#,
    )
    .expect("load plugin_add");
    host.load_inline(
        "plugin_replace",
        &bare_manifest("plugin_replace"),
        r#"
        leviathan.ui.regions.replace_slot(
            { region = "main_bar", section = "left", id = "plugin.a.slot" },
            {
                region = "main_bar",
                section = "left",
                id = "plugin.a.slot",
                priority = 422,
                widget = { kind = "text", value = "B" },
            }
        )
        "#,
    )
    .expect("load plugin_replace");

    let replayed = replay_main_bar(&host);
    assert_eq!(
        main_bar_slot_priority(&replayed, "plugin.a.slot"),
        Some(422)
    );
    assert_eq!(
        slot_owner(&host, "main_bar", "left", "plugin.a.slot").as_deref(),
        Some("plugin_replace")
    );

    host.unload_plugin("plugin_replace")
        .expect("unload plugin_replace");

    let replayed = replay_main_bar(&host);
    assert_eq!(
        main_bar_slot_priority(&replayed, "plugin.a.slot"),
        Some(421)
    );
    assert_eq!(
        slot_owner(&host, "main_bar", "left", "plugin.a.slot").as_deref(),
        Some("plugin_add")
    );

    host.unload_plugin("plugin_add").expect("unload plugin_add");

    let replayed = replay_main_bar(&host);
    assert!(!replayed.contains("plugin.a.slot"));
    assert_eq!(slot_owner(&host, "main_bar", "left", "plugin.a.slot"), None);
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

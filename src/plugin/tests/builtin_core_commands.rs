use serde_json::json;

use crate::plugin::commands::{InvokeOutcome, PaletteState, HOST_COMMAND_PLUGIN_ID};
use crate::plugin::core_commands::CoreCommandAction;
use crate::plugin::tests::harness::MockHost;
use crate::screens::repository::RepositoryMessage;

fn manifest(id: &str, capabilities: &[&str]) -> String {
    let caps = capabilities
        .iter()
        .map(|cap| format!(r#""{cap}""#))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"
id = "{id}"
name = "{id}"
version = "0.1.0"
api_version = "1.0"
capabilities = [{caps}]
"#
    )
}

#[test]
fn built_in_core_commands_feed_devtools_palette_and_keymaps() {
    let mut host = MockHost::new();
    host.set_builtin_keymap("repository", "<C-p>", "repository.pull", "Pull");

    let snap = host.introspect();
    let pull = snap
        .commands
        .iter()
        .find(|cmd| cmd.name == "repository.pull")
        .expect("repository.pull command");
    assert_eq!(pull.plugin_id, HOST_COMMAND_PLUGIN_ID);
    assert!(pull.enabled);
    assert!(pull.keymap_eligible);
    assert!(pull.palette_visible);
    assert!(pull.hook_after);
    assert!(pull
        .plugin_invocation_capabilities
        .iter()
        .any(|cap| cap == "git:write:fetch"));

    let summaries = host.command_registry().borrow().summaries();
    let mut palette = PaletteState::new();
    palette.set_context("repository");
    palette.set_query("pull");
    let visible = palette.filter(&summaries);
    assert!(visible.iter().any(|cmd| cmd.name == "repository.pull"));

    assert!(snap.keymaps.iter().any(|row| {
        row.context == "repository" && row.command == "repository.pull" && row.key == "<C-p>"
    }));
}

#[test]
fn plugin_can_invoke_allowed_built_in_command() {
    let mut host = MockHost::new();
    host.host_mut()
        .sync_repository("repo", "/tmp/repo", "main", "abc123", "origin", &[]);
    host.load_inline(
        "allowed",
        &manifest("allowed", &["command:invoke:repository.open_search"]),
        r#"
        _G.ok = tostring(leviathan.command.invoke("repository.open_search"))
        "#,
    )
    .expect("load allowed");

    assert_eq!(
        host.read_global_string("allowed", "ok").as_deref(),
        Some("true")
    );
    let actions = host.host_mut().take_core_command_actions();
    assert!(matches!(
        actions.as_slice(),
        [CoreCommandAction::Repository(message)]
            if matches!(message.as_ref(), RepositoryMessage::OpenCommitSearch)
    ));
}

#[test]
fn plugin_invoke_denies_without_required_context() {
    let mut host = MockHost::new();
    host.load_inline(
        "wrong_context",
        &manifest("wrong_context", &["command:invoke:repository.open_search"]),
        r#"
        _G.ok = tostring(leviathan.command.invoke("repository.open_search"))
        "#,
    )
    .expect("load wrong context");

    assert_eq!(
        host.read_global_string("wrong_context", "ok").as_deref(),
        Some("false")
    );
    let codes: Vec<String> = host
        .diagnostics()
        .tail(100)
        .into_iter()
        .map(|diag| diag.code)
        .collect();
    assert!(codes.iter().any(|code| code == "command.context_denied"));
}

#[test]
fn plugin_invoke_denies_without_capability() {
    let mut host = MockHost::new();
    host.host_mut()
        .sync_repository("repo", "/tmp/repo", "main", "abc123", "origin", &[]);
    host.load_inline(
        "denied",
        &manifest("denied", &[]),
        r#"
        _G.ok = tostring(leviathan.command.invoke("repository.open_search"))
        "#,
    )
    .expect("load denied");

    assert_eq!(
        host.read_global_string("denied", "ok").as_deref(),
        Some("false")
    );
    assert!(host.host_mut().take_core_command_actions().is_empty());
    let codes: Vec<String> = host
        .diagnostics()
        .tail(100)
        .into_iter()
        .map(|diag| diag.code)
        .collect();
    assert!(codes.iter().any(|code| code == "command.capability_denied"));
}

#[test]
fn toolbar_command_queues_same_pull_message() {
    let mut host = MockHost::new();
    let outcome = host.invoke_command("repository.pull", json!({}));
    assert!(matches!(outcome, InvokeOutcome::Ok));
    let actions = host.host_mut().take_core_command_actions();
    assert!(matches!(
        actions.as_slice(),
        [CoreCommandAction::Repository(message)]
            if matches!(message.as_ref(), RepositoryMessage::PullRequested)
    ));
}

#[test]
fn command_result_event_fires_for_built_in_command() {
    let mut host = MockHost::new();
    host.load_inline(
        "watcher",
        &manifest("watcher", &[]),
        r#"
        leviathan.autocmd.create("CommandExecuted", {
            callback = function(ev)
                _G.last_name = ev.payload.name
                _G.last_ok = tostring(ev.payload.ok)
            end,
        })
        "#,
    )
    .expect("load watcher");

    let outcome = host.invoke_command("repository.open_search", json!({}));
    assert!(matches!(outcome, InvokeOutcome::Ok));
    assert_eq!(
        host.read_global_string("watcher", "last_name").as_deref(),
        Some("repository.open_search")
    );
    assert_eq!(
        host.read_global_string("watcher", "last_ok").as_deref(),
        Some("true")
    );
}

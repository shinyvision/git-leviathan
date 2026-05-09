# Leviathan Plugin Author Guide

Leviathan plugins are Lua scripts that extend the GUI git client. They live
under the app config root in `plugins/<plugin_id>/` and consist of two files:

- `plugin.toml` — manifest declaring identity, capabilities, services
- `init.lua` — the plugin's entry point

On Linux `<config-root>` is `$XDG_CONFIG_HOME/git_leviathan`, falling back to
`~/.config/git_leviathan`. On macOS it is
`~/Library/Application Support/git_leviathan`, and on Windows it is
`%APPDATA%\git_leviathan`. That directory has its own bootstrap `init.lua`
which decides what to load:

```lua
leviathan.plugins.load_dir("plugins")
```

## Hello World

Create `<config-root>/plugins/hello_world/plugin.toml`:

```toml
id = "hello_world"
name = "Hello World"
version = "0.1.0"
api_version = "1.0"
```

And `<config-root>/plugins/hello_world/init.lua`:

```lua
local handle, err = leviathan.ui.slot.add{
    region = "main_bar",
    id = "hello_world.greeting",
    section = "left",
    priority = 100,
    widget = { kind = "text", value = "Hello!" },
}
assert(handle, err)
```

That's it. Restart the app and you'll see "Hello!" in the main bar.

## Concept Map

- **Regions** — `main_bar`, `tab_bar`, `repository` are UI regions.
  Discover regions with `leviathan.ui.region.*`.
- **Slots** — region elements identified by `(region, section/pane, id)`.
  Add, remove, and replace slots with `leviathan.ui.slot.*`.
- **UI Context** — dynamic slot widgets receive `ctx` with theme,
  repository, tab, selection, focus, viewport, and surface payload summaries.
- **Screens** — full-page UI panels registered via `leviathan.ui.screen.register`.
- **Autocmds** — event subscriptions via `leviathan.autocmd.create`.
  Fire on tab changes, fetches, branch switches.
- **Capabilities** — host calls (filesystem, env, etc.) require capability
  declarations in `plugin.toml`. See [manifest.md](manifest.md).
- **Services** — plugins publish/consume named, versioned RPC interfaces.
  See [api/services.md](api/services.md).
- **Persistence** — `leviathan.persist.open` provides versioned KV storage.

## Public UI API

| API call | Required capability | Behavior |
| --- | --- | --- |
| `leviathan.ui.slot.add(spec)` | `ui:region:<region>` | Adds a toolbar, tab bar, or repository-pane slot and returns a handle. |
| `leviathan.ui.slot.replace(target, spec)` | `ui:region:<region>` plus `ui:replace:*` for slots owned by others | Replaces an existing slot by full address and returns a handle. |
| `leviathan.ui.slot.remove(target)` | `ui:region:<region>` plus `ui:remove:*` for slots owned by others | Removes a slot by full address. |
| `leviathan.ui.context_menu(point, item)` | `ui:context_menu:<point>` | Adds a command-backed menu item at a context-menu extension point. |
| `leviathan.ui.graph_decoration(commit_hash, decoration)` | `ui:decoration:graph` | Adds a static decoration to a commit graph row. |
| `leviathan.ui.diff_decoration(decoration)` | `ui:decoration:diff` | Adds a static decoration to a diff line or hunk. |
| `leviathan.ui.overlay(spec)` | `ui:overlay` | Shows a host-owned overlay above the active screen. |
| `leviathan.ui.remove_overlay(id)` | `ui:overlay` | Removes an overlay owned by the calling plugin. |
| `leviathan.ui.screen.register(spec)` | `ui:screen` | Registers a full plugin screen with lifecycle callbacks. |
| `leviathan.ui.dock.register(spec)` | `ui:dock` | Registers a persistent dock panel with host-owned layout state. |
| `leviathan.ui.settings.register(spec)` | none | Registers a custom settings view for the plugin. |
| `leviathan.ui.context.current()` | none | Returns the current typed UI context for dynamic views. |
| `leviathan.ui.region.list()` / `describe(name)` | none | Lists and describes mounted UI regions. |

## Hot Reload

Save `init.lua`; the host re-runs it. Open screens with `serialize` /
`deserialize` hooks preserve their state. Failed reloads roll back to the
last working version.

## Type Stubs

Run `cargo xtask gen-stubs` and symlink `target/plugin-stubs/leviathan.lua`
to `~/.config/leviathan/types/leviathan.lua` for autocomplete in
lua-language-server.

## Stability And Certification

- [stability.md](stability.md) defines the API 1.0 compatibility matrix and semver policy.
- [certification.md](certification.md) defines the `cargo xtask plugin certify` suite.

## Examples

- [hello_world](examples/hello_world/) — minimal slot
- [persist_counter](examples/persist_counter/) — uses persist API
- [service_publisher](examples/service_publisher/) — publishes a service
- [toolbar_only](../../plugins/examples/toolbar_only/) — toolbar slot
- [graph_decoration_provider](../../plugins/examples/graph_decoration_provider/) — commit graph decoration
- [diff_gutter_provider](../../plugins/examples/diff_gutter_provider/) — diff gutter decoration
- [dock_panel](../../plugins/examples/dock_panel/) — dock panel
- [full_screen](../../plugins/examples/full_screen/) — plugin screen
- [settings_panel](../../plugins/examples/settings_panel/) — custom settings panel

## API Reference

- [api/ui.md](api/ui.md)
- [api/fs.md](api/fs.md)
- [api/env.md](api/env.md)
- [api/api.md](api/api.md)
- [api/services.md](api/services.md)
- [api/persist.md](api/persist.md)
- [api/repository.md](api/repository.md)
- [api/tab_registry.md](api/tab_registry.md)

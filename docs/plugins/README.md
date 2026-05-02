# Leviathan Plugin Author Guide

Leviathan plugins are Lua scripts that extend the GUI git client. They live
in `<repo-or-config>/plugins/<plugin_id>/` and consist of two files:

- `plugin.toml` — manifest declaring identity, capabilities, services
- `init.lua` — the plugin's entry point

## Hello World

Create `plugins/hello_world/plugin.toml`:

```toml
id = "hello_world"
name = "Hello World"
version = "0.1.0"
api_version = "1.0"
```

And `plugins/hello_world/init.lua`:

```lua
leviathan.ui.main_bar.add{
    id = "hello_world.greeting",
    section = "left",
    priority = 100,
    widget = { kind = "text", value = "Hello!" },
}
```

That's it. Restart the app and you'll see "Hello!" in the main bar.

## Concept Map

- **Regions** — `main_bar`, `tab_bar`, `repository` are UI regions. Each
  exposes `:add(spec)`, `:remove(target)`, `:replace(target, spec)`.
  See [api/ui.md](api/ui.md).
- **Slots** — region elements identified by `(region, section/pane, id)`.
- **Screens** — full-page UI panels registered via `leviathan.ui.register_screen`.
- **Autocmds** — event subscriptions via `leviathan.api.create_autocmd`.
  Fire on tab changes, fetches, branch switches.
- **Capabilities** — host calls (filesystem, env, etc.) require capability
  declarations in `plugin.toml`. See [manifest.md](manifest.md).
- **Services** — plugins publish/consume named, versioned RPC interfaces.
  See [api/services.md](api/services.md).
- **Persistence** — `leviathan.persist.open` provides versioned KV storage.

## Hot Reload

Save `init.lua`; the host re-runs it. Open screens with `serialize` /
`deserialize` hooks preserve their state. Failed reloads roll back to the
last working version.

## Type Stubs

Run `cargo xtask gen-stubs` and symlink `target/plugin-stubs/leviathan.lua`
to `~/.config/leviathan/types/leviathan.lua` for autocomplete in
lua-language-server.

## Examples

- [hello_world](examples/hello_world/) — minimal slot
- [persist_counter](examples/persist_counter/) — uses persist API
- [service_publisher](examples/service_publisher/) — publishes a service

## API Reference

- [api/ui.md](api/ui.md)
- [api/fs.md](api/fs.md)
- [api/env.md](api/env.md)
- [api/api.md](api/api.md)
- [api/services.md](api/services.md)
- [api/persist.md](api/persist.md)
- [api/repository.md](api/repository.md)
- [api/tab_registry.md](api/tab_registry.md)

# Plugin Package Layout

The package layout supports multi-file plugins modelled on Neovim. A single
`init.lua` still works, but plugins of any size should split into modules.

## Layout

```text
plugins/<plugin_id>/
  plugin.toml                       # manifest
  init.lua                          # entry point; runs once at load
  lua/<plugin_id>/                  # plugin's own modules
    foo.lua                         # require("<plugin_id>.foo")
    bar/init.lua                    # require("<plugin_id>.bar")
    bar/baz.lua                     # require("<plugin_id>.bar.baz")
  after/plugin/                     # post-init bootstrap; lexical order
    01_init.lua
    02_keymaps.lua
  assets/                           # bundled files (icons, gifs, ...)
  doc/                              # plugin documentation
  tests/                            # plugin tests (plugin-test)
  migrations/                       # persist migrations
```

## `require` resolution

The host installs its own `require` and removes `dofile` / `loadfile`. Names
are resolved against an ordered runtime path:

1. The calling plugin's own `lua/<plugin_id>/` tree.
2. Each plugin id listed in the manifest's `requires_plugins`, in order, if
   that plugin is currently loaded.

Names are validated before any filesystem lookup. Module names must be
dotted ASCII identifiers (alphanumeric, `_`, `-`). Path traversal (`..`),
forward and backward slashes, and colons are rejected with a
`lua.module.invalid_name` diagnostic.

A `require("other_plugin.foo")` where `other_plugin` is not in
`requires_plugins` is rejected with `lua.module.forbidden`. A name that
passes the policy filter but does not match any file produces
`lua.module.not_found`.

The module cache is per generation: reload drops every cached module, so a
fresh generation never sees stale values.

## `after/plugin/`

After `init.lua` succeeds, the host walks `after/plugin/*.lua` in lexical
order. Each file is compiled and executed independently, so a single bad
file produces a diagnostic without skipping the rest of the directory.
Failures emit `runtime.after_dir_failed` (I/O) or `lua.after_failed`
(execution).

## Strict globals

Plugins are loaded with strict-globals enforcement on by default:

- Reading an undeclared Lua global raises a Lua error and emits
  `lua.strict_globals.read`.
- Writing an undeclared Lua global raises a Lua error and emits
  `lua.strict_globals.write`.

The Lua stdlib (`string`, `table`, `math`, `pairs`, `ipairs`, `print`,
`type`, ...) and the `leviathan` host table are always allowed; they are
captured at install time. Plugin code should use `local` for everything
else and surface state through returned modules.

To opt out (e.g. for plugins that intentionally use `_G`), add to
`plugin.toml`:

```toml
[runtime]
strict_globals = false
```

## Runtime introspection

The `leviathan.runtime` table exposes three helpers, all host-implemented:

- `leviathan.runtime.path()` — ordered runtime-path entries
  (`{ plugin, kind, root }`).
- `leviathan.runtime.find(module)` — first matching path or `nil`.
- `leviathan.runtime.module_graph()` — currently cached modules, grouped by
  owning plugin.

These read live state owned by the host loader. The same state is exposed
to in-app devtools as `InspectorSnapshot.runtime_paths` and
`InspectorSnapshot.loaded_modules`.

## Migration from single-file plugins

A single-file `init.lua` keeps working unchanged. To migrate:

1. Move helper functions and constants into `lua/<plugin_id>/<name>.lua`.
2. Have each module return a table (`local M = {}; ... return M`).
3. Replace local definitions in `init.lua` with
   `local <name> = require("<plugin_id>.<name>")`.
4. If your plugin scribbled on `_G`, either rewrite to `local`/`M.` or
   add `[runtime] strict_globals = false` to the manifest.

The bundled `foo_demo` plugin in this repo demonstrates the new layout.

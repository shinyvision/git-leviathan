# Plugin API v1

Compatibility surface: plugin manifests target `api_version = "1.0"`.
The current host compatibility version is `1.0`; future plugin migrations
should either preserve this surface or explicitly version-gate changes.

This document freezes the Lua APIs and host-owned plugin resources that exist
before the plugin refactor begins.

## Lua Global

Every plugin runs in its own Lua state with a global `leviathan` table.

- `leviathan.log(message)` logs to host stderr.
- `leviathan.repository` starts as an empty repository snapshot and is replaced
  by the host after repository sync.
- `leviathan.tab_registry` starts as an empty tab snapshot and is refreshed by
  the host after tab changes.

## `leviathan.api`

- `create_autocmd(events, opts)` registers `opts.callback` once per event name.
- `schedule(fn)` queues `fn` for the next host tick.
- `defer_fn(ms, fn)` queues `fn` after `ms` milliseconds have elapsed.
- `create_user_command(name, fn)` registers a named coroutine-backed command.

Current host event names used by bundled plugins and app code:

- `BranchChanged`
- `FetchStart`
- `FetchEnd`
- `TabAdded`
- `TabRemoved`
- `TabReordered`
- `TabSwitched`

## `leviathan.ui`

- `list_regions()` returns the region names in descriptor order.
- `region(name)` returns the matching region handle or errors.
- `register_screen(spec)` stores a plugin screen definition.

Screen spec fields:

- `id`: screen id local to the plugin.
- `init`: function returning initial screen state.
- `view`: function receiving state and returning a widget tree.
- `update`: function receiving `(state, event, value)` and returning either a
  state table or `{ state = ..., navigate = ... }`.
- `serialize`: optional function returning persisted screen state for reload.
- `deserialize`: optional function rebuilding state from serialized data.

Each region handle exposes:

- `add(spec)`
- `remove(target)`
- `replace(target, spec)`

Slot spec fields:

- `id`: slot id.
- `section`: required for chrome regions.
- `pane`: required for content regions.
- `priority`: integer ordering key.
- `widget`: widget tree table or function returning a widget tree.
- `on_click`: optional slot callback function.

Slot callbacks receive `(slot_id, event, value)`. Returning
`{ navigate = "screen_id" }` opens that plugin screen.

Current region descriptors:

| Region | Type | Address |
| --- | --- | --- |
| `main_bar` | chrome | `section = "left" / "center" / "right"` |
| `tab_bar` | chrome | `section = "left" / "center" / "right"` |
| `repository` | content | `pane = "sidebar" / "main"`, `section = "top" / "bottom"` |

The tables are available as `leviathan.ui.main_bar`,
`leviathan.ui.tab_bar`, and `leviathan.ui.repository`.

## `leviathan.fs`

Filesystem APIs use Lua-style result pairs for fallible operations:
`(value, nil)` or `(nil, err)`, and `(true, nil)` or `(false, err)` for
mutations.

Read operations:

- `read_file(path)`
- `read_lines(path)`
- `list_dir(path)`
- `exists(path)`
- `is_file(path)`
- `is_dir(path)`
- `is_symlink(path)`
- `size(path)`
- `modified(path)`
- `metadata(path)`
- `read_link(path)`
- `copy(src, dst)` also requires write access to `dst`.

Write operations:

- `write_file(path, content)`
- `append_file(path, content)`
- `delete(path)`
- `mkdir(path)`
- `rename(src, dst)`
- `touch(path)`

Path helpers and locations:

- `parent(path)`
- `basename(path)`
- `stem(path)`
- `extension(path)`
- `join(a, b)`
- `relative_to(path, base)`
- `with_extension(path, ext)`
- `with_file_name(path, name)`
- `is_absolute(path)`
- `canonicalize(path)`
- `cwd()`
- `home()`
- `temp_dir()`
- `config_dir()`
- `cache_dir()`
- `data_dir()`
- `state_dir()`

File content operations are capped at 8 MiB per call for read, write, append,
and copy. Relative paths passed to `read_file` and `read_lines` resolve under
the plugin root.

## `leviathan.env`

Requires the `env` capability.

- `get(name)` returns `(value, nil)`, `(nil, nil)` when missing, or
  `(nil, err)` for non-UTF-8 values.
- `list()` returns a table of UTF-8 environment variables.

## `leviathan.repository`

Read-only snapshot fields:

- `name`
- `workdir_path`
- `current_branch_name`
- `current_branch`
- `is_open`
- `is_detached`
- `is_unborn`
- `is_bare`
- `head_hash`
- `default_remote_name`
- `local_branches`
- `remote_branches`
- `tags`

Local branch fields:

- `name`
- `hash`
- `is_current`
- `upstream_branch`

Remote branch fields:

- `name`
- `remote_name`
- `hash`

Tag fields:

- `name`
- `hash`

## `leviathan.tab_registry`

Snapshot fields:

- `list`: array of `{ path, name }`.
- `current`: current `{ path, name }` or nil.

Mutation functions queue host tab operations:

- `add(path)`
- `remove(path)`
- `select(path)`
- `reorder(paths)`

## `leviathan.services`

- `register(name, version, methods)` publishes a service declared in
  `provides_services`.
- `get(name, version)` returns a proxy for a service declared in
  `consumes_services`, or `nil` for declared optional consumers whose
  provider is absent.

The legacy `"name@version"` form still works for both functions. Required
consumers block load/reload when missing; optional consumers can use
`{ service = "name@version", optional = true }` in `plugin.toml`.

## `leviathan.persist`

- `open(name, opts)` opens a plugin-local JSON store in the plugin state dir.

Options:

- `version`: target integer version, default `1`.
- `migrations`: array of `{ from, to, transform }`.

Returned store userdata:

- `store:get(key)`
- `store:set(key, value)`
- `store:version()`

## `leviathan.health`

- `register(fn)` stores a health-check callback.

The host calls the callback with a context userdata exposing:

- `ctx:ok(message)`
- `ctx:info(message)`
- `ctx:warn(message)`
- `ctx:error(message)`

## Manifest Surface

Current manifest fields:

- `id`
- `name`
- `version`
- `api_version`
- `description`
- `capabilities`
- `provides_services`
- `consumes_services`
- `dependencies`

Current capability strings:

- `fs:read` and `fs:read:plugin`
- `fs:read:state`
- `fs:read:config`
- `fs:read:workdir`
- `fs:read:any`
- `fs:write:plugin`
- `fs:write:state`
- `fs:write:config`
- `fs:write:workdir`
- `fs:write:any`
- `process:spawn`
- `net:fetch`
- `clipboard`
- `notify`
- `env`

## PluginHost Resource Inventory

`PluginHost` currently stores these plugin-owned resources directly or in
host-wide collections:

- Loaded plugin table: plugin id, root path, manifest, Lua state.
- Slot handlers: registry keys keyed by `region:container:id`.
- Screen definitions: `init`, `view`, `update`, optional `serialize`, optional
  `deserialize` registry keys.
- Screen state: per-screen Lua registry keys.
- Dynamic widgets: widget function registry keys plus cached JSON widget trees.
- Deferred queue: immediate callbacks, delayed callbacks, suspended coroutines.
- User commands: names mapped to Lua registry keys.
- Health checks: Lua registry keys.
- Slot operation log: add, replace, and remove operations for every region.
- Autocmd map: event names mapped to `(plugin_id, callback)` entries.
- Active screen and current widget tree.
- Split widget sizes and current split drag state.
- Shared pending tab operations queue.
- Service registry entries published by plugins.
- Capability audit log entries.
- Last reload error per plugin.

`last_repository_hash` and `last_tab_snapshot` are host sync caches, not
plugin-owned public resources, but they affect when observable tables and
autocmds are refreshed.

## Bundled Plugin API Usage

| Plugin | APIs used |
| --- | --- |
| `dancing_banana_test` | `leviathan.api.create_autocmd`, `leviathan.ui.main_bar.replace`, `leviathan.log` |
| `file_explorer` | `leviathan.fs.list_dir`, `home`, `parent`, `delete`; `leviathan.ui.main_bar.add`, `register_screen`; `leviathan.log` |
| `foo_demo` | `leviathan.ui.main_bar.add`, `register_screen`, `leviathan.log` |
| `regions_demo` | `leviathan.ui.tab_bar.add`, `leviathan.ui.repository.add` |
| `repository_info` | `leviathan.ui.main_bar.replace`, `remove`; `leviathan.repository`; `leviathan.log` |
| `tablist_demo` | `leviathan.api.create_autocmd`, `leviathan.ui.tab_bar.replace`, `leviathan.tab_registry.list/current/select/remove/reorder`, `leviathan.log` |
| `terminal` | `leviathan.ui.main_bar.add`, `leviathan.log` |

Bundled plugins now declare `api_version = "2.0"`; keep this page as the
frozen v1 compatibility reference for older public plugins.

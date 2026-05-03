# `plugin.toml` Reference

The manifest is parsed against the `PluginManifest` schema in
`git_leviathan_plugin_api::manifest`. All fields are flat (no nested
tables except `[dependencies]`).

## Required Fields

- `id` (string) — unique plugin identifier; matches the directory name.
- `name` (string) — human-readable name.
- `version` (string) — semver.
- `api_version` (string) — host API version requested. Format `MAJOR.MINOR`.
  Plugin loads if its major matches the host and its minor <= host minor.

## Optional Fields

- `description` (string) — one-line summary.
- `capabilities` (string array) — host calls the plugin needs. Examples:
  `"fs:read"`, `"fs:read:any"`, `"fs:write:state"`, `"fs:write:plugin"`,
  `"process:spawn"`, `"net:fetch"`, `"clipboard"`, `"notify"`, `"env"`.
- `provides_services` (string array) — services the plugin publishes,
  in `"name@version"` form. Required to call `leviathan.services.register`.
- `consumes_services` (string array) — services the plugin uses.
  Required to call `leviathan.services.get`.

## `[dependencies]` Table

Declare plugin dependencies as `id = "<semver req>"`:

```toml
[dependencies]
repository_info = ">=0.5.0"
```

(Dependency resolution is descriptive in this version; the host doesn't
yet enforce required deps. Useful as documentation today.)

## Example

```toml
id = "git-tools"
name = "Git Tools"
version = "1.2.0"
api_version = "2.0"
description = "Useful git extensions."
capabilities = ["fs:read:workdir", "process:spawn"]
provides_services = ["diff_viewer@1"]
consumes_services = ["repository@1"]

[dependencies]
repository_info = ">=0.5.0"
```

## Capability Scopes

Filesystem capabilities are scoped to a directory:

| Scope | Path |
|-------|------|
| `plugin` | The plugin's own directory |
| `state` | `~/.local/state/git_leviathan/<plugin_id>/` |
| `config` | `~/.config/git_leviathan/<plugin_id>/` |
| `workdir` | The active git workdir |
| `any` | Anywhere |

Bare `fs:read` defaults to `fs:read:plugin`.

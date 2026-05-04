# Plugin API Stability

Plugin API `1.0` is the first stable public API for GUI plugins.

## Compatibility Matrix

| Host version | Plugin API | Widget schema | Extension points |
| --- | --- | --- | --- |
| `1.0` | `1.0` | `1` | `main_bar`, `tab_bar`, `repository`, `screens`, `dock.pane`, `settings.panel`, `overlays`, `commands`, graph/diff decorations, context menus |

Plugins declare the target API in `plugin.toml`:

```toml
api_version = "1.0"
```

The host rejects unsupported `api_version` values before running plugin code.

## Semver Policy

- Additions in host `1.x` may include new functions, optional fields, widgets, capabilities, events, and extension points.
- Public API removals are not allowed in host `1.x`.
- Renames or validation tightening require deprecation before a later major API.
- Descriptor schema changes increment `descriptor_version`.

## Stability Metadata

Generated descriptors include `since`, `stability`, and the migration promise
for the stable API. `experimental_apis` is empty for API `1.0`.

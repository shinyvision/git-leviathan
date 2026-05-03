# Plugin Widget v1

Compatibility surface: plugin manifests target `api_version = "1.0"`.

Plugin UI is declarative. Lua returns a nested widget table with a required
`kind` string; Rust owns rendering, event routing, asset resolution, and split
state. Unknown extra fields are tolerated. Unknown widget kinds are rejected
when static slot specs are registered and render as error text in the runtime
widget builder.

## Common Values

- Length: number of pixels, `"fill"`, or `"shrink"`.
- Color: `"#RRGGBB"`.
- Horizontal alignment: `"start"`, `"left"`, `"center"`, `"centre"`,
  `"end"`, `"right"`.
- Vertical alignment: `"start"`, `"top"`, `"center"`, `"centre"`, `"end"`,
  `"bottom"`.
- Asset paths for `icon` and `image` are safe relative paths under the plugin
  root. Absolute paths, `.` components, `..` components, NUL bytes, empty
  strings, and paths over 256 bytes are rejected by the renderer.

## Event Routing

Interactive widget events route by render scope.

- Screen scope: `button`, `mouse_area`, and `tablist` dispatch to the screen
  `update(state, event, value)` callback.
- Slot scope: those widgets dispatch to the slot `on_click(slot_id, event,
  value)` callback.

Empty or absent event names are non-interactive.

## Widget Kinds And Fields

### `text`

Fields:

- `kind = "text"`
- `value`: string, default `""`.
- `size`: number, default `14`.
- `color`: color, default theme text.

### `button`

Fields:

- `kind = "button"`
- `child`: widget. Wins over `text` when both are present.
- `text`: string shortcut for a text child.
- `on_click`: event name.
- `value`: JSON-like Lua value delivered with the event.
- `width`: length.
- `height`: length, default `"shrink"`.
- `style`: button style table.

Style fields:

- `background`: color.
- `background_hover`: color.
- `text_color`: color.
- `border`: `{ width, radius, color }`.

Buttons have zero built-in padding; wrap content in a `padding` widget.

### `row`

Fields:

- `kind = "row"`
- `children`: array of widgets.
- `spacing`: number, default `0`.
- `width`: length, default `"fill"`.
- `height`: length, default `"shrink"`.
- `align_y`: vertical alignment.

### `column`

Fields:

- `kind = "column"`
- `children`: array of widgets.
- `spacing`: number, default `0`.
- `width`: length, default `"fill"`.
- `height`: length, default `"fill"`.
- `align_x`: horizontal alignment.

### `container`

Fields:

- `kind = "container"`
- `child`: widget.
- `bg`: color.
- `width`: length, default `"fill"`.
- `height`: length, default `"fill"`.
- `max_width`: number.
- `max_height`: number.
- `min_width`: number, consumed by `resizable_split` size limits.
- `min_height`: number, consumed by `resizable_split` size limits.
- `center_x`: boolean, default `false`.
- `center_y`: boolean, default `false`.

Container does not have padding. Wrap the child in a `padding` widget.

### `padding`

Fields:

- `kind = "padding"`
- `top`: number, default `0`.
- `right`: number, default `0`.
- `bottom`: number, default `0`.
- `left`: number, default `0`.
- `width`: length, default `"shrink"`.
- `height`: length, default `"shrink"`.
- `child`: widget.

### `space`

Fields:

- `kind = "space"`
- `width`: length, default `"shrink"`.
- `height`: length, default `"shrink"`.

### `icon`

Fields:

- `kind = "icon"`
- `path`: plugin-relative SVG path.
- `size`: number, default `16`.
- `color`: color, default theme text.

### `image`

Fields:

- `kind = "image"`
- `path`: plugin-relative raster image path.
- `size`: number, default `16`.

### `scrollable`

Fields:

- `kind = "scrollable"`
- `child`: widget.
- `width`: length, default `"fill"`.
- `height`: length, default `"fill"`.

### `mouse_area`

Fields:

- `kind = "mouse_area"`
- `child`: widget.
- `on_click`: event name.
- `value`: JSON-like Lua value delivered with the event.

### `tablist`

Fields:

- `kind = "tablist"`
- `tabs`: array of `{ id, name }`.
- `active`: id matching one of the tab ids.
- `orderable`: boolean, default `false`.
- `on_select`: event name; payload is the selected tab id.
- `on_close`: event name; payload is the closed tab id.
- `on_reorder`: event name; payload is the reordered id array.

Tab ids may be any JSON-like Lua value.

### `resizable_split`

Fields:

- `kind = "resizable_split"`
- `id`: string, default `"split"`.
- `direction`: `"horizontal"` or `"vertical"`, default `"horizontal"`.
- `children`: array of widgets.

This widget renders only in screen scope. Split sizes are host-owned and keyed
by plugin id, screen/slot scope, and split id. Child size limits come from
container `min_width` / `max_width` for horizontal splits and `min_height` /
`max_height` for vertical splits.

## Slot Registration Surface

Slot specs are not widgets themselves, but they own widget trees.

Chrome region slot spec:

```lua
{
  id = "plugin.example.slot",
  section = "left",
  priority = 10,
  widget = { kind = "text", value = "hello" },
  on_click = function(slot_id, event, value) end,
}
```

Content region slot spec:

```lua
{
  id = "plugin.example.banner",
  pane = "sidebar",
  section = "top",
  priority = 10,
  widget = { kind = "text", value = "hello" },
}
```

`widget` may also be a function returning a widget tree. The host refreshes
dynamic slot widgets when plugin-observable state changes.

## Region Descriptor Snapshot

Current v1 region descriptors:

```text
api_version = "1.0"
main_bar: chrome sections=[left, center, right]
tab_bar: chrome sections=[left, center, right]
repository: content panes=[sidebar(top,bottom), main(top,bottom)]
```

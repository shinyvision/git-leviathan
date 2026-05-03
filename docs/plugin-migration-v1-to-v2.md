# Plugin Migration: v1 to v2

Git Leviathan still loads `api_version = "1.0"` plugins through a compatibility
shim. New plugins should target:

```toml
api_version = "2.0"
```

v2 keeps the same widget tree shape and manifest fields, but the recommended
Lua surface is descriptor-backed and easier for the host to validate.

## Feature Checks

Use `leviathan.has(...)` when a plugin can run on more than one host version.

```lua
if leviathan.has("ui.regions.add_slot@2") then
  leviathan.ui.regions.add_slot({
    region = "main_bar",
    section = "right",
    id = "plugin.example.button",
    priority = 100,
    widget = { kind = "text", value = "Example" },
  })
end

if leviathan.has("autocmd.create@2") then
  leviathan.autocmd.create("FetchStart", {
    callback = function(event)
      leviathan.log(event.event)
    end,
  })
end
```

The v1 compatibility descriptors are still queryable:

```lua
if leviathan.has("ui.main_bar.add@1") then
  -- Old public plugins can still use this path.
end
```

## Manifest

Before:

```toml
id = "example"
name = "Example"
version = "0.1.0"
api_version = "1.0"
```

After:

```toml
id = "example"
name = "Example"
version = "0.1.0"
api_version = "2.0"
```

## Region Slots

v1 used per-region helper tables:

```lua
leviathan.ui.main_bar.add({
  section = "right",
  id = "plugin.example.button",
  priority = 100,
  widget = { kind = "text", value = "Example" },
})
```

v2 uses one descriptor-backed region namespace:

```lua
leviathan.ui.regions.add_slot({
  region = "main_bar",
  section = "right",
  id = "plugin.example.button",
  priority = 100,
  widget = { kind = "text", value = "Example" },
})
```

Content regions include both `region` and `pane`:

```lua
leviathan.ui.regions.add_slot({
  region = "repository",
  pane = "sidebar",
  section = "top",
  id = "plugin.example.sidebar",
  priority = 10,
  widget = { kind = "text", value = "Repository note" },
})
```

Replace and remove also take the region in the target:

```lua
leviathan.ui.regions.replace_slot(
  { region = "main_bar", section = "left", id = "builtin.repo_info" },
  {
    region = "main_bar",
    section = "left",
    id = "plugin.example.repo_info",
    priority = 10,
    widget = { kind = "text", value = "Repo" },
  }
)

leviathan.ui.regions.remove_slot({
  region = "main_bar",
  section = "left",
  id = "builtin.branch_info",
})
```

## Autocmds

Before:

```lua
leviathan.api.create_autocmd({ "FetchStart", "FetchEnd" }, {
  callback = function(event)
    leviathan.log(event)
  end,
})
```

After:

```lua
leviathan.autocmd.create({ "FetchStart", "FetchEnd" }, {
  callback = function(event)
    leviathan.log(event.event)
  end,
})
```

## Commands

Before:

```lua
leviathan.api.create_user_command("example.hello", function()
  leviathan.log("hello")
end)
```

After:

```lua
leviathan.command.create("example.hello", {
  title = "Example: Hello",
  context = "global",
  run = function()
    leviathan.log("hello")
  end,
})
```

## Deprecation Diagnostics

The v1-only calls still work, but the host records `api.deprecated` warnings
for:

- `leviathan.api.create_user_command`
- `leviathan.api.create_autocmd`
- `leviathan.ui.main_bar.*`, `leviathan.ui.tab_bar.*`, and
  `leviathan.ui.repository.*`

These diagnostics are warnings, not plugin load failures.

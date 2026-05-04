assert(leviathan.ui.dock.register({
  id = "summary",
  title = "Example Summary",
  area = "right",
  default_open = true,
  view = function(ctx)
    return {
      kind = "padding",
      top = 8,
      right = 8,
      bottom = 8,
      left = 8,
      child = {
        kind = "column",
        spacing = 6,
        children = {
          { kind = "text", value = "Dock panel", size = 14 },
          { kind = "text", value = ctx.repository.name or "No repository", size = 12 },
        },
      },
    }
  end,
}))

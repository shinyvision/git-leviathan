assert(leviathan.ui.dock.register({
  id = "acceptance",
  title = "Acceptance Dock",
  area = "right",
  default_open = true,
  view = function(ctx)
    return {
      kind = "column",
      spacing = 4,
      children = {
        { kind = "text", value = "Dock" },
        { kind = "text", value = ctx.repository.name or "No repository" },
      },
    }
  end,
}))

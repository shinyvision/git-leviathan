leviathan.ui.settings.register({
  view = function()
    return {
      kind = "column",
      spacing = 8,
      children = {
        { kind = "text", value = "Example settings", size = 16 },
        { kind = "text", value = "This panel can be replaced by a generated form.", size = 12 },
      },
    }
  end,
})

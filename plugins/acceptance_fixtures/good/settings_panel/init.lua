leviathan.settings.define_schema({
  enabled = { type = "boolean", default = true },
})

leviathan.ui.settings.register({
  view = function(ctx)
    return {
      kind = "column",
      spacing = 4,
      children = {
        { kind = "text", value = "Settings" },
        { kind = "text", value = tostring(ctx.values.enabled) },
      },
    }
  end,
})

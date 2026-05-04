leviathan.ui.screen.register({
  id = "home",
  title = "Example Screen",
  init = function()
    return { clicks = 0 }
  end,
  view = function(state, ctx)
    return {
      kind = "padding",
      top = 16,
      right = 16,
      bottom = 16,
      left = 16,
      child = {
        kind = "column",
        spacing = 8,
        children = {
          { kind = "text", value = "Full screen plugin", size = 18 },
          { kind = "text", value = ctx.repository.name or "No repository", size = 12 },
          { kind = "text", value = "Clicks: " .. tostring(state.clicks or 0), size = 12 },
          { kind = "button", on_click = "increment", child = { kind = "text", value = "Increment" } },
        },
      },
    }
  end,
  update = function(state, event)
    if event == "increment" then
      state.clicks = (state.clicks or 0) + 1
    end
    return state
  end,
})

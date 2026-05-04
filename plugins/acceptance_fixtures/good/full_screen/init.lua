leviathan.ui.screen.register({
  id = "home",
  title = "Acceptance Screen",
  init = function()
    return { visits = 1 }
  end,
  view = function(state, ctx)
    return {
      kind = "text",
      value = (ctx.repository.name or "No repository") .. " " .. tostring(state.visits or 0),
    }
  end,
  update = function(state, event)
    if event == "visit" then
      state.visits = (state.visits or 0) + 1
    end
    return state
  end,
})

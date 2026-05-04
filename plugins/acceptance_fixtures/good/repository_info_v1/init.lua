local render_count = 0

leviathan.ui.slot.replace(
  { region = "main_bar", section = "left", id = "builtin.repo_info" },
  {
    region = "main_bar",
    section = "left",
    id = "builtin.repo_info",
    priority = 10,
    widget = function()
      render_count = render_count + 1
      _G.render_count = render_count
      return { kind = "text", value = "Repo v1 #" .. tostring(render_count) }
    end,
  }
)

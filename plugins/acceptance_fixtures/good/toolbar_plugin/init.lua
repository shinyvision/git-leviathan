leviathan.command.create("toolbar_plugin.ping", {
  title = "Toolbar Plugin Ping",
  context = "global",
  run = function(_args)
    _G.toolbar_runs = (_G.toolbar_runs or 0) + 1
  end,
})

assert(leviathan.ui.slot.add({
  region = "main_bar",
  section = "right",
  id = "plugin.toolbar_plugin.ping",
  priority = 50,
  widget = {
    kind = "button",
    on_click = "toolbar_plugin.ping",
    child = { kind = "text", value = "Ping" },
  },
}))

leviathan.command.create("example_toolbar_only.ping", {
  title = "Example Toolbar: Ping",
  run = function()
    leviathan.log("example_toolbar_only ping")
  end,
})

assert(leviathan.ui.slot.add({
  region = "main_bar",
  section = "right",
  id = "example.toolbar_only.ping",
  priority = 100,
  widget = {
    kind = "button",
    on_click = "example_toolbar_only.ping",
    child = { kind = "text", value = "Ping" },
  },
}))

assert(leviathan.ui.slot.add({
  region = "main_bar",
  section = "left",
  id = "bad.invalid_ui",
  priority = 1,
  widget = { kind = "no_such_widget", value = "bad" },
}))

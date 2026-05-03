-- Demonstrates the regions API:
--  - a tag in the right side of the tab bar
--  - a banner above the sidebar in the repository view
--
-- Style rule: padding is always a standalone `padding` widget — no widget
-- on its own carries a `padding` field.

leviathan.ui.regions.add_slot({
  region = "tab_bar",
  section = "right",
  id = "plugin.regions_demo.tag",
  priority = 10,
  widget = {
    kind = "text",
    value = "[demo]",
  },
})

leviathan.ui.regions.add_slot({
  region = "repository",
  pane = "sidebar",
  section = "top",
  id = "plugin.regions_demo.banner",
  priority = 10,
  widget = {
    kind = "padding",
    top = 6,
    right = 8,
    bottom = 6,
    left = 8,
    child = {
      kind = "text",
      value = "regions_demo plugin",
    },
  },
})

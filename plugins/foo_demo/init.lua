-- Foo Demo plugin
-- Contributes a "Foo" main-bar button that opens a 3-panel screen.
--
-- Helper widgets live in `lua/foo_demo/widgets.lua` and
-- are loaded via `require("foo_demo.widgets")`. Style rule: padding is
-- always a standalone `padding` widget.

local widgets = require("foo_demo.widgets")
local std_button, panel = widgets.std_button, widgets.panel
local COLOR_TEXT = widgets.COLOR_TEXT

leviathan.ui.regions.add_slot {
  region   = "main_bar",
  id       = "plugin.foo_demo.foo",
  section  = "right",
  priority = 101,
  on_click = function()
    return { navigate = "foo_screen" }
  end,
  widget = {
    kind     = "button",
    on_click = "click",
    height   = 32,
    style = {
      text_color       = "#e1e5f4",
      background_hover = "#20222f",
      border           = { width = 1, radius = 6, color = "#242635" },
    },
    child = {
      kind   = "padding",
      top    = 6,
      right  = 10,
      bottom = 6,
      left   = 10,
      child  = { kind = "text", value = "Foo", size = 12, color = "#e1e5f4" },
    },
  },
}

leviathan.ui.register_screen({
  id = "foo_screen",
  init = function()
    return { clicks = 0 }
  end,
  view = function(state)
    return {
      kind   = "padding",
      top = 16, right = 16, bottom = 16, left = 16,
      width = "fill", height = "fill",
      child = {
        kind    = "column",
        spacing = 12,
        children = {
          {
            kind = "row",
            spacing = 12,
            children = {
              std_button("← Back to repository", "back"),
              { kind = "text", value = "Foo plugin screen", size = 18, color = COLOR_TEXT },
            },
          },
          {
            kind = "container",
            child = {
              kind = "resizable_split",
              id = "main",
              direction = "vertical",
              children = {
                panel("Foo", "#12131c", { min_height = 100, max_height = 400 }),
                panel("Bar", "#191a25", { min_height = 150 }),
                panel("Baz", "#0e0f16", { min_height = 120 }),
              },
            },
          },
        },
      },
    }
  end,
  update = function(state, event, value)
    if event == "back" then
      return { navigate = "repository" }
    end
    return { state = state }
  end,
})

leviathan.log("foo_demo plugin loaded")

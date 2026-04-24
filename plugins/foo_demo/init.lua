-- Foo Demo plugin
-- Contributes a "Foo" main-bar button that opens a 3-panel screen.
--
-- Style rule: padding is always a standalone `padding` widget.

local COLOR_TEXT   = "#e1e5f4"
local COLOR_PANEL  = "#191a25"
local COLOR_HOVER  = "#20222f"
local COLOR_BORDER = "#242635"

local function std_button(label, event)
  return {
    kind     = "button",
    on_click = event,
    style = {
      background       = COLOR_PANEL,
      background_hover = COLOR_HOVER,
      text_color       = COLOR_TEXT,
      border           = { width = 1, radius = 4, color = COLOR_BORDER },
    },
    child = {
      kind = "padding",
      top = 6, right = 12, bottom = 6, left = 12,
      child = { kind = "text", value = label, size = 14, color = COLOR_TEXT },
    },
  }
end

local function panel(label, bg, opts)
  opts = opts or {}
  return {
    kind = "container",
    bg = bg,
    min_width = opts.min_width,
    max_width = opts.max_width,
    min_height = opts.min_height,
    max_height = opts.max_height,
    center_x = true,
    center_y = true,
    child = { kind = "text", value = label, size = 24, color = COLOR_TEXT },
  }
end

leviathan.ui.main_bar.add {
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

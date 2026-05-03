local TEXT_PRIMARY   = "#e1e5f4"
local TEXT_SECONDARY = "#8b90a5"
local TEXT_DIM       = "#585d6e"
local BG_HOVER       = "#20222f"
local TOOLBAR_HEIGHT = 50
local FONT_XS        = 10

leviathan.ui.regions.add_slot {
  region   = "main_bar",
  id       = "plugin.terminal.terminal",
  section  = "center",
  priority = 60,
  on_click = function()
  end,
  widget = {
    kind     = "button",
    on_click = "click",
    height   = TOOLBAR_HEIGHT,
    style = {
      text_color       = TEXT_PRIMARY,
      background_hover = BG_HOVER,
    },
    child = {
      kind   = "padding",
      top    = 6,
      right  = 12,
      bottom = 6,
      left   = 12,
      child = {
        kind    = "column",
        width   = "shrink",
        height  = "shrink",
        spacing = 3,
        align_x = "center",
        children = {
          { kind = "icon", path = "icons/terminal-2.svg", size = 20, color = TEXT_SECONDARY },
          { kind = "text", value = "Terminal",            size = FONT_XS, color = TEXT_DIM },
        },
      },
    },
  },
}

leviathan.log("terminal plugin loaded")

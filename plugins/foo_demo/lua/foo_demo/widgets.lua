-- Reusable widget builders for the foo_demo plugin.
--
-- Phase 5 demonstrates plugin packages: helpers live in
-- `lua/foo_demo/widgets.lua` and are reached from `init.lua` (and
-- `after/plugin/*.lua`) via `require("foo_demo.widgets")`.

local M = {}

local COLOR_TEXT   = "#e1e5f4"
local COLOR_PANEL  = "#191a25"
local COLOR_HOVER  = "#20222f"
local COLOR_BORDER = "#242635"

M.COLOR_TEXT = COLOR_TEXT

function M.std_button(label, event)
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

function M.panel(label, bg, opts)
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

return M

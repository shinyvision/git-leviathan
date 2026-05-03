-- File Explorer plugin for Git Leviathan.
-- Demonstrates the full plugin surface: fs.* Lua API, icon/scrollable/
-- mouse_area widgets, stateful navigation, destructive-op confirmation.
--
-- Style rule: padding is always a standalone `padding` widget — no widget
-- on its own carries a `padding` field. Buttons/columns/rows/containers
-- wrap padded children explicitly.

local fs  = leviathan.fs
local ui  = leviathan.ui
local log = leviathan.log

local COLOR_BG         = "#171822"
local COLOR_PANEL      = "#191a25"
local COLOR_HEADER     = "#101119"
local COLOR_ROW_SEL    = "#1f3262"
local COLOR_TEXT       = "#e1e5f4"
local COLOR_TEXT_DIM   = "#8b90a5"
local COLOR_TEXT_MUTED = "#585d6e"
local COLOR_DANGER     = "#e64d4d"
local COLOR_FOLDER     = "#faa33d"
local COLOR_FILE       = "#8b90a5"
local COLOR_BORDER     = "#242635"
local COLOR_HOVER      = "#20222f"

local function fmt_size(bytes)
  if bytes < 1024           then return string.format("%d B",   bytes)             end
  if bytes < 1048576        then return string.format("%.1f KB", bytes / 1024)      end
  if bytes < 1073741824     then return string.format("%.1f MB", bytes / 1048576)   end
  return string.format("%.2f GB", bytes / 1073741824)
end

local function load_entries(path)
  local entries, err = fs.list_dir(path)
  if not entries then
    log("file_explorer: list_dir failed for " .. tostring(path) .. ": " .. tostring(err))
    return {}
  end
  return entries
end

local function txt(value, size, color)
  return { kind = "text", value = value, size = size or 14, color = color or COLOR_TEXT }
end

local function icon(path, size, color)
  return { kind = "icon", path = path, size = size or 16, color = color }
end

-- Pad child on all sides by `all` pixels, or asymmetrically via a table
-- ({top, right, bottom, left}, optionally with width/height for the pad
-- wrapper itself).
local function pad(sides, child)
  if type(sides) == "number" then
    return {
      kind = "padding",
      top = sides, right = sides, bottom = sides, left = sides,
      child = child,
    }
  end
  return {
    kind   = "padding",
    top    = sides.top    or sides.vertical   or 0,
    right  = sides.right  or sides.horizontal or 0,
    bottom = sides.bottom or sides.vertical   or 0,
    left   = sides.left   or sides.horizontal or 0,
    width  = sides.width,
    height = sides.height,
    child  = child,
  }
end

-- Standard in-screen text button — same look the widget-tree button used
-- to bake in, now declared explicitly.
local function std_button(label, on_click_event)
  return {
    kind     = "button",
    on_click = on_click_event or "",
    style = {
      background       = COLOR_PANEL,
      background_hover = COLOR_HOVER,
      text_color       = COLOR_TEXT,
      border           = { width = 1, radius = 4, color = COLOR_BORDER },
    },
    child = pad(
      { top = 6, right = 12, bottom = 6, left = 12 },
      { kind = "text", value = label, size = 14, color = COLOR_TEXT }
    ),
  }
end

local function breadcrumb(path)
  return {
    kind  = "container",
    width = "fill", height = "shrink",
    bg    = COLOR_HEADER,
    child = pad(
      { top = 10, right = 14, bottom = 10, left = 14, width = "fill", height = "shrink" },
      {
        kind    = "row",
        width   = "fill", height = "shrink",
        spacing = 10,
        children = {
          std_button("← Repository", "nav_back"),
          std_button("↑ Parent",     "nav_up"),
          { kind = "space", width = 6 },
          txt(path, 13, COLOR_TEXT_DIM),
        },
      }
    ),
  }
end

local function file_row(entry, is_selected)
  local ic = entry.is_dir
    and icon("icons/folder.svg", 16, COLOR_FOLDER)
    or  icon("icons/file.svg",   16, COLOR_FILE)
  local label = entry.name .. (entry.is_dir and "/" or "")
  local size_label = entry.is_dir and "" or fmt_size(entry.size)
  local bg = is_selected and COLOR_ROW_SEL or nil

  return {
    kind     = "mouse_area",
    on_click = "open",
    value    = { path = entry.path, is_dir = entry.is_dir, name = entry.name },
    child = {
      kind  = "container",
      width = "fill", height = "shrink",
      bg    = bg,
      child = pad(
        { top = 6, right = 14, bottom = 6, left = 14, width = "fill", height = "shrink" },
        {
          kind    = "row",
          width   = "fill", height = "shrink",
          spacing = 10,
          children = {
            ic,
            txt(label, 14, COLOR_TEXT),
            { kind = "space", width = "fill" },
            txt(size_label, 12, COLOR_TEXT_MUTED),
          },
        }
      ),
    },
  }
end

local function file_list(state)
  local rows = {}
  for i, entry in ipairs(state.entries) do
    rows[i] = file_row(entry, entry.path == state.selected)
  end
  if #rows == 0 then
    rows[1] = {
      kind     = "container",
      width    = "fill", height = "shrink",
      center_x = true,
      child = pad(24, txt("(empty directory)", 13, COLOR_TEXT_MUTED)),
    }
  end
  return {
    kind  = "scrollable",
    width = "fill", height = "fill",
    child = {
      kind     = "column",
      width    = "fill", height = "shrink",
      spacing  = 0,
      children = rows,
    },
  }
end

local function footer(state)
  local has_selection = state.selected ~= nil
  local selected_label = state.selected or "—"
  local delete_btn = std_button("Delete", has_selection and "delete_req" or "")
  return {
    kind  = "container",
    width = "fill", height = "shrink",
    bg    = COLOR_HEADER,
    child = pad(
      { top = 10, right = 14, bottom = 10, left = 14, width = "fill", height = "shrink" },
      {
        kind    = "row",
        width   = "fill", height = "shrink",
        spacing = 10,
        children = {
          txt("Selected:", 12, COLOR_TEXT_MUTED),
          txt(selected_label, 12, COLOR_TEXT),
          { kind = "space", width = "fill" },
          delete_btn,
        },
      }
    ),
  }
end

local function confirm_modal(path)
  return {
    kind     = "container",
    width    = "fill", height = "fill",
    bg       = COLOR_PANEL,
    center_x = true, center_y = true,
    child = pad(24, {
      kind     = "column",
      width    = "shrink", height = "shrink",
      spacing  = 14,
      children = {
        txt("Confirm delete", 18, COLOR_DANGER),
        txt(path, 13, COLOR_TEXT_DIM),
        txt("This cannot be undone.", 12, COLOR_TEXT_MUTED),
        {
          kind    = "row",
          spacing = 10,
          children = {
            std_button("Cancel",  "delete_cancel"),
            std_button("Confirm", "delete_confirm"),
          },
        },
      },
    }),
  }
end

ui.regions.add_slot {
  region   = "main_bar",
  id       = "plugin.file_explorer.files",
  section  = "right",
  priority = 100,
  on_click = function()
    return { navigate = "explorer" }
  end,
  widget = {
    kind     = "button",
    on_click = "click",
    height   = 32,
    style = {
      text_color       = COLOR_TEXT,
      background_hover = COLOR_HOVER,
      border           = { width = 1, radius = 6, color = COLOR_BORDER },
    },
    child = pad(
      { top = 6, right = 10, bottom = 6, left = 10 },
      { kind = "text", value = "Files", size = 12, color = COLOR_TEXT }
    ),
  },
}

ui.register_screen({
  id = "explorer",
  init = function()
    local home = fs.home() or "/"
    return {
      current_path   = home,
      entries        = load_entries(home),
      selected       = nil,
      confirm_delete = nil,
    }
  end,
  view = function(state)
    local body
    if state.confirm_delete then
      body = confirm_modal(state.confirm_delete)
    else
      body = {
        kind  = "container",
        width = "fill", height = "fill",
        bg    = COLOR_BG,
        child = file_list(state),
      }
    end
    return {
      kind     = "column",
      width    = "fill", height = "fill",
      spacing  = 0,
      children = { breadcrumb(state.current_path), body, footer(state) },
    }
  end,
  update = function(state, event, value)
    if event == "nav_back" then
      return { navigate = "repository" }

    elseif event == "nav_up" then
      local parent = fs.parent(state.current_path)
      if parent then
        state.current_path = parent
        state.entries      = load_entries(parent)
        state.selected     = nil
      end
      return { state = state }

    elseif event == "open" then
      if value and value.is_dir then
        state.current_path = value.path
        state.entries      = load_entries(value.path)
        state.selected     = nil
      elseif value then
        state.selected = value.path
      end
      return { state = state }

    elseif event == "delete_req" then
      if state.selected then
        state.confirm_delete = state.selected
      end
      return { state = state }

    elseif event == "delete_cancel" then
      state.confirm_delete = nil
      return { state = state }

    elseif event == "delete_confirm" then
      if state.confirm_delete then
        local ok, err = fs.delete(state.confirm_delete)
        if not ok then
          log("file_explorer: delete failed: " .. tostring(err))
        end
        state.confirm_delete = nil
        state.selected       = nil
        state.entries        = load_entries(state.current_path)
      end
      return { state = state }
    end

    return { state = state }
  end,
})

log("file_explorer plugin loaded")

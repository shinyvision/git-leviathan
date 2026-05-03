local ui = leviathan.ui
local command = leviathan.command
local log = leviathan.log

local OVERLAY_ID = "command_palette"
local PLUGIN_ID = "command_palette"

local C = {
  backdrop = "#000000AA",
  panel = "#151927",
  panel_2 = "#171a26",
  blue = "#284ea8",
  blue_2 = "#3567d6",
  text = "#e1e5f4",
  dim = "#8b90a5",
  muted = "#585d6e",
  border = "#2d3450",
  danger = "#e64d4d",
}

local state = {
  query = "",
}

local function lower(s)
  return string.lower(tostring(s or ""))
end

local function trim(s)
  return tostring(s or ""):match("^%s*(.-)%s*$")
end

local function pad(spec, child)
  if type(spec) == "number" then
    return {
      kind = "padding",
      top = spec,
      right = spec,
      bottom = spec,
      left = spec,
      child = child,
    }
  end
  spec.child = child
  spec.kind = "padding"
  return spec
end

local function txt(value, size, color)
  return {
    kind = "text",
    value = value or "",
    size = size or 13,
    color = color or C.text,
  }
end

local function command_map()
  local map = {}
  for _, item in ipairs(command.list()) do
    map[item.name] = item
  end
  return map
end

local function split_command_line(line)
  local tokens = {}
  local buf = ""
  local quote = nil
  local escaped = false
  line = tostring(line or "")

  local function flush()
    if #buf > 0 then
      tokens[#tokens + 1] = buf
      buf = ""
    end
  end

  for i = 1, #line do
    local ch = string.sub(line, i, i)
    if escaped then
      buf = buf .. ch
      escaped = false
    elseif ch == "\\" then
      escaped = true
    elseif quote then
      if ch == quote then
        quote = nil
      else
        buf = buf .. ch
      end
    elseif ch == "'" or ch == '"' then
      quote = ch
    elseif string.match(ch, "%s") then
      flush()
    else
      buf = buf .. ch
    end
  end

  if escaped then
    buf = buf .. "\\"
  end
  if quote then
    return nil, "unterminated quote"
  end
  flush()
  return tokens, nil
end

local function arg_type(spec)
  return tostring((spec and (spec.type or spec.ty)) or "string")
end

local function convert_arg(spec, token)
  local ty = arg_type(spec)
  if ty == "boolean" then
    local v = lower(token)
    if v == "true" or v == "1" or v == "yes" or v == "on" then
      return true, nil
    end
    if v == "false" or v == "0" or v == "no" or v == "off" then
      return false, nil
    end
    return nil, "expected boolean for " .. tostring(spec.name)
  elseif ty == "integer" then
    local n = tonumber(token)
    if not n or n ~= math.floor(n) then
      return nil, "expected integer for " .. tostring(spec.name)
    end
    return n, nil
  elseif ty == "number" then
    local n = tonumber(token)
    if not n then
      return nil, "expected number for " .. tostring(spec.name)
    end
    return n, nil
  elseif string.sub(ty, 1, 5) == "enum:" then
    local allowed = string.sub(ty, 6)
    for value in string.gmatch(allowed, "([^,]+)") do
      if token == value then
        return token, nil
      end
    end
    return nil, "expected one of " .. allowed .. " for " .. tostring(spec.name)
  end
  return tostring(token), nil
end

local function build_args(item, tokens)
  local specs = item.args or {}
  local spec_by_name = {}
  for _, spec in ipairs(specs) do
    spec_by_name[spec.name] = spec
  end

  local args = {}
  local positional = {}
  for _, token in ipairs(tokens) do
    local eq = string.find(token, "=", 1, true)
    if eq and eq > 1 then
      local key = string.sub(token, 1, eq - 1)
      local raw = string.sub(token, eq + 1)
      local spec = spec_by_name[key]
      if not spec then
        return nil, "unknown argument " .. key
      end
      local value, err = convert_arg(spec, raw)
      if err then
        return nil, err
      end
      args[key] = value
    else
      positional[#positional + 1] = token
    end
  end

  local pos = 1
  for _, spec in ipairs(specs) do
    if args[spec.name] == nil and positional[pos] ~= nil then
      local value, err = convert_arg(spec, positional[pos])
      if err then
        return nil, err
      end
      args[spec.name] = value
      pos = pos + 1
    end
  end

  if positional[pos] ~= nil then
    return nil, "too many arguments"
  end
  return args, nil
end

local function syntax_for(item)
  local parts = { item.name }
  for _, spec in ipairs(item.args or {}) do
    local name = tostring(spec.name)
    if spec.required then
      parts[#parts + 1] = "<" .. name .. ">"
    else
      parts[#parts + 1] = "[" .. name .. "]"
    end
  end
  return table.concat(parts, " ")
end

local function command_rows()
  local query = lower(trim(state.query):match("^(%S+)") or "")
  local rows = {}
  local count = 0

  for _, item in ipairs(command.list()) do
    local syntax = syntax_for(item)
    local haystack = lower(syntax .. " " .. (item.title or "") .. " " .. (item.description or ""))
    if query == "" or string.find(haystack, query, 1, true) then
      count = count + 1
      rows[#rows + 1] = {
        kind = "button",
        on_click = "run",
        value = { line = item.name },
        width = "fill",
        height = "shrink",
        style = {
          background = count == 1 and "#1d2d5a" or C.panel_2,
          background_hover = "#22396f",
          text_color = C.text,
          border = { width = 1, radius = 6, color = count == 1 and C.blue_2 or C.border },
        },
        child = pad({ top = 10, right = 12, bottom = 10, left = 12, width = "fill" }, {
          kind = "column",
          spacing = 2,
          width = "fill",
          height = "shrink",
          children = {
            txt(item.name, 14, C.text),
            txt(syntax, 11, item.destructive and C.danger or C.dim),
          },
        }),
      }
    end
    if count >= 8 then
      break
    end
  end

  if #rows == 0 then
    rows[1] = pad(14, txt("No matching commands", 13, C.muted))
  end

  return rows
end

local function execute_line(line)
  line = trim(line)
  if line == "" then
    return
  end
  local tokens, split_err = split_command_line(line)
  if split_err then
    log("command_palette: " .. split_err)
    return
  end
  local name = tokens[1]
  if not name or name == "" then
    return
  end
  table.remove(tokens, 1)

  local item = command_map()[name]
  if not item then
    log("command_palette: unknown command: " .. tostring(name))
    return
  end

  local args, arg_err = build_args(item, tokens)
  if arg_err then
    log("command_palette: " .. arg_err)
    return
  end

  local ok = command.invoke(name, args or {})
  if not ok then
    log("command_palette: command failed or rejected: " .. tostring(name))
  end
end

local function execute_first()
  local rows = command_rows()
  local first = rows[1]
  if first and first.value and first.value.line then
    execute_line(first.value.line)
  end
end

local function close()
  ui.remove_overlay(OVERLAY_ID)
  state.query = ""
end

local function dismiss_area(width, height)
  return {
    kind = "mouse_area",
    on_click = "dismiss",
    child = {
      kind = "space",
      width = width or "fill",
      height = height or "fill",
    },
  }
end

local function palette_panel()
  return {
    kind = "container",
    width = 620,
    height = 500,
    bg = C.panel,
    child = pad({ top = 14, right = 14, bottom = 14, left = 14, width = "fill", height = "fill" }, {
      kind = "column",
      spacing = 10,
      width = "fill",
      height = "shrink",
      children = {
        {
          kind = "container",
          width = "fill",
          height = "shrink",
          bg = C.blue,
          child = pad({ top = 8, right = 8, bottom = 8, left = 8, width = "fill", height = "shrink" }, {
            kind = "text_input",
            id = "palette-query",
            placeholder = "command args",
            value = state.query,
            on_input = "query",
            on_submit = "submit",
            width = "fill",
            height = 38,
            autofocus = true,
            style = {
              background = C.blue,
              text_color = "#ffffff",
              placeholder_color = "#b8c8ff",
              border = { width = 1, radius = 8, color = C.blue_2 },
            },
          }),
        },
        {
          kind = "scrollable",
          width = "fill",
          height = 394,
          child = {
            kind = "column",
            spacing = 6,
            width = "fill",
            height = "shrink",
            children = command_rows(),
          },
        },
      },
    }),
  }
end

local function render()
  ui.overlay({
    id = OVERLAY_ID,
    priority = 1000,
    dismissible = true,
    on_event = function(_, event, value)
      if event == "dismiss" or event == "escape" then
        close()
      elseif event == "query" then
        state.query = tostring(value or "")
        render()
      elseif event == "submit" then
        if trim(state.query) == "" then
          execute_first()
        else
          execute_line(state.query)
        end
        close()
      elseif event == "run" then
        execute_line(value and value.line)
        close()
      end
    end,
    widget = {
      kind = "container",
      width = "fill",
      height = "fill",
      bg = C.backdrop,
      child = {
        kind = "column",
        width = "fill",
        height = "fill",
        spacing = 0,
        children = {
          dismiss_area("fill", "fill"),
          {
            kind = "row",
            width = "fill",
          height = 500,
            spacing = 0,
            children = {
              dismiss_area("fill", 500),
              palette_panel(),
              dismiss_area("fill", 500),
            },
          },
          dismiss_area("fill", "fill"),
        },
      },
    },
  })
end

command.create("command_palette.open", {
  title = "Command Palette: Open",
  description = "Open the command palette.",
  run = function()
    state.query = ""
    render()
  end,
})

leviathan.keymap.set("global", ":", "command_palette.open", {
  description = "Open command palette",
})

leviathan.keymap.set("repository", ":", "command_palette.open", {
  description = "Open command palette",
})

log("command_palette plugin loaded")

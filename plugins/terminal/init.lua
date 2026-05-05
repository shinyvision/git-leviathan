local TEXT_PRIMARY = "#e1e5f4"
local TEXT_SECONDARY = "#8b90a5"
local TEXT_DIM = "#585d6e"
local BG_HOVER = "#20222f"
local BG_TERMINAL = "#080a0f"
local ACCENT_RED = "#ff6b6b"
local FONT_XS = 10
local FONT_SM = 12
local TOOLBAR_HEIGHT = 50
local PANEL_HEIGHT = 280

local state = {
  tabs = {},
}

local function repo_cwd()
  local repo = leviathan.repository or {}
  if repo.workdir_path and repo.workdir_path ~= "" then
    return repo.workdir_path
  end
  return nil
end

local function tab_key()
  local registry = leviathan.tab_registry or {}
  local current = registry.current or {}
  if current.path and current.path ~= "" then
    return current.path
  end
  return repo_cwd() or "__no_repository__"
end

local function tab_state()
  local key = tab_key()
  local tab = state.tabs[key]
  if not tab then
    tab = {
      open = false,
      session = nil,
      error = nil,
    }
    state.tabs[key] = tab
  end
  return tab
end

local function text_node(value, color, size)
  return { kind = "text", value = value, size = size or FONT_SM, color = color or TEXT_PRIMARY }
end

local function ensure_session(tab)
  if tab.session then
    return true
  end
  local session, err = leviathan.shell.open({
    cwd = repo_cwd(),
    rows = 24,
    cols = 100,
  })
  if err then
    tab.error = err
    return false
  end
  tab.session = session
  tab.error = nil
  return true
end

local function terminal_widget()
  local tab = tab_state()
  if tab.session and not leviathan.shell.is_running(tab.session) then
    leviathan.shell.close(tab.session)
    tab.session = nil
    tab.open = false
  end
  if not tab.open then
    return { kind = "space", height = 0 }
  end
  if not ensure_session(tab) then
    return {
      kind = "container",
      height = PANEL_HEIGHT,
      width = "fill",
      bg = BG_TERMINAL,
      child = {
        kind = "padding",
        top = 12,
        right = 12,
        bottom = 12,
        left = 12,
        child = text_node(tab.error or "terminal unavailable", ACCENT_RED, FONT_SM),
      },
    }
  end
  return {
    kind = "container",
    height = PANEL_HEIGHT,
    width = "fill",
    bg = BG_TERMINAL,
    child = {
      kind = "terminal",
      session = tab.session,
      width = "fill",
      height = "fill",
      font_size = 13,
    },
  }
end

local function handle_event(_, event)
  if event == "toggle" then
    local tab = tab_state()
    tab.open = not tab.open
  end
end

leviathan.ui.slot.add {
  region = "main_bar",
  id = "plugin.terminal.terminal",
  section = "center",
  priority = 60,
  on_click = handle_event,
  widget = {
    kind = "button",
    on_click = "toggle",
    height = TOOLBAR_HEIGHT,
    style = {
      text_color = TEXT_PRIMARY,
      background_hover = BG_HOVER,
    },
    child = {
      kind = "padding",
      top = 6,
      right = 12,
      bottom = 6,
      left = 12,
      child = {
        kind = "column",
        width = "shrink",
        height = "shrink",
        spacing = 3,
        align_x = "center",
        children = {
          { kind = "icon", path = "icons/terminal-2.svg", size = 20, color = TEXT_SECONDARY },
          { kind = "text", value = "Terminal", size = FONT_XS, color = TEXT_DIM },
        },
      },
    },
  },
}

leviathan.ui.slot.add {
  region = "repository",
  pane = "graph",
  section = "bottom",
  id = "plugin.terminal.panel",
  priority = 100,
  on_click = handle_event,
  widget = terminal_widget,
  depends_on = { "plugin_state", "repository", "tab" },
}

leviathan.log("terminal plugin loaded")

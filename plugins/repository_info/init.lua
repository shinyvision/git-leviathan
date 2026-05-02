local TEXT_DIM = "#585d6e"
local TEXT_PRIMARY = "#e1e5f4"
local TEXT_ACTIVE_BRANCH = "#60d87e"
local BORDER = "#242535"
local FONT_XS = 10
local FONT_SM = 11

local function info_column(label, value, value_color)
  return {
    kind = "padding",
    top = 0,
    right = 10,
    bottom = 0,
    left = 10,
    child = {
      kind = "column",
      width = "shrink",
      height = "shrink",
      children = {
        { kind = "text", value = label, size = FONT_XS, color = TEXT_DIM },
        { kind = "text", value = value, size = FONT_SM, color = value_color },
      },
    },
  }
end

leviathan.ui.main_bar.replace(
  { section = "left", id = "builtin.repo_info" },
  {
  id = "plugin.repository_info.repository_info",
  section = "left",
  priority = 10,
  widget = function()
    local repo = leviathan.repository or {}
    local repo_name = repo.name or ""
    local branch_name = repo.current_branch_name
    if branch_name == nil or branch_name == "" then
      local current = repo.current_branch
      branch_name = (current and current.name) or ""
    end

    return {
      kind = "row",
      width = "shrink",
      height = "shrink",
      align_y = "center",
      children = {
        info_column("repository", repo_name, TEXT_PRIMARY),
        { kind = "icon", path = "icons/chevron-right.svg", size = 20, color = BORDER },
        info_column("branch", branch_name, TEXT_ACTIVE_BRANCH),
      },
    }
  end,
  }
)

leviathan.ui.main_bar.remove({ section = "left", id = "builtin.separator_chevron" })
leviathan.ui.main_bar.remove({ section = "left", id = "builtin.branch_info" })

leviathan.log("repository_info plugin loaded")

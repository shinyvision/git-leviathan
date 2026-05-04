assert(leviathan.ui.slot.add({
  region = "main_bar",
  section = "center",
  id = "example.status_bar.branch",
  priority = 200,
  widget = function(ctx)
    local branch = ctx.repository.current_branch_name or "no branch"
    return { kind = "text", value = "Status: " .. branch, size = 12 }
  end,
  depends_on = { "repository" },
}))

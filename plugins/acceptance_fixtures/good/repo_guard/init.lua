_G.branch_ok = -1
_G.branch_err = ""
_G.reset_ok = -1
_G.reset_err = ""

leviathan.command.create("repo_guard.create_branch", {
  title = "Repo Guard Create Branch",
  description = "Create a guarded branch.",
  context = "repository",
  args = {
    { name = "name", type = "string", required = false, default = "repo_guard_default" },
  },
  run = function(args)
    local name = (args and args.name) or "repo_guard_default"
    local ok, err = leviathan.git.create_branch({ name = name })
    _G.branch_ok = ok and 1 or 0
    _G.branch_err = err or ""
  end,
})

leviathan.command.create("repo_guard.reset_hard", {
  title = "Repo Guard Reset Hard",
  description = "Exercise destructive confirmation.",
  context = "repository",
  destructive = true,
  args = {
    { name = "ref", type = "string", required = true },
  },
  run = function(args)
    local ok, err = leviathan.git.reset({ ref = args.ref, mode = "hard" })
    _G.reset_ok = ok and 1 or 0
    _G.reset_err = err or ""
  end,
})

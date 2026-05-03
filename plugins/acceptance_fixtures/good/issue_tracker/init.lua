leviathan.services.register("issue_tracker", 1, {
  lookup = function(commit)
    return "ISSUE-" .. string.sub(commit, 1, 6)
  end,
})

leviathan.health.register(function(ctx)
  ctx:ok("issue tracker service ready")
end)

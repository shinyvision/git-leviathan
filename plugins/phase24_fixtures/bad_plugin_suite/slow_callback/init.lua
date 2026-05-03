leviathan.autocmd.create("BranchChanged", {
  callback = function(_ev)
    _G.slow_callback_seen = 1
  end,
})

_G.activated = "yes"
_G.lazy_runs = _G.lazy_runs or 0
_G.lazy_events = _G.lazy_events or 0

leviathan.command.create("lazy_demo.run", {
  title = "Lazy Demo Run",
  description = "Activate and run the lazy demo.",
  context = "global",
  run = function(_args)
    _G.lazy_runs = _G.lazy_runs + 1
  end,
})

leviathan.keymap.set("global", "<leader>d", "lazy_demo.run", {
  description = "Run lazy demo",
})

leviathan.autocmd.create("FetchStarted", {
  callback = function(_ev)
    _G.lazy_events = _G.lazy_events + 1
  end,
})

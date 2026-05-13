local labels = require("commit_lens.labels")
local tracker = leviathan.services.get("issue_tracker", 1)
local issue = tracker.lookup("abc1234")
local store = leviathan.persist.open("lens", { version = 1 })

_G.command_runs = _G.command_runs or 0
_G.keymap_runs = _G.keymap_runs or 0
_G.commit_events = _G.commit_events or 0
_G.async_done = _G.async_done or 0
_G.timer_fires = _G.timer_fires or 0
_G.watch_fires = _G.watch_fires or 0
_G.issue = issue

store:set("last_issue", issue)
leviathan.settings.define_schema({
  enabled = { type = "boolean", default = true },
  refreshes = { type = "integer", default = 0, min = 0, max = 1000 },
})
leviathan.settings.set({ enabled = true, refreshes = store:get("refreshes") or 0 })

leviathan.ui.graph_decoration({ hash = "abc1234" }, {
  id = "commit-lens.issue",
  kind = "badge",
  text = labels.badge(issue),
  fg = "#ffffff",
  bg = "#305c7a",
})

leviathan.ui.slot.add({
  region = "repository",
  pane = "details",
  section = "top",
  id = "plugin.commit_lens.annotation",
  priority = 15,
  widget = function()
    return { kind = "text", value = "Lens " .. tostring(store:get("last_issue") or "none") }
  end,
})

leviathan.command.create("commit_lens.refresh", {
  title = "Commit Lens Refresh",
  description = "Refresh commit lens annotations.",
  context = "repository",
  args = {
    { name = "from_keymap", type = "boolean", required = false, default = false },
  },
  run = function(args)
    _G.command_runs = _G.command_runs + 1
    if args and args.from_keymap then
      _G.keymap_runs = _G.keymap_runs + 1
    end
    leviathan.async.spawn(
      function(_ctx)
        return 24
      end,
      function(ok, value)
        if ok then
          _G.async_done = value
          store:set("refreshes", value)
          leviathan.settings.set({ refreshes = value })
        end
      end
    )
  end,
})

leviathan.keymap.set("repository", "gl", "commit_lens.refresh", {
  description = "Refresh commit lens",
  args = { from_keymap = true },
})

leviathan.autocmd.create("CommitSelected", {
  callback = function(ev)
    _G.commit_events = _G.commit_events + 1
    _G.last_commit = ev.payload and ev.payload.commit and ev.payload.commit.hash or ""
  end,
})

leviathan.timer.every(10, function()
  _G.timer_fires = _G.timer_fires + 1
end)

local labels_source = leviathan.runtime.find("commit_lens.labels").source
local watch_path = leviathan.fs.join(
  leviathan.fs.parent(leviathan.fs.parent(leviathan.fs.parent(labels_source))),
  "watched.txt"
)
leviathan.fs.watch(watch_path, { recursive = false }, function(_ev)
  _G.watch_fires = _G.watch_fires + 1
end)

leviathan.health.register(function(ctx)
  ctx:ok("commit lens ready")
end)

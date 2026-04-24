-- Dancing banana plugin. Replaces the built-in fetch indicator so the
-- main bar shows a dancing banana GIF while refs are being fetched and
-- falls back to the Tabler refresh icon when idle.
--
-- Pattern: autocmd callbacks mutate plugin-local Lua state; the slot's
-- widget is a function that re-reads that state on each refresh. The
-- host re-invokes the function whenever any of this plugin's autocmds
-- fire, so the UI stays a pure function of `fetching`.

local TEXT_DIM = "#585d6e"

local fetching = false

leviathan.api.create_autocmd({ "FetchStart" }, {
  callback = function()
    fetching = true
  end,
})

leviathan.api.create_autocmd({ "FetchEnd" }, {
  callback = function()
    fetching = false
  end,
})

leviathan.ui.main_bar.replace("builtin.fetch_indicator", {
  id = "plugin.dancing_banana_test.fetch_indicator",
  section = "left",
  priority = 40,
  widget = function()
    if fetching then
      return {
        kind = "padding",
        top = 0,
        right = 0,
        bottom = 0,
        left = 0,
        child = {
          kind = "image",
          path = "assets/dancing_banana.gif",
          size = 25,
        },
      }
    else
      return {
        kind = "padding",
        top = 0,
        right = 6,
        bottom = 0,
        left = 6,
        child = {
          kind = "icon",
          path = "assets/refresh.svg",
          size = 13,
          color = TEXT_DIM,
        },
      }
    end
  end,
})

leviathan.log("dancing_banana_test plugin loaded")

local TEXT_DIM = "#585d6e"

local fetching = false

leviathan.autocmd.create("FetchStarted", {
  callback = function()
    fetching = true
  end,
})

leviathan.autocmd.create("FetchFinished", {
  callback = function()
    fetching = false
  end,
})

leviathan.ui.regions.replace_slot(
  { region = "main_bar", section = "left", id = "builtin.fetch_indicator" },
  {
  region = "main_bar",
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
  }
)

leviathan.log("dancing_banana_test plugin loaded")

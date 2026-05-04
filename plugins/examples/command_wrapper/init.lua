leviathan.command.create("example_command_wrapper.refresh", {
  title = "Example: Refresh Through Wrapper",
  description = "Runs a built-in command through a plugin command.",
  run = function()
    return leviathan.command.invoke("repository.refresh", {})
  end,
})

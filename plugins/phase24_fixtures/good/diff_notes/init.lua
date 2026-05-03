_G.note_actions = _G.note_actions or 0

leviathan.command.create("diff_notes.add", {
  title = "Add Diff Note",
  description = "Attach a note to the selected diff line.",
  context = "repository",
  run = function(_args)
    _G.note_actions = _G.note_actions + 1
  end,
})

leviathan.ui.diff_decoration({
  id = "diff-notes.line",
  kind = "line_hint",
  severity = "info",
  text = "tracked by diff_notes",
  file = "src/main.rs",
  line = 7,
})

leviathan.ui.context_menu("repository.diff.context_menu", {
  id = "diff-notes.add",
  label = "Add note",
  command = "diff_notes.add",
  priority = 12,
})

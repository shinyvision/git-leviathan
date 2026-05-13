local function focus_bar(ctx)
  if not ctx.focus.matches_pane then
    return nil
  end
  return {
    kind = "container",
    width = "fill",
    height = 2,
    bg = "#4f8dff",
  }
end

for _, point in ipairs({
  "repository.sidebar.chrome",
  "repository.graph.chrome",
  "repository.details.chrome",
  "repository.diff.chrome",
}) do
  assert(leviathan.ui.contribute(point, {
    id = "example.focus_indicator.bar",
    widget = focus_bar,
  }))
end

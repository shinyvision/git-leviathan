leviathan.async.spawn(function(ctx)
  for _ = 1, 1000000 do
    if ctx:cancelled() then
      break
    end
  end
  return 0
end)

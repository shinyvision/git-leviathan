local store = leviathan.persist.open("counter", { version = 1 })
local n = store:get("n") or 0
store:set("n", n + 1)
leviathan.log(string.format("counter is now %d", n + 1))

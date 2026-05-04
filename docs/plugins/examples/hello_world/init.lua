local handle, err = leviathan.ui.slot.add{
    region = "main_bar",
    id = "hello_world.greeting",
    section = "left",
    priority = 100,
    widget = { kind = "text", value = "Hello, plugin world!" },
}
assert(handle, err)

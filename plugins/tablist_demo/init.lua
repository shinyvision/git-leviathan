leviathan.autocmd.create({ "TabAdded", "TabRemoved", "TabSwitched", "TabReordered" }, {
	callback = function(event)
		leviathan.log("tablist_demo: " .. event)
	end,
})

leviathan.ui.regions.replace_slot(
	{ region = "tab_bar", section = "center", id = "builtin.tab_list" },
	{
	region = "tab_bar",
	id = "plugin.tablist_demo.tab_list",
	section = "center",
	priority = 10,
	widget = function()
		local list = leviathan.tab_registry.list or {}
		local current = leviathan.tab_registry.current
		local tabs = {}
		for _, t in ipairs(list) do
			table.insert(tabs, { id = t.path, name = t.name })
		end
		return {
			kind = "tablist",
			tabs = tabs,
			active = current and current.path or nil,
			orderable = true,
			on_select = "select",
			on_close = "close",
			on_reorder = "reorder",
		}
	end,
	on_click = function(_slot_id, event, value)
		if event == "select" then
			leviathan.tab_registry.select(value)
		elseif event == "close" then
			leviathan.tab_registry.remove(value)
		elseif event == "reorder" then
			leviathan.tab_registry.reorder(value)
		end
	end,
	}
)

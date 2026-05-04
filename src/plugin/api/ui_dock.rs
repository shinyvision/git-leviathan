use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Table};

use super::{BuildState, DockPanelDef};
use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::dock::DockArea;
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};

pub fn install(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    ui: &Table,
) -> mlua::Result<()> {
    let dock = lua.create_table()?;
    dock.set(
        "register",
        lua.create_function(move |lua_inner, spec: Table| {
            match register_panel(lua_inner, &build, &ledger, &guard, spec) {
                Ok(handle) => Ok((Some(handle), None::<String>)),
                Err(err) => Ok((None::<Table>, Some(err))),
            }
        })?,
    )?;
    ui.set("dock", dock)?;
    Ok(())
}

fn register_panel(
    lua: &Lua,
    build: &Rc<RefCell<BuildState>>,
    ledger: &ResourceLedger,
    guard: &CapabilityGuard,
    spec: Table,
) -> Result<Table, String> {
    let id: String = spec.get("id").map_err(|e| e.to_string())?;
    let title: String = spec.get("title").map_err(|e| e.to_string())?;
    let area_raw: String = spec.get("area").map_err(|e| e.to_string())?;
    let area = DockArea::parse(&area_raw)?;
    let default_open = spec
        .get::<Option<bool>>("default_open")
        .map_err(|e| e.to_string())?
        .unwrap_or(false);
    let view: Function = spec.get("view").map_err(|e| e.to_string())?;
    let update: Option<Function> = spec.get("update").map_err(|e| e.to_string())?;
    let source = ResourceLedger::source_location(lua);
    guard
        .check_named_for_target(
            "ui:dock",
            &format!("dock:{id}"),
            "leviathan.ui.dock.register",
            source.as_deref(),
            "Declare and grant `ui:dock`.",
        )
        .map_err(|e| e.to_string())?;

    let handle = format!("dock:{id}");
    ledger.remove_by_kind_handle(PluginResourceKind::DockPanel, &handle);
    ledger.remove_by_handle_prefix(&format!("{handle}:"));
    ledger.record(
        PluginResourceKind::DockPanel,
        handle.clone(),
        source.clone(),
    );
    let view_key = lua.create_registry_value(view).map_err(|e| e.to_string())?;
    ledger.record(
        PluginResourceKind::LuaRegistryKey,
        format!("{handle}:view"),
        source.clone(),
    );
    let update_key = update
        .map(|f| {
            let key = lua.create_registry_value(f).map_err(|e| e.to_string())?;
            ledger.record(
                PluginResourceKind::LuaRegistryKey,
                format!("{handle}:update"),
                source.clone(),
            );
            Ok::<_, String>(key)
        })
        .transpose()?;

    build.borrow_mut().dock_panels.insert(
        id.clone(),
        DockPanelDef {
            id: id.clone(),
            title: title.clone(),
            area,
            default_open,
            view: view_key,
            update: update_key,
            source_location: source,
        },
    );
    make_handle(lua, ledger.plugin_id().as_str(), &id, &title, area).map_err(|e| e.to_string())
}

fn make_handle(
    lua: &Lua,
    plugin_id: &str,
    id: &str,
    title: &str,
    area: DockArea,
) -> mlua::Result<Table> {
    let handle = lua.create_table()?;
    handle.set("plugin_id", plugin_id)?;
    handle.set("id", id)?;
    handle.set("key", crate::plugin::dock::panel_key(plugin_id, id))?;
    handle.set("title", title)?;
    handle.set("area", area.as_str())?;
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::capability_grants::{DecidedBy, Decision, GrantStore};
    use git_leviathan_plugin_api::capability::Capability;
    use mlua::Lua;
    use std::path::PathBuf;

    fn guard() -> Rc<CapabilityGuard> {
        let store = GrantStore::new_in_memory();
        store
            .record_decision(
                "p",
                "0.1.0",
                "ui:dock",
                Decision::Allow,
                DecidedBy::Default,
                None,
            )
            .unwrap();
        let root = PathBuf::from("/tmp/git-leviathan-dock-test");
        Rc::new(CapabilityGuard::new(
            "p",
            "0.1.0",
            vec![Capability::try_from("ui:dock".to_string()).unwrap()],
            crate::plugin::capabilities::CapabilityPaths {
                plugin_root: root.clone(),
                state_dir: root.clone(),
                config_dir: root,
                workdir: None,
            },
            store,
        ))
    }

    #[test]
    fn register_panel_captures_callbacks() {
        let lua = Lua::new();
        let build = Rc::new(RefCell::new(BuildState::default()));
        let ledger =
            ResourceLedger::new("p".into(), crate::plugin::resources::GenerationId::new(1));
        let spec: Table = lua
            .load(
                r#"return {
                    id = "panel", title = "Panel", area = "right",
                    default_open = true,
                    view = function(ctx) return { kind = "text", value = ctx.type } end,
                    update = function(state) return { state = state } end,
                }"#,
            )
            .eval()
            .unwrap();
        let handle = register_panel(&lua, &build, &ledger, &guard(), spec).unwrap();
        assert_eq!(handle.get::<String>("key").unwrap(), "p:panel");
        assert!(build.borrow().dock_panels.contains_key("panel"));
    }
}

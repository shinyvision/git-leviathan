//! `leviathan.ui` hooks and screen registration.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Table};

use super::{BuildState, ScreenDef, SettingsPanelDef};
use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::diagnostic::DiagnosticStore;
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};

pub fn install(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    diagnostics: DiagnosticStore,
    ui_context: crate::plugin::ui::context::UiContextStore,
    leviathan: &Table,
) -> mlua::Result<()> {
    let ui = lua.create_table()?;
    super::ui_core::install(
        lua,
        Rc::clone(&build),
        ledger.clone(),
        Rc::clone(&guard),
        diagnostics.clone(),
        ui_context,
        &ui,
    )?;
    install_settings_register(lua, Rc::clone(&build), ledger.clone(), &ui)?;
    install_screen_register(lua, build, ledger, guard, &ui)?;
    leviathan.set("ui", ui)?;
    Ok(())
}

fn install_settings_register(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    ui: &Table,
) -> mlua::Result<()> {
    let settings = lua.create_table()?;
    settings.set(
        "register",
        lua.create_function(move |lua_inner, spec: Table| {
            let view: Function = spec.get("view")?;
            let source = ResourceLedger::source_location(lua_inner);
            let key = lua_inner.create_registry_value(view)?;
            ledger.remove_by_handle_prefix("settings_panel");
            ledger.record(
                PluginResourceKind::LuaRegistryKey,
                "settings_panel:view",
                source.clone(),
            );
            build.borrow_mut().settings_panel = Some(SettingsPanelDef {
                view: key,
                source_location: source,
            });
            Ok(())
        })?,
    )?;
    ui.set("settings", settings)?;
    Ok(())
}

fn install_screen_register(
    lua: &Lua,
    build: Rc<RefCell<BuildState>>,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    ui: &Table,
) -> mlua::Result<()> {
    let screen = lua.create_table()?;
    screen.set(
        "register",
        lua.create_function(move |lua_inner, spec: Table| {
            let id: String = spec.get("id")?;
            let title: Option<String> = spec.get("title")?;
            let breadcrumbs: Option<Vec<String>> = spec.get("breadcrumbs")?;
            let bind_repository: Option<bool> = spec.get("bind_repository")?;
            let source = ResourceLedger::source_location(lua_inner);
            guard
                .check_named_for_target(
                    "ui:screen",
                    &format!("screen:{id}"),
                    "leviathan.ui.screen.register",
                    source.as_deref(),
                    "Declare and grant `ui:screen`.",
                )
                .map_err(mlua::Error::external)?;
            let init: Function = spec.get("init")?;
            let view: Function = spec.get("view")?;
            let update: Function = spec.get("update")?;
            let serialize_opt: Option<Function> = spec.get("serialize")?;
            let deserialize_opt: Option<Function> = spec.get("deserialize")?;
            let can_close_opt: Option<Function> = spec.get("can_close")?;
            let prefix = format!("screen:{id}");
            ledger.remove_by_handle_prefix(&prefix);
            ledger.record(PluginResourceKind::Screen, prefix.clone(), source.clone());
            let def = ScreenDef {
                id: id.clone(),
                title: title.unwrap_or_else(|| id.clone()),
                breadcrumbs: breadcrumbs.unwrap_or_default(),
                bind_repository: bind_repository.unwrap_or(false),
                init: register_screen_key(
                    lua_inner,
                    &ledger,
                    &prefix,
                    "init",
                    init,
                    source.clone(),
                )?,
                view: register_screen_key(
                    lua_inner,
                    &ledger,
                    &prefix,
                    "view",
                    view,
                    source.clone(),
                )?,
                update: register_screen_key(
                    lua_inner,
                    &ledger,
                    &prefix,
                    "update",
                    update,
                    source.clone(),
                )?,
                serialize: serialize_opt
                    .map(|f| {
                        register_screen_key(
                            lua_inner,
                            &ledger,
                            &prefix,
                            "serialize",
                            f,
                            source.clone(),
                        )
                    })
                    .transpose()?,
                deserialize: deserialize_opt
                    .map(|f| {
                        register_screen_key(
                            lua_inner,
                            &ledger,
                            &prefix,
                            "deserialize",
                            f,
                            source.clone(),
                        )
                    })
                    .transpose()?,
                can_close: can_close_opt
                    .map(|f| {
                        register_screen_key(
                            lua_inner,
                            &ledger,
                            &prefix,
                            "can_close",
                            f,
                            source.clone(),
                        )
                    })
                    .transpose()?,
            };
            build.borrow_mut().screens.insert(id, def);
            Ok(())
        })?,
    )?;
    ui.set("screen", screen)?;
    Ok(())
}

fn register_screen_key(
    lua: &Lua,
    ledger: &ResourceLedger,
    prefix: &str,
    name: &str,
    function: Function,
    source: Option<String>,
) -> mlua::Result<mlua::RegistryKey> {
    let key = lua.create_registry_value(function)?;
    ledger.record(
        PluginResourceKind::LuaRegistryKey,
        format!("{prefix}:{name}"),
        source,
    );
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::capability_grants::{DecidedBy, Decision, GrantStore};
    use git_leviathan_plugin_api::capability::Capability;
    use mlua::Lua;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    fn install_test_harness() -> (Lua, Rc<RefCell<BuildState>>) {
        let lua = Lua::new();
        let build = Rc::new(RefCell::new(BuildState::default()));
        let leviathan = lua.create_table().unwrap();
        let ledger = ResourceLedger::new(
            "test".into(),
            crate::plugin::resources::GenerationId::new(1),
        );
        let store = GrantStore::new_in_memory();
        for cap in ["ui:region:main_bar", "ui:screen"] {
            store
                .record_decision(
                    "test",
                    "0.1.0",
                    cap,
                    Decision::Allow,
                    DecidedBy::Default,
                    None,
                )
                .unwrap();
        }
        let root = PathBuf::from("/tmp/git-leviathan-ui-test");
        let guard = Rc::new(CapabilityGuard::new(
            "test",
            "0.1.0",
            vec![
                Capability::try_from("ui:region:main_bar".to_string()).unwrap(),
                Capability::try_from("ui:screen".to_string()).unwrap(),
            ],
            crate::plugin::capabilities::CapabilityPaths {
                plugin_root: root.clone(),
                state_dir: root.clone(),
                config_dir: root,
                workdir: None,
            },
            store,
        ));
        super::install(
            &lua,
            Rc::clone(&build),
            ledger,
            guard,
            DiagnosticStore::default(),
            crate::plugin::ui::context::UiContextStore::new(
                "test",
                crate::plugin::resources::GenerationId::new(1),
            ),
            &leviathan,
        )
        .unwrap();
        lua.globals().set("leviathan", leviathan).unwrap();
        (lua, build)
    }

    #[test]
    fn direct_region_handles_are_not_installed() {
        let (lua, _) = install_test_harness();
        let ok: bool = lua
            .load(r#"return leviathan.ui["main_bar"] == nil and leviathan.ui.region ~= nil and leviathan.ui.slot ~= nil"#)
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn list_regions_returns_all() {
        let (lua, _) = install_test_harness();
        let names: Vec<String> = lua
            .load("local names = assert(leviathan.ui.region.list()); return names")
            .eval()
            .unwrap();
        assert_eq!(names, vec!["main_bar", "tab_bar", "repository"]);
    }

    #[test]
    fn slot_api_adds_slot() {
        let (lua, build) = install_test_harness();
        lua.load(
            r#"
            assert(leviathan.ui.slot.add{
                region = "main_bar",
                section = "left",
                id = "x",
                priority = 0,
                widget = { kind = "text", value = "hi" },
            })
        "#,
        )
        .exec()
        .unwrap();
        assert_eq!(build.borrow().slot_ops.len(), 1);
    }
}

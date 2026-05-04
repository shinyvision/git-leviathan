use git_leviathan_plugin_api::descriptor::region::RegionDescriptor;
use mlua::{Function, Lua, LuaSerdeExt, Table, Value as LuaValue};

use crate::plugin::resources::{PluginResourceKind, ResourceLedger};
use crate::plugin::ui::invalidation::{default_dependencies_for_region, UiDependency};
use crate::plugin::ui::widget_ast;

use super::{DynamicWidgetCall, RawSlotSpec, WidgetSource};

pub(super) fn read_address_with_id(
    t: &Table,
    desc: &RegionDescriptor,
) -> mlua::Result<(Option<String>, String, String)> {
    let pane: Option<String> = t.get("pane")?;
    let section: String = t.get("section")?;
    let id: String = t.get("id")?;
    desc.validate_address(pane.as_deref(), Some(&section))
        .map_err(mlua::Error::external)?;
    Ok((pane, section, id))
}

pub(super) fn compose_container(pane: Option<&str>, section: &str) -> String {
    match pane {
        Some(p) => format!("{p}.{section}"),
        None => section.to_string(),
    }
}

pub(super) fn slot_handle(region: &str, container: &str, id: &str) -> String {
    format!("{region}:{container}:{id}")
}

pub(super) fn read_spec(
    lua: &Lua,
    spec: &Table,
    desc: &RegionDescriptor,
    ledger: &ResourceLedger,
    dynamic_call: DynamicWidgetCall,
) -> mlua::Result<RawSlotSpec> {
    let id: String = spec.get("id")?;
    let pane: Option<String> = spec.get("pane")?;
    let section: String = spec.get("section")?;
    desc.validate_address(pane.as_deref(), Some(&section))
        .map_err(mlua::Error::external)?;
    let priority: i32 = spec.get("priority")?;
    let container = compose_container(pane.as_deref(), &section);
    let handle = slot_handle(desc.name, &container, &id);
    let source_location = ResourceLedger::source_location(lua);
    let depends_on = read_dependencies(spec, desc.name)?;
    ledger.remove_by_kind_handle(PluginResourceKind::Slot, &handle);
    ledger.record(
        PluginResourceKind::Slot,
        handle.clone(),
        source_location.clone(),
    );
    let widget = read_widget(
        lua,
        spec,
        ledger,
        &handle,
        source_location.clone(),
        dynamic_call,
    )?;
    let on_click_fn: Option<Function> = spec.get("on_click")?;
    let on_click = on_click_fn
        .map(|f| {
            let key = lua.create_registry_value(f)?;
            ledger.record(
                PluginResourceKind::LuaRegistryKey,
                format!("{handle}:on_click"),
                source_location.clone(),
            );
            Ok::<mlua::RegistryKey, mlua::Error>(key)
        })
        .transpose()?;
    Ok(RawSlotSpec {
        id,
        region: desc.name.to_string(),
        container,
        priority,
        widget,
        depends_on,
        on_click,
        source_location,
    })
}

fn read_dependencies(spec: &Table, region: &str) -> mlua::Result<Vec<UiDependency>> {
    let raw: Option<Table> = spec.get("depends_on")?;
    let Some(table) = raw else {
        return Ok(default_dependencies_for_region(region));
    };
    let mut out = Vec::new();
    for value in table.sequence_values::<String>() {
        let value = value?;
        let Some(dep) = UiDependency::parse(&value) else {
            return Err(mlua::Error::external(format!(
                "unknown UI dependency `{value}`"
            )));
        };
        if !out.contains(&dep) {
            out.push(dep);
        }
    }
    if out.is_empty() {
        return Err(mlua::Error::external("depends_on must not be empty"));
    }
    Ok(out)
}

fn read_widget(
    lua: &Lua,
    spec: &Table,
    ledger: &ResourceLedger,
    slot_handle: &str,
    source_location: Option<String>,
    dynamic_call: DynamicWidgetCall,
) -> mlua::Result<WidgetSource> {
    let v: LuaValue = spec.get("widget")?;
    Ok(match v {
        LuaValue::Function(f) => {
            let key = lua.create_registry_value(f)?;
            ledger.record(
                PluginResourceKind::LuaRegistryKey,
                format!("{slot_handle}:widget"),
                source_location,
            );
            WidgetSource::Dynamic {
                key,
                call: dynamic_call,
            }
        }
        other => {
            let json: serde_json::Value = lua
                .from_value(other)
                .map_err(|e| mlua::Error::external(format!("invalid widget tree: {e}")))?;
            // Decode the static tree into the typed `WidgetAst`. Field-
            // level errors (unknown kind, type mismatch, depth/node/string
            // limits) surface here at boundary time and abort the slot
            // registration so the plugin author sees a precise reason.
            let ast = widget_ast::decode(&json)
                .map_err(|e| mlua::Error::external(format!("invalid widget tree: {e}")))?;
            WidgetSource::Static(Box::new(ast))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_leviathan_plugin_api::descriptor::region::REGIONS;
    use mlua::Lua;

    fn ledger() -> ResourceLedger {
        ResourceLedger::new(
            "test".into(),
            crate::plugin::resources::GenerationId::new(1),
        )
    }

    #[test]
    fn read_spec_decodes_region_slot() {
        let lua = Lua::new();
        let desc = REGIONS.get("main_bar").unwrap();
        let spec: Table = lua
            .load(r#"return { id = "x", section = "left", priority = 0, widget = { kind = "text", text = "hi" } }"#)
            .eval()
            .unwrap();
        let raw = read_spec(&lua, &spec, desc, &ledger(), DynamicWidgetCall::Context).unwrap();
        assert_eq!(raw.id, "x");
        assert_eq!(raw.region, "main_bar");
        assert_eq!(raw.container, "left");
        assert!(raw
            .depends_on
            .contains(&crate::plugin::ui::invalidation::UiDependency::Repository));
    }

    #[test]
    fn read_spec_rejects_bad_section() {
        let lua = Lua::new();
        let desc = REGIONS.get("main_bar").unwrap();
        let spec: Table = lua
            .load(r#"return { id = "x", section = "nope", priority = 0, widget = { kind = "text", text = "hi" } }"#)
            .eval()
            .unwrap();
        let err = match read_spec(&lua, &spec, desc, &ledger(), DynamicWidgetCall::Context) {
            Ok(_) => panic!("expected invalid section to fail"),
            Err(err) => err.to_string(),
        };
        assert!(err.contains("unknown section 'nope'"), "got: {err}");
    }

    #[test]
    fn factory_rejects_unknown_widget_kind() {
        let lua = Lua::new();
        let desc = REGIONS.get("main_bar").unwrap();
        let spec: Table = lua
            .load(r#"return { id = "x", section = "left", priority = 0, widget = { kind = "rwo", value = "hi" } }"#)
            .eval()
            .unwrap();
        let err = match read_spec(&lua, &spec, desc, &ledger(), DynamicWidgetCall::Context) {
            Ok(_) => panic!("expected invalid widget to fail"),
            Err(err) => err.to_string(),
        };
        assert!(
            err.contains("invalid widget tree") || err.contains("unknown variant"),
            "got: {err}"
        );
    }

    #[test]
    fn content_region_requires_pane() {
        let lua = Lua::new();
        let desc = REGIONS.get("repository").unwrap();
        let spec: Table = lua
            .load(r#"return { id = "x", section = "top", priority = 0, widget = { kind = "text", text = "hi" } }"#)
            .eval()
            .unwrap();
        let err = match read_spec(&lua, &spec, desc, &ledger(), DynamicWidgetCall::Context) {
            Ok(_) => panic!("expected missing pane to fail"),
            Err(err) => err.to_string(),
        };
        assert!(err.contains("unknown pane"), "got: {err}");
    }
}

//! extension points `leviathan.ui.{overlay, context_menu, graph_decoration,
//! diff_decoration}` Lua APIs.
//!
//! Each registration funnels through `CapabilityGuard::check_named`
//! before recording into the host-side [`ExtensionRegistry`]. The
//! registry handle is cheap-clone (Rc-backed) and shared into the
//! plugin's Lua factory at install time.
//!
//! Registrations are also recorded against the plugin's
//! [`ResourceLedger`] so the standard unload path drops the
//! resource rows without us doing it ourselves.

use std::cell::RefCell;
use std::rc::Rc;

use git_leviathan_plugin_api::descriptor::decoration::{DiffDecoration, GraphDecoration};
use git_leviathan_plugin_api::descriptor::region::REGIONS;
use mlua::{Function, Lua, LuaSerdeExt, Table, Value as LuaValue};

use crate::plugin::capabilities::CapabilityGuard;
use crate::plugin::extensions::{
    ContextMenuItemRecord, DiffDecorationRecord, ExtensionRegistry, GraphDecorationRecord,
    OverlayCallbacks, OverlayRecord,
};
use crate::plugin::resources::{PluginResourceKind, ResourceLedger};
use crate::plugin::ui::widget_ast;

/// Mount the four extension points functions onto the existing `leviathan.ui`
/// table.
///
/// `ui` must be the table the host installed under `leviathan.ui`
/// — we extend it in-place rather than allocating a new table.
pub fn install(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
    overlay_callbacks: Rc<RefCell<OverlayCallbacks>>,
) -> mlua::Result<()> {
    install_overlay(
        lua,
        ui,
        ledger.clone(),
        Rc::clone(&guard),
        registry.clone(),
        Rc::clone(&overlay_callbacks),
    )?;
    install_remove_overlay(
        lua,
        ui,
        ledger.clone(),
        Rc::clone(&guard),
        registry.clone(),
        overlay_callbacks,
    )?;
    install_context_menu(lua, ui, ledger.clone(), Rc::clone(&guard), registry.clone())?;
    install_graph_decoration(lua, ui, ledger.clone(), Rc::clone(&guard), registry.clone())?;
    install_diff_decoration(lua, ui, ledger, guard, registry)?;
    Ok(())
}

fn install_overlay(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
    overlay_callbacks: Rc<RefCell<OverlayCallbacks>>,
) -> mlua::Result<()> {
    let plugin_id = ledger.plugin_id().as_str().to_string();
    ui.set(
        "overlay",
        lua.create_function(move |lua_inner, spec: Table| {
            guard
                .check_named("ui:overlay")
                .map_err(mlua::Error::external)?;
            let id: String = spec.get("id")?;
            let dismissible: bool = spec.get::<Option<bool>>("dismissible")?.unwrap_or(true);
            let priority: i32 = spec.get::<Option<i32>>("priority")?.unwrap_or(0);
            let widget_value: LuaValue = spec.get("widget")?;
            let widget_json: serde_json::Value = lua_inner
                .from_value(widget_value)
                .map_err(|e| mlua::Error::external(format!("invalid overlay widget: {e}")))?;
            let widget = widget_ast::decode(&widget_json)
                .map_err(|e| mlua::Error::external(format!("invalid overlay widget: {e}")))?;
            let source = ResourceLedger::source_location(lua_inner);
            let handle = format!("overlay:{id}");
            ledger.remove_by_kind_handle(PluginResourceKind::Overlay, &handle);
            ledger.record(PluginResourceKind::Overlay, handle, source.clone());
            overlay_callbacks.borrow_mut().remove(&id);
            let callback: Option<Function> = match spec.get::<Option<Function>>("on_event")? {
                Some(callback) => Some(callback),
                None => spec.get::<Option<Function>>("update")?,
            };
            if let Some(callback) = callback {
                let key = lua_inner.create_registry_value(callback)?;
                ledger.record(
                    PluginResourceKind::LuaRegistryKey,
                    format!("overlay:{id}:on_event"),
                    source.clone(),
                );
                overlay_callbacks.borrow_mut().insert(id.clone(), key);
            }
            registry.add_overlay(OverlayRecord {
                plugin_id: plugin_id.clone(),
                id,
                priority,
                dismissible,
                widget,
                source_location: source,
            });
            Ok(())
        })?,
    )?;
    Ok(())
}

fn install_remove_overlay(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
    overlay_callbacks: Rc<RefCell<OverlayCallbacks>>,
) -> mlua::Result<()> {
    let plugin_id = ledger.plugin_id().as_str().to_string();
    let remove = lua.create_function(move |_, id: String| {
        guard
            .check_named("ui:overlay")
            .map_err(mlua::Error::external)?;
        registry.remove_overlay(&plugin_id, &id);
        overlay_callbacks.borrow_mut().remove(&id);
        ledger.remove_by_kind_handle(PluginResourceKind::Overlay, &format!("overlay:{id}"));
        ledger.remove_by_kind_handle(
            PluginResourceKind::LuaRegistryKey,
            &format!("overlay:{id}:on_event"),
        );
        Ok(())
    })?;
    ui.set("remove_overlay", remove)?;
    Ok(())
}

fn install_context_menu(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
) -> mlua::Result<()> {
    let plugin_id = ledger.plugin_id().as_str().to_string();
    ui.set(
        "context_menu",
        lua.create_function(move |lua_inner, (region, item): (String, Table)| {
            guard
                .check_named("ui:context_menu")
                .map_err(mlua::Error::external)?;
            validate_context_menu_region(&region).map_err(mlua::Error::external)?;
            let id: String = item.get("id")?;
            let label: String = item.get("label")?;
            let command: String = item.get("command")?;
            let priority: i32 = item.get::<Option<i32>>("priority")?.unwrap_or(0);
            let condition_capability: Option<String> = item.get("condition_capability")?;
            let source = ResourceLedger::source_location(lua_inner);
            let handle = format!("context_menu:{region}:{id}");
            ledger.remove_by_handle_prefix(&handle);
            ledger.record(PluginResourceKind::ContextMenuItem, handle, source.clone());
            registry.add_context_menu_item(ContextMenuItemRecord {
                plugin_id: plugin_id.clone(),
                region,
                id,
                label,
                command,
                priority,
                condition_capability,
                source_location: source,
            });
            Ok(())
        })?,
    )?;
    Ok(())
}

fn install_graph_decoration(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
) -> mlua::Result<()> {
    let plugin_id = ledger.plugin_id().as_str().to_string();
    ui.set(
        "graph_decoration",
        lua.create_function(
            move |lua_inner, (commit_hash, decoration): (String, Table)| {
                guard
                    .check_named("ui:graph_decoration")
                    .map_err(mlua::Error::external)?;
                if commit_hash.is_empty() {
                    return Err(mlua::Error::external(
                        "graph_decoration: commit_hash must be a non-empty string",
                    ));
                }
                // Pull a stable id off the table or synthesise one from
                // (kind, commit_hash) so callers don't have to invent one
                // for the simple case.
                let explicit_id: Option<String> = decoration.get("id")?;
                let value: serde_json::Value = lua_inner
                    .from_value(LuaValue::Table(decoration))
                    .map_err(|e| mlua::Error::external(format!("invalid graph decoration: {e}")))?;
                let parsed: GraphDecoration = serde_json::from_value(value)
                    .map_err(|e| mlua::Error::external(format!("invalid graph decoration: {e}")))?;
                let id = explicit_id.unwrap_or_else(|| format!("{}:{commit_hash}", parsed.kind()));
                let source = ResourceLedger::source_location(lua_inner);
                let handle = format!("graph_decoration:{commit_hash}:{id}");
                ledger.remove_by_kind_handle(PluginResourceKind::GraphDecoration, &handle);
                ledger.record(PluginResourceKind::GraphDecoration, handle, source.clone());
                registry.add_graph_decoration(GraphDecorationRecord {
                    plugin_id: plugin_id.clone(),
                    id,
                    commit_hash,
                    decoration: parsed,
                    source_location: source,
                });
                Ok(())
            },
        )?,
    )?;
    Ok(())
}

fn install_diff_decoration(
    lua: &Lua,
    ui: &Table,
    ledger: ResourceLedger,
    guard: Rc<CapabilityGuard>,
    registry: ExtensionRegistry,
) -> mlua::Result<()> {
    let plugin_id = ledger.plugin_id().as_str().to_string();
    ui.set(
        "diff_decoration",
        lua.create_function(move |lua_inner, decoration: Table| {
            guard
                .check_named("ui:diff_decoration")
                .map_err(mlua::Error::external)?;
            let explicit_id: Option<String> = decoration.get("id")?;
            let value: serde_json::Value = lua_inner
                .from_value(LuaValue::Table(decoration))
                .map_err(|e| mlua::Error::external(format!("invalid diff decoration: {e}")))?;
            let parsed: DiffDecoration = serde_json::from_value(value)
                .map_err(|e| mlua::Error::external(format!("invalid diff decoration: {e}")))?;
            let id = explicit_id.unwrap_or_else(|| diff_decoration_default_id(&parsed));
            let source = ResourceLedger::source_location(lua_inner);
            let handle = format!("diff_decoration:{id}");
            ledger.remove_by_kind_handle(PluginResourceKind::DiffDecoration, &handle);
            ledger.record(PluginResourceKind::DiffDecoration, handle, source.clone());
            registry.add_diff_decoration(DiffDecorationRecord {
                plugin_id: plugin_id.clone(),
                id,
                decoration: parsed,
                source_location: source,
            });
            Ok(())
        })?,
    )?;
    Ok(())
}

fn diff_decoration_default_id(d: &DiffDecoration) -> String {
    match d {
        DiffDecoration::LineHint { file, line, .. } => format!("line_hint:{file}:{line}"),
        DiffDecoration::HunkBadge { hunk_id, .. } => format!("hunk_badge:{hunk_id}"),
        DiffDecoration::LineGutter { file, line, .. } => format!("line_gutter:{file}:{line}"),
    }
}

fn validate_context_menu_region(region: &str) -> Result<(), String> {
    // The address is a stack of dot-separated tokens. The region
    // table holds entries with multi-dot names (`repository.diff`,
    // `repository.graph`) so we resolve longest-prefix first: try
    // descending tail splits until one matches a region descriptor.
    use git_leviathan_plugin_api::descriptor::region::RegionKind;
    for split in (1..=region.matches('.').count() + 1).rev() {
        let (head, tail) = nth_dot_split(region, split);
        let Some(descriptor) = REGIONS.get(head) else {
            continue;
        };
        let tail = tail
            .ok_or_else(|| format!("expected '<region>.<pane?>.context_menu' (got '{region}')"))?;
        return match &descriptor.kind {
            RegionKind::Chrome { sections, .. } => {
                if tail != "context_menu" {
                    return Err(format!(
                        "context_menu only valid on a *.context_menu section (got '{tail}')"
                    ));
                }
                if !sections.contains(&"context_menu") {
                    return Err(format!("region '{head}' has no 'context_menu' section"));
                }
                Ok(())
            }
            RegionKind::Content { panes } => {
                // Allow either `<region>.<pane>.context_menu` or, when
                // there's exactly one pane, `<region>.context_menu`.
                let (pane, section) = match tail.split_once('.') {
                    Some((p, s)) => (p, s),
                    None if panes.len() == 1 => (panes[0].name, tail),
                    None => {
                        return Err(format!(
                            "expected '{head}.<pane>.context_menu' (got '{region}')"
                        ))
                    }
                };
                if section != "context_menu" {
                    return Err(format!(
                        "context_menu only valid on a *.context_menu section (got '{section}')"
                    ));
                }
                descriptor.validate_address(Some(pane), Some(section))
            }
        };
    }
    Err(format!("unknown region in '{region}'"))
}

/// Split `s` at the n-th `.` from the start. `n=1` returns
/// `("repository", Some("diff.context_menu"))`. Returns
/// `(s, None)` when the index exceeds the dot count.
fn nth_dot_split(s: &str, n: usize) -> (&str, Option<&str>) {
    let mut indices = s
        .char_indices()
        .filter_map(|(i, c)| (c == '.').then_some(i));
    match indices.nth(n - 1) {
        Some(idx) => (&s[..idx], Some(&s[idx + 1..])),
        None => (s, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_context_menu_accepts_known_addresses() {
        assert!(validate_context_menu_region("repository.diff.context_menu").is_ok());
        assert!(validate_context_menu_region("repository.graph.context_menu").is_ok());
    }

    #[test]
    fn validate_context_menu_rejects_unknown() {
        let err = validate_context_menu_region("repository.diff.toolbar").unwrap_err();
        assert!(err.contains("context_menu"), "got: {err}");

        let err = validate_context_menu_region("nope.context_menu").unwrap_err();
        assert!(err.contains("unknown region"), "got: {err}");
    }
}

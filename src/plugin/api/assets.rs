use std::path::{Component, Path};

use mlua::{Lua, Table};

use crate::plugin::resources::{PluginResourceKind, ResourceLedger};

const MAX_ASSET_PATH_LEN: usize = 256;

pub fn install(lua: &Lua, leviathan: &Table, ledger: ResourceLedger) -> mlua::Result<()> {
    let assets = lua.create_table()?;
    let load_svg_ledger = ledger.clone();
    assets.set(
        "load_svg",
        lua.create_function(move |lua_inner, path: String| {
            match asset_handle(lua_inner, &load_svg_ledger, "svg", &path) {
                Ok(handle) => Ok((Some(handle), None::<String>)),
                Err(err) => Ok((None::<Table>, Some(err))),
            }
        })?,
    )?;
    leviathan.set("assets", assets)?;
    Ok(())
}

fn asset_handle(
    lua: &Lua,
    ledger: &ResourceLedger,
    kind: &str,
    path: &str,
) -> Result<Table, String> {
    if !safe_asset_path(path) {
        return Err(format!("invalid plugin asset path: {path}"));
    }
    let handle = format!("asset:{kind}:{path}");
    ledger.record(
        PluginResourceKind::AssetHandle,
        handle.clone(),
        ResourceLedger::source_location(lua),
    );
    let table = lua.create_table().map_err(|e| e.to_string())?;
    table
        .set("__leviathan_asset_handle", handle.clone())
        .map_err(|e| e.to_string())?;
    table.set("handle", handle).map_err(|e| e.to_string())?;
    table.set("kind", kind).map_err(|e| e.to_string())?;
    table.set("path", path).map_err(|e| e.to_string())?;
    Ok(table)
}

fn safe_asset_path(path: &str) -> bool {
    if path.is_empty() || path.len() > MAX_ASSET_PATH_LEN || path.contains('\0') {
        return false;
    }
    Path::new(path)
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_escaping_asset_paths() {
        assert!(!safe_asset_path("../x.svg"));
        assert!(!safe_asset_path("/x.svg"));
        assert!(safe_asset_path("icons/x.svg"));
    }

    #[test]
    fn load_svg_returns_handle() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        let ledger = ResourceLedger::new(
            "assets".into(),
            crate::plugin::resources::GenerationId::new(1),
        );
        install(&lua, &leviathan, ledger.clone()).unwrap();
        lua.globals().set("leviathan", leviathan).unwrap();
        let path: String = lua
            .load(r#"local h, err = leviathan.assets.load_svg("icons/foo.svg"); assert(not err); return h.path"#)
            .eval()
            .unwrap();
        assert_eq!(path, "icons/foo.svg");
        assert!(
            ledger.contains_kind_handle(PluginResourceKind::AssetHandle, "asset:svg:icons/foo.svg")
        );
    }
}

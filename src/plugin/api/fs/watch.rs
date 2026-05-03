use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use mlua::{Function, Lua, Table};

use crate::plugin::capabilities::CapabilityGuard;

/// async runtime watch-handle userdata returned by `leviathan.fs.watch`.
/// `:cancel()` drops the OS watcher and the parked Lua callback so
/// further events for this watch_id stop dispatching.
pub struct LuaWatchHandle {
    pub watch_id: crate::plugin::watchers::WatchId,
    pub registry: crate::plugin::watchers::FileWatcherRegistry,
    pub callbacks: Rc<RefCell<crate::plugin::watchers::PluginWatcherCallbacks>>,
}

impl mlua::UserData for LuaWatchHandle {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("cancel", |_, this, ()| {
            this.registry.cancel(this.watch_id);
            this.callbacks.borrow_mut().remove(this.watch_id);
            Ok(())
        });
        methods.add_method("id", |_, this, ()| Ok(this.watch_id.get()));
    }
}

pub struct WatchInstallContext {
    pub guard: Rc<CapabilityGuard>,
    pub ledger: crate::plugin::resources::ResourceLedger,
    pub registry: crate::plugin::watchers::FileWatcherRegistry,
    pub callbacks: Rc<RefCell<crate::plugin::watchers::PluginWatcherCallbacks>>,
    pub plugin_id: crate::plugin::resources::PluginId,
    pub generation_id: crate::plugin::resources::GenerationId,
}

/// async runtime file-watch install. Mounts `leviathan.fs.watch(path, opts, cb)`
/// onto the existing fs table. Each call records a [`PluginResourceKind::FileWatcher`]
/// entry; cancellation flows through the returned userdata or
/// `cancel_for_generation`.
pub fn install_watch(lua: &Lua, leviathan: &Table, ctx: WatchInstallContext) -> mlua::Result<()> {
    use crate::plugin::resources::PluginResourceKind;

    let fs_tbl: Table = leviathan.get("fs")?;
    let WatchInstallContext {
        guard,
        ledger,
        registry,
        callbacks,
        plugin_id,
        generation_id,
    } = ctx;

    fs_tbl.set(
        "watch",
        lua.create_function(
            move |lua_inner, (path, opts, cb): (String, Option<Table>, Function)| {
                let recursive = opts
                    .as_ref()
                    .and_then(|t| t.get::<Option<bool>>("recursive").ok().flatten())
                    .unwrap_or(false);
                let path_buf = PathBuf::from(&path);
                guard
                    .check_fs_watch(&path_buf)
                    .map_err(mlua::Error::external)?;

                let watch_id = registry.allocate();
                let source = crate::plugin::resources::ResourceLedger::source_location(lua_inner);
                let resource_id = ledger.record(
                    PluginResourceKind::FileWatcher,
                    format!("fs.watch:{}", path),
                    source.clone(),
                );
                ledger.record(
                    PluginResourceKind::LuaRegistryKey,
                    format!("watch:{}", watch_id.get()),
                    source,
                );

                if let Err(e) = registry.register(
                    plugin_id.clone(),
                    generation_id,
                    watch_id,
                    resource_id,
                    path_buf,
                    recursive,
                ) {
                    ledger.remove_resource(resource_id);
                    return Err(mlua::Error::external(e));
                }

                let key = lua_inner.create_registry_value(cb)?;
                callbacks.borrow_mut().insert(watch_id, key);

                Ok(LuaWatchHandle {
                    watch_id,
                    registry: registry.clone(),
                    callbacks: Rc::clone(&callbacks),
                })
            },
        )?,
    )?;

    Ok(())
}

//! `leviathan.fs` returns `(value, nil)` on success and `(nil, err)` or
//! `(false, err)` on failure. File payloads are capped at 8 MiB.

use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use mlua::{Function, Lua, Table};

use crate::plugin::capabilities::CapabilityGuard;

pub fn install(lua: &Lua, leviathan: &Table, guard: Rc<CapabilityGuard>) -> mlua::Result<()> {
    let fs_tbl = lua.create_table()?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "list_dir",
        lua.create_function(move |lua_inner, path: String| {
            g.check_fs_read(Path::new(&path))
                .map_err(mlua::Error::external)?;
            match list_dir(&path) {
                Ok(entries) => {
                    let arr = lua_inner.create_table()?;
                    for (i, e) in entries.into_iter().enumerate() {
                        let t = lua_inner.create_table()?;
                        t.set("name", e.name)?;
                        t.set("path", e.path)?;
                        t.set("is_dir", e.is_dir)?;
                        t.set("is_symlink", e.is_symlink)?;
                        t.set("size", e.size)?;
                        t.set("modified", e.modified)?;
                        arr.set(i + 1, t)?;
                    }
                    Ok((Some(arr), None::<String>))
                }
                Err(e) => Ok((None, Some(e))),
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "delete",
        lua.create_function(move |_, path: String| {
            let p = Path::new(&path);
            g.check_fs_write(p).map_err(mlua::Error::external)?;
            let res = if p.is_dir() {
                fs::remove_dir_all(p)
            } else {
                fs::remove_file(p)
            };
            match res {
                Ok(()) => Ok((true, None::<String>)),
                Err(e) => Ok((false, Some(e.to_string()))),
            }
        })?,
    )?;

    fs_tbl.set(
        "parent",
        lua.create_function(|_, path: String| -> mlua::Result<Option<String>> {
            Ok(Path::new(&path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty()))
        })?,
    )?;

    fs_tbl.set(
        "basename",
        lua.create_function(|_, path: String| -> mlua::Result<Option<String>> {
            Ok(Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned()))
        })?,
    )?;

    fs_tbl.set(
        "stem",
        lua.create_function(|_, path: String| -> mlua::Result<Option<String>> {
            Ok(Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned()))
        })?,
    )?;

    fs_tbl.set(
        "extension",
        lua.create_function(|_, path: String| -> mlua::Result<Option<String>> {
            Ok(Path::new(&path)
                .extension()
                .map(|e| e.to_string_lossy().into_owned()))
        })?,
    )?;

    fs_tbl.set(
        "join",
        lua.create_function(|_, (a, b): (String, String)| -> mlua::Result<String> {
            Ok(Path::new(&a).join(&b).to_string_lossy().into_owned())
        })?,
    )?;

    fs_tbl.set(
        "relative_to",
        lua.create_function(
            |_, (path, base): (String, String)| -> mlua::Result<Option<String>> {
                Ok(Path::new(&path)
                    .strip_prefix(Path::new(&base))
                    .ok()
                    .map(|p| p.to_string_lossy().into_owned()))
            },
        )?,
    )?;

    fs_tbl.set(
        "with_extension",
        lua.create_function(
            |_, (path, ext): (String, String)| -> mlua::Result<Option<String>> {
                let p = Path::new(&path);
                if p.file_name().is_none() {
                    return Ok(None);
                }
                Ok(Some(p.with_extension(&ext).to_string_lossy().into_owned()))
            },
        )?,
    )?;

    fs_tbl.set(
        "with_file_name",
        lua.create_function(
            |_, (path, name): (String, String)| -> mlua::Result<Option<String>> {
                let p = Path::new(&path);
                if p.file_name().is_none() {
                    return Ok(None);
                }
                Ok(Some(p.with_file_name(&name).to_string_lossy().into_owned()))
            },
        )?,
    )?;

    fs_tbl.set(
        "home",
        lua.create_function(|_, ()| -> mlua::Result<Option<String>> {
            Ok(dirs::home_dir().map(|p| p.to_string_lossy().into_owned()))
        })?,
    )?;

    fs_tbl.set(
        "cwd",
        lua.create_function(|_, ()| match std::env::current_dir() {
            Ok(p) => Ok((Some(p.to_string_lossy().into_owned()), None::<String>)),
            Err(e) => Ok((None, Some(e.to_string()))),
        })?,
    )?;

    fs_tbl.set(
        "temp_dir",
        lua.create_function(|_, ()| -> mlua::Result<String> {
            Ok(std::env::temp_dir().to_string_lossy().into_owned())
        })?,
    )?;

    fs_tbl.set(
        "config_dir",
        lua.create_function(|_, ()| -> mlua::Result<Option<String>> {
            Ok(dirs::config_dir().map(|p| p.to_string_lossy().into_owned()))
        })?,
    )?;

    fs_tbl.set(
        "cache_dir",
        lua.create_function(|_, ()| -> mlua::Result<Option<String>> {
            Ok(dirs::cache_dir().map(|p| p.to_string_lossy().into_owned()))
        })?,
    )?;

    fs_tbl.set(
        "data_dir",
        lua.create_function(|_, ()| -> mlua::Result<Option<String>> {
            Ok(dirs::data_dir().map(|p| p.to_string_lossy().into_owned()))
        })?,
    )?;

    fs_tbl.set(
        "state_dir",
        lua.create_function(|_, ()| -> mlua::Result<Option<String>> {
            Ok(dirs::state_dir().map(|p| p.to_string_lossy().into_owned()))
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "exists",
        lua.create_function(move |_, path: String| -> mlua::Result<bool> {
            g.check_fs_read(Path::new(&path))
                .map_err(mlua::Error::external)?;
            Ok(Path::new(&path).exists())
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "is_dir",
        lua.create_function(move |_, path: String| -> mlua::Result<bool> {
            g.check_fs_read(Path::new(&path))
                .map_err(mlua::Error::external)?;
            Ok(Path::new(&path).is_dir())
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "is_file",
        lua.create_function(move |_, path: String| -> mlua::Result<bool> {
            g.check_fs_read(Path::new(&path))
                .map_err(mlua::Error::external)?;
            Ok(Path::new(&path).is_file())
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "is_symlink",
        lua.create_function(move |_, path: String| -> mlua::Result<bool> {
            g.check_fs_read(Path::new(&path))
                .map_err(mlua::Error::external)?;
            Ok(fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false))
        })?,
    )?;

    fs_tbl.set(
        "is_absolute",
        lua.create_function(|_, path: String| -> mlua::Result<bool> {
            Ok(Path::new(&path).is_absolute())
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "size",
        lua.create_function(move |_, path: String| {
            g.check_fs_read(Path::new(&path))
                .map_err(mlua::Error::external)?;
            match fs::symlink_metadata(&path) {
                Ok(m) => Ok((Some(m.len()), None::<String>)),
                Err(e) => Ok((None, Some(e.to_string()))),
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "modified",
        lua.create_function(move |_, path: String| {
            g.check_fs_read(Path::new(&path))
                .map_err(mlua::Error::external)?;
            match fs::symlink_metadata(&path) {
                Ok(m) => match m.modified() {
                    Ok(t) => match t.duration_since(UNIX_EPOCH) {
                        Ok(d) => Ok((Some(d.as_secs() as i64), None::<String>)),
                        Err(e) => Ok((None, Some(e.to_string()))),
                    },
                    Err(e) => Ok((None, Some(e.to_string()))),
                },
                Err(e) => Ok((None, Some(e.to_string()))),
            }
        })?,
    )?;

    fs_tbl.set(
        "canonicalize",
        lua.create_function(|_, path: String| match fs::canonicalize(&path) {
            Ok(p) => Ok((Some(p.to_string_lossy().into_owned()), None::<String>)),
            Err(e) => Ok((None, Some(e.to_string()))),
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "read_link",
        lua.create_function(move |_, path: String| {
            g.check_fs_read(Path::new(&path))
                .map_err(mlua::Error::external)?;
            match fs::read_link(&path) {
                Ok(p) => Ok((Some(p.to_string_lossy().into_owned()), None::<String>)),
                Err(e) => Ok((None, Some(e.to_string()))),
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    let plugin_root = guard.plugin_root().to_path_buf();
    fs_tbl.set(
        "read_file",
        lua.create_function(move |_, path: String| {
            g.check_fs_read(Path::new(&path))
                .map_err(mlua::Error::external)?;
            let resolved = resolve_for_read(&plugin_root, &path);
            match read_file(&resolved) {
                Ok(s) => Ok((Some(s), None::<String>)),
                Err(e) => Ok((None, Some(e))),
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    let plugin_root = guard.plugin_root().to_path_buf();
    fs_tbl.set(
        "read_lines",
        lua.create_function(move |lua_inner, path: String| {
            g.check_fs_read(Path::new(&path))
                .map_err(mlua::Error::external)?;
            let resolved = resolve_for_read(&plugin_root, &path);
            match read_file(&resolved) {
                Ok(s) => {
                    let arr = lua_inner.create_table()?;
                    for (i, line) in s.lines().enumerate() {
                        arr.set(i + 1, line)?;
                    }
                    Ok((Some(arr), None::<String>))
                }
                Err(e) => Ok((None, Some(e))),
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "mkdir",
        lua.create_function(move |_, path: String| {
            g.check_fs_write(Path::new(&path))
                .map_err(mlua::Error::external)?;
            match fs::create_dir_all(&path) {
                Ok(()) => Ok((true, None::<String>)),
                Err(e) => Ok((false, Some(e.to_string()))),
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "write_file",
        lua.create_function(move |_, (path, content): (String, String)| {
            g.check_fs_write(Path::new(&path))
                .map_err(mlua::Error::external)?;
            match write_file(&path, &content) {
                Ok(()) => Ok((true, None::<String>)),
                Err(e) => Ok((false, Some(e))),
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "rename",
        lua.create_function(move |_, (src, dst): (String, String)| {
            g.check_fs_write(Path::new(&src))
                .map_err(mlua::Error::external)?;
            g.check_fs_write(Path::new(&dst))
                .map_err(mlua::Error::external)?;
            match fs::rename(&src, &dst) {
                Ok(()) => Ok((true, None::<String>)),
                Err(e) => Ok((false, Some(e.to_string()))),
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "copy",
        lua.create_function(move |_, (src, dst): (String, String)| {
            g.check_fs_read(Path::new(&src))
                .map_err(mlua::Error::external)?;
            g.check_fs_write(Path::new(&dst))
                .map_err(mlua::Error::external)?;
            match copy_file(&src, &dst) {
                Ok(()) => Ok((true, None::<String>)),
                Err(e) => Ok((false, Some(e))),
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "metadata",
        lua.create_function(move |lua_inner, path: String| {
            g.check_fs_read(Path::new(&path))
                .map_err(mlua::Error::external)?;
            match metadata(&path) {
                Ok(e) => {
                    let t = lua_inner.create_table()?;
                    t.set("name", e.name)?;
                    t.set("path", e.path)?;
                    t.set("is_dir", e.is_dir)?;
                    t.set("is_symlink", e.is_symlink)?;
                    t.set("size", e.size)?;
                    t.set("modified", e.modified)?;
                    Ok((Some(t), None::<String>))
                }
                Err(e) => Ok((None, Some(e))),
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "append_file",
        lua.create_function(move |_, (path, content): (String, String)| {
            g.check_fs_write(Path::new(&path))
                .map_err(mlua::Error::external)?;
            match append_file(&path, &content) {
                Ok(()) => Ok((true, None::<String>)),
                Err(e) => Ok((false, Some(e))),
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    fs_tbl.set(
        "touch",
        lua.create_function(move |_, path: String| {
            g.check_fs_write(Path::new(&path))
                .map_err(mlua::Error::external)?;
            match touch(&path) {
                Ok(()) => Ok((true, None::<String>)),
                Err(e) => Ok((false, Some(e))),
            }
        })?,
    )?;

    leviathan.set("fs", fs_tbl)?;
    Ok(())
}

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

/// async runtime file-watch install. Mounts `leviathan.fs.watch(path, opts, cb)`
/// onto the existing fs table. Each call records a [`PluginResourceKind::FileWatcher`]
/// entry; cancellation flows through the returned userdata or
/// `cancel_for_generation`.
#[allow(clippy::too_many_arguments)]
pub fn install_watch(
    lua: &Lua,
    leviathan: &Table,
    guard: Rc<CapabilityGuard>,
    ledger: crate::plugin::resources::ResourceLedger,
    registry: crate::plugin::watchers::FileWatcherRegistry,
    callbacks: Rc<RefCell<crate::plugin::watchers::PluginWatcherCallbacks>>,
    plugin_id: crate::plugin::resources::PluginId,
    generation_id: crate::plugin::resources::GenerationId,
) -> mlua::Result<()> {
    use crate::plugin::resources::PluginResourceKind;

    let fs_tbl: Table = leviathan.get("fs")?;

    fs_tbl.set(
        "watch",
        lua.create_function(
            move |lua_inner, (path, opts, cb): (String, Option<Table>, Function)| {
                let recursive = opts
                    .as_ref()
                    .and_then(|t| t.get::<Option<bool>>("recursive").ok().flatten())
                    .unwrap_or(false);
                let path_buf = std::path::PathBuf::from(&path);
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

/// Resolve a plugin-supplied path against the plugin's root when the path
/// is relative. Absolute paths pass through unchanged. Used by `read_file`
/// / `read_lines` so plugins can name their bundled assets without
/// recomputing the plugin root every call.
fn resolve_for_read(plugin_root: &Path, path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        path.to_string()
    } else {
        plugin_root.join(p).to_string_lossy().into_owned()
    }
}

const READ_FILE_BYTE_LIMIT: u64 = 8 * 1024 * 1024;
const WRITE_FILE_BYTE_LIMIT: u64 = 8 * 1024 * 1024;
const APPEND_FILE_BYTE_LIMIT: u64 = 8 * 1024 * 1024;
const COPY_BYTE_LIMIT: u64 = 8 * 1024 * 1024;

fn read_file(path: &str) -> Result<String, String> {
    let p = Path::new(path);
    let meta = fs::metadata(p).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(format!("not a regular file: {path}"));
    }
    if meta.len() > READ_FILE_BYTE_LIMIT {
        return Err(format!(
            "file exceeds {READ_FILE_BYTE_LIMIT}-byte read_file limit: {path}"
        ));
    }
    fs::read_to_string(p).map_err(|e| e.to_string())
}

fn copy_file(src: &str, dst: &str) -> Result<(), String> {
    let meta = fs::symlink_metadata(src).map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() {
        return Err(format!("not a regular file: {src}"));
    }
    if meta.len() > COPY_BYTE_LIMIT {
        return Err(format!(
            "source exceeds {COPY_BYTE_LIMIT}-byte copy limit: {src}"
        ));
    }
    fs::copy(src, dst).map(|_| ()).map_err(|e| e.to_string())
}

fn write_file(path: &str, content: &str) -> Result<(), String> {
    if content.len() as u64 > WRITE_FILE_BYTE_LIMIT {
        return Err(format!(
            "content exceeds {WRITE_FILE_BYTE_LIMIT}-byte write_file limit: {path}"
        ));
    }
    fs::write(path, content).map_err(|e| e.to_string())
}

fn append_file(path: &str, content: &str) -> Result<(), String> {
    if content.len() as u64 > APPEND_FILE_BYTE_LIMIT {
        return Err(format!(
            "content exceeds {APPEND_FILE_BYTE_LIMIT}-byte append_file limit: {path}"
        ));
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(content.as_bytes()).map_err(|e| e.to_string())
}

fn touch(path: &str) -> Result<(), String> {
    let f = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|e| e.to_string())?;
    let times = fs::FileTimes::new().set_modified(SystemTime::now());
    f.set_times(times).map_err(|e| e.to_string())
}

#[cfg_attr(test, derive(Debug))]
struct Entry {
    name: String,
    path: String,
    is_dir: bool,
    is_symlink: bool,
    size: u64,
    modified: i64,
}

fn metadata(path: &str) -> Result<Entry, String> {
    let p = Path::new(path);
    let meta = fs::symlink_metadata(p).map_err(|e| e.to_string())?;
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_type = meta.file_type();
    let is_symlink = file_type.is_symlink();
    let is_dir = file_type.is_dir();
    let size = if is_dir { 0 } else { meta.len() };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok(Entry {
        name,
        path: path.to_string(),
        is_dir,
        is_symlink,
        size,
        modified,
    })
}

fn list_dir(path: &str) -> Result<Vec<Entry>, String> {
    let p = PathBuf::from(path);
    let rd = fs::read_dir(&p).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let full = entry.path().to_string_lossy().into_owned();
        let file_type = meta.file_type();
        let is_symlink = file_type.is_symlink();
        let is_dir = file_type.is_dir();
        let size = if is_dir { 0 } else { meta.len() };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(Entry {
            name,
            path: full,
            is_dir,
            is_symlink,
            size,
            modified,
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;
    use std::io::Write;

    fn install_unrestricted(lua: &Lua, leviathan: &Table) {
        use crate::plugin::capability_grants::{DecidedBy, Decision, GrantStore};
        use git_leviathan_plugin_api::capability::{Capability, FsScope};
        let store = GrantStore::new_in_memory();
        store
            .record_decision(
                "test",
                "0.1.0",
                "fs:read:any",
                Decision::Allow,
                DecidedBy::Default,
                None,
            )
            .unwrap();
        store
            .record_decision(
                "test",
                "0.1.0",
                "fs:write:any",
                Decision::Allow,
                DecidedBy::Default,
                None,
            )
            .unwrap();
        let guard = Rc::new(CapabilityGuard::new(
            "test",
            "0.1.0",
            vec![
                Capability::FsRead {
                    scope: FsScope::Any,
                },
                Capability::FsWrite {
                    scope: FsScope::Any,
                },
            ],
            std::env::temp_dir(),
            std::env::temp_dir(),
            std::env::temp_dir(),
            None,
            store,
        ));
        install(lua, leviathan, guard).unwrap();
    }

    #[test]
    fn list_dir_returns_sorted_entries() {
        let tmp = tempfile::tempdir().unwrap();
        stdfs::create_dir(tmp.path().join("zdir")).unwrap();
        stdfs::create_dir(tmp.path().join("adir")).unwrap();
        let mut f = stdfs::File::create(tmp.path().join("zfile.txt")).unwrap();
        f.write_all(b"hello").unwrap();
        stdfs::File::create(tmp.path().join("afile.txt")).unwrap();

        let entries = list_dir(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 4);
        // Dirs first, alphabetised within groups
        assert_eq!(entries[0].name, "adir");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "zdir");
        assert!(entries[1].is_dir);
        assert_eq!(entries[2].name, "afile.txt");
        assert!(!entries[2].is_dir);
        assert_eq!(entries[3].name, "zfile.txt");
        assert_eq!(entries[3].size, 5);
    }

    #[cfg(unix)]
    #[test]
    fn list_dir_flags_symlink_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("real.txt");
        stdfs::File::create(&target).unwrap();
        std::os::unix::fs::symlink(&target, tmp.path().join("link.txt")).unwrap();

        let entries = list_dir(tmp.path().to_str().unwrap()).unwrap();
        let link = entries.iter().find(|e| e.name == "link.txt").unwrap();
        let real = entries.iter().find(|e| e.name == "real.txt").unwrap();
        assert!(link.is_symlink);
        assert!(!link.is_dir);
        assert!(!real.is_symlink);
    }

    #[test]
    fn list_dir_errors_for_missing_path() {
        let err = list_dir("/definitely/does/not/exist").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn read_file_returns_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("note.txt");
        let mut f = stdfs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();
        let got = read_file(path.to_str().unwrap()).unwrap();
        assert_eq!(got, "hello world");
    }

    #[test]
    fn read_file_errors_for_missing_path() {
        let err = read_file("/definitely/does/not/exist").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn read_file_errors_for_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let err = read_file(tmp.path().to_str().unwrap()).unwrap_err();
        assert!(err.contains("not a regular file"));
    }

    fn basename(path: &str) -> Option<String> {
        Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    }

    #[test]
    fn basename_returns_last_component() {
        assert_eq!(basename("/home/user/file.txt").as_deref(), Some("file.txt"));
        assert_eq!(basename("/home/user/dir").as_deref(), Some("dir"));
        assert_eq!(basename("file.txt").as_deref(), Some("file.txt"));
    }

    #[test]
    fn basename_strips_trailing_separator() {
        assert_eq!(basename("/home/user/dir/").as_deref(), Some("dir"));
    }

    #[test]
    fn basename_returns_none_for_root() {
        assert_eq!(basename("/"), None);
    }

    #[test]
    fn basename_returns_none_for_empty_path() {
        assert_eq!(basename(""), None);
    }

    fn stem(path: &str) -> Option<String> {
        Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
    }

    #[test]
    fn stem_returns_filename_without_final_extension() {
        assert_eq!(stem("/home/user/file.txt").as_deref(), Some("file"));
        assert_eq!(stem("archive.tar.gz").as_deref(), Some("archive.tar"));
    }

    #[test]
    fn stem_returns_whole_name_when_no_extension() {
        assert_eq!(stem("/home/user/README").as_deref(), Some("README"));
    }

    #[test]
    fn stem_returns_dotfile_name_unchanged() {
        assert_eq!(stem(".gitignore").as_deref(), Some(".gitignore"));
    }

    #[test]
    fn stem_strips_trailing_separator() {
        assert_eq!(stem("/home/user/dir/").as_deref(), Some("dir"));
    }

    #[test]
    fn stem_returns_none_for_root() {
        assert_eq!(stem("/"), None);
    }

    #[test]
    fn stem_returns_none_for_empty_path() {
        assert_eq!(stem(""), None);
    }

    fn extension(path: &str) -> Option<String> {
        Path::new(path)
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
    }

    #[test]
    fn extension_returns_suffix_after_final_dot() {
        assert_eq!(extension("/home/user/file.txt").as_deref(), Some("txt"));
        assert_eq!(extension("archive.tar.gz").as_deref(), Some("gz"));
    }

    #[test]
    fn extension_returns_none_for_no_dot() {
        assert_eq!(extension("/home/user/README"), None);
    }

    #[test]
    fn extension_returns_none_for_dotfile() {
        assert_eq!(extension(".gitignore"), None);
    }

    #[test]
    fn extension_returns_none_for_directory_path() {
        assert_eq!(extension("/home/user/dir/"), None);
    }

    #[test]
    fn is_file_distinguishes_regular_files_from_directories() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("note.txt");
        stdfs::File::create(&file_path).unwrap();

        let file_str = file_path.to_str().unwrap();
        let dir_str = tmp.path().to_str().unwrap();
        let missing = "/definitely/does/not/exist";

        let got_file: bool = lua
            .load(format!("return leviathan.fs.is_file({file_str:?})"))
            .eval()
            .unwrap();
        let got_dir: bool = lua
            .load(format!("return leviathan.fs.is_file({dir_str:?})"))
            .eval()
            .unwrap();
        let got_missing: bool = lua
            .load(format!("return leviathan.fs.is_file({missing:?})"))
            .eval()
            .unwrap();
        assert!(got_file);
        assert!(!got_dir);
        assert!(!got_missing);
    }

    #[cfg(unix)]
    #[test]
    fn is_symlink_distinguishes_symlinks_from_regular_files() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target.txt");
        stdfs::File::create(&target).unwrap();
        let link = tmp.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let target_str = target.to_str().unwrap();
        let link_str = link.to_str().unwrap();
        let dir_str = tmp.path().to_str().unwrap();
        let missing = "/definitely/does/not/exist";

        let got_link: bool = lua
            .load(format!("return leviathan.fs.is_symlink({link_str:?})"))
            .eval()
            .unwrap();
        let got_target: bool = lua
            .load(format!("return leviathan.fs.is_symlink({target_str:?})"))
            .eval()
            .unwrap();
        let got_dir: bool = lua
            .load(format!("return leviathan.fs.is_symlink({dir_str:?})"))
            .eval()
            .unwrap();
        let got_missing: bool = lua
            .load(format!("return leviathan.fs.is_symlink({missing:?})"))
            .eval()
            .unwrap();
        assert!(got_link);
        assert!(!got_target);
        assert!(!got_dir);
        assert!(!got_missing);
    }

    #[test]
    fn canonicalize_resolves_relative_components() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b");
        stdfs::create_dir_all(&nested).unwrap();
        let messy = nested.join("..").join(".").join("b");
        let messy_str = messy.to_str().unwrap();

        let (got, err): (Option<String>, Option<String>) = lua
            .load(format!("return leviathan.fs.canonicalize({messy_str:?})"))
            .eval()
            .unwrap();
        assert!(err.is_none());
        let resolved = got.expect("canonicalize returned nil for valid path");
        let expected = stdfs::canonicalize(&nested).unwrap();
        assert_eq!(resolved, expected.to_string_lossy());
    }

    #[test]
    fn canonicalize_returns_error_for_missing_path() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let (got, err): (Option<String>, Option<String>) = lua
            .load("return leviathan.fs.canonicalize('/definitely/does/not/exist')")
            .eval()
            .unwrap();
        assert!(got.is_none());
        assert!(!err.expect("expected error string").is_empty());
    }

    #[test]
    fn cwd_returns_process_working_directory() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let (got, err): (Option<String>, Option<String>) =
            lua.load("return leviathan.fs.cwd()").eval().unwrap();
        assert!(err.is_none());
        let resolved = got.expect("cwd returned nil");
        let expected = std::env::current_dir().unwrap();
        assert_eq!(resolved, expected.to_string_lossy());
    }

    #[test]
    fn temp_dir_returns_process_temp_directory() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: String = lua.load("return leviathan.fs.temp_dir()").eval().unwrap();
        let expected = std::env::temp_dir();
        assert_eq!(got, expected.to_string_lossy());
        assert!(!got.is_empty());
    }

    #[test]
    fn config_dir_matches_dirs_crate_resolution() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua.load("return leviathan.fs.config_dir()").eval().unwrap();
        let expected = dirs::config_dir().map(|p| p.to_string_lossy().into_owned());
        assert_eq!(got, expected);
    }

    #[test]
    fn cache_dir_matches_dirs_crate_resolution() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua.load("return leviathan.fs.cache_dir()").eval().unwrap();
        let expected = dirs::cache_dir().map(|p| p.to_string_lossy().into_owned());
        assert_eq!(got, expected);
    }

    #[test]
    fn data_dir_matches_dirs_crate_resolution() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua.load("return leviathan.fs.data_dir()").eval().unwrap();
        let expected = dirs::data_dir().map(|p| p.to_string_lossy().into_owned());
        assert_eq!(got, expected);
    }

    #[test]
    fn state_dir_matches_dirs_crate_resolution() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua.load("return leviathan.fs.state_dir()").eval().unwrap();
        let expected = dirs::state_dir().map(|p| p.to_string_lossy().into_owned());
        assert_eq!(got, expected);
    }

    #[test]
    fn is_absolute_distinguishes_absolute_from_relative_paths() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let cases: &[(&str, bool)] = if cfg!(windows) {
            &[
                ("C:\\foo\\bar", true),
                ("\\\\server\\share\\file", true),
                ("foo\\bar", false),
                (".\\foo", false),
                ("", false),
            ]
        } else {
            &[
                ("/", true),
                ("/home/user/file.txt", true),
                ("foo/bar", false),
                ("./foo", false),
                ("../foo", false),
                ("", false),
            ]
        };

        for (path, expected) in cases {
            let got: bool = lua
                .load(format!("return leviathan.fs.is_absolute({path:?})"))
                .eval()
                .unwrap();
            assert_eq!(got, *expected, "is_absolute({path:?})");
        }
    }

    #[test]
    fn size_returns_byte_length_for_regular_file() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("payload.bin");
        let mut f = stdfs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();
        let path_str = path.to_str().unwrap();

        let (got, err): (Option<u64>, Option<String>) = lua
            .load(format!("return leviathan.fs.size({path_str:?})"))
            .eval()
            .unwrap();
        assert!(err.is_none());
        assert_eq!(got, Some(11));
    }

    #[test]
    fn size_returns_error_for_missing_path() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let (got, err): (Option<u64>, Option<String>) = lua
            .load("return leviathan.fs.size('/definitely/does/not/exist')")
            .eval()
            .unwrap();
        assert!(got.is_none());
        assert!(!err.expect("expected error string").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn size_does_not_follow_symlinks() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("real.bin");
        let mut f = stdfs::File::create(&target).unwrap();
        f.write_all(&[0u8; 4096]).unwrap();
        let link = tmp.path().join("link.bin");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let link_str = link.to_str().unwrap();

        let (got, err): (Option<u64>, Option<String>) = lua
            .load(format!("return leviathan.fs.size({link_str:?})"))
            .eval()
            .unwrap();
        assert!(err.is_none());
        let n = got.expect("size returned nil for symlink");
        assert!(
            n < 4096,
            "symlink size should be link length, not target length"
        );
    }

    #[test]
    fn relative_to_strips_base_prefix() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.relative_to('/home/user/project/src/main.rs', '/home/user/project')")
            .eval()
            .unwrap();
        assert_eq!(got.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn relative_to_returns_empty_string_when_paths_are_equal() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.relative_to('/home/user/project', '/home/user/project')")
            .eval()
            .unwrap();
        assert_eq!(got.as_deref(), Some(""));
    }

    #[test]
    fn relative_to_returns_nil_when_path_is_not_under_base() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.relative_to('/etc/hosts', '/home/user')")
            .eval()
            .unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn relative_to_does_not_match_partial_component() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.relative_to('/home/userdata/file', '/home/user')")
            .eval()
            .unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn with_extension_replaces_final_extension() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.with_extension('/home/user/file.txt', 'rs')")
            .eval()
            .unwrap();
        assert_eq!(got.as_deref(), Some("/home/user/file.rs"));
    }

    #[test]
    fn with_extension_adds_extension_when_missing() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.with_extension('README', 'md')")
            .eval()
            .unwrap();
        assert_eq!(got.as_deref(), Some("README.md"));
    }

    #[test]
    fn with_extension_strips_extension_when_ext_empty() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.with_extension('/home/user/file.txt', '')")
            .eval()
            .unwrap();
        assert_eq!(got.as_deref(), Some("/home/user/file"));
    }

    #[test]
    fn with_extension_only_replaces_final_extension() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.with_extension('archive.tar.gz', 'bz2')")
            .eval()
            .unwrap();
        assert_eq!(got.as_deref(), Some("archive.tar.bz2"));
    }

    #[test]
    fn with_extension_returns_nil_for_root() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.with_extension('/', 'rs')")
            .eval()
            .unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn with_extension_returns_nil_for_empty_path() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.with_extension('', 'rs')")
            .eval()
            .unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn with_file_name_replaces_final_component() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.with_file_name('/home/user/file.txt', 'notes.md')")
            .eval()
            .unwrap();
        assert_eq!(got.as_deref(), Some("/home/user/notes.md"));
    }

    #[test]
    fn with_file_name_replaces_when_path_has_no_parent() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.with_file_name('file.txt', 'notes.md')")
            .eval()
            .unwrap();
        assert_eq!(got.as_deref(), Some("notes.md"));
    }

    #[test]
    fn with_file_name_returns_nil_for_root() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.with_file_name('/', 'foo')")
            .eval()
            .unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn with_file_name_returns_nil_for_empty_path() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.with_file_name('', 'foo')")
            .eval()
            .unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn with_file_name_returns_nil_for_trailing_dotdot() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let got: Option<String> = lua
            .load("return leviathan.fs.with_file_name('/home/user/..', 'foo')")
            .eval()
            .unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn modified_returns_mtime_for_regular_file() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("note.txt");
        stdfs::File::create(&path).unwrap();
        let path_str = path.to_str().unwrap();

        let (got, err): (Option<i64>, Option<String>) = lua
            .load(format!("return leviathan.fs.modified({path_str:?})"))
            .eval()
            .unwrap();
        assert!(err.is_none());
        let secs = got.expect("modified returned nil");
        let now = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!(
            (now - secs).abs() < 60,
            "mtime {secs} not close to now {now}"
        );
    }

    #[test]
    fn modified_returns_error_for_missing_path() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let (got, err): (Option<i64>, Option<String>) = lua
            .load("return leviathan.fs.modified('/definitely/does/not/exist')")
            .eval()
            .unwrap();
        assert!(got.is_none());
        assert!(!err.expect("expected error string").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn modified_does_not_follow_symlinks() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("real.bin");
        stdfs::File::create(&target).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let link = tmp.path().join("link.bin");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let target_str = target.to_str().unwrap();
        let link_str = link.to_str().unwrap();

        let (target_got, _): (Option<i64>, Option<String>) = lua
            .load(format!("return leviathan.fs.modified({target_str:?})"))
            .eval()
            .unwrap();
        let (link_got, _): (Option<i64>, Option<String>) = lua
            .load(format!("return leviathan.fs.modified({link_str:?})"))
            .eval()
            .unwrap();
        let target_mtime = target_got.expect("target mtime nil");
        let link_mtime = link_got.expect("link mtime nil");
        assert!(
            link_mtime >= target_mtime,
            "symlink mtime {link_mtime} should be its own (>= target {target_mtime})"
        );
    }

    #[cfg(unix)]
    #[test]
    fn read_link_returns_stored_target_for_symlink() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("real.txt");
        stdfs::File::create(&target).unwrap();
        let link = tmp.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let link_str = link.to_str().unwrap();
        let target_str = target.to_str().unwrap();

        let (got, err): (Option<String>, Option<String>) = lua
            .load(format!("return leviathan.fs.read_link({link_str:?})"))
            .eval()
            .unwrap();
        assert!(err.is_none());
        assert_eq!(got.as_deref(), Some(target_str));
    }

    #[cfg(unix)]
    #[test]
    fn read_link_preserves_relative_target() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        stdfs::File::create(tmp.path().join("real.txt")).unwrap();
        let link = tmp.path().join("link.txt");
        std::os::unix::fs::symlink("real.txt", &link).unwrap();
        let link_str = link.to_str().unwrap();

        let (got, err): (Option<String>, Option<String>) = lua
            .load(format!("return leviathan.fs.read_link({link_str:?})"))
            .eval()
            .unwrap();
        assert!(err.is_none());
        assert_eq!(got.as_deref(), Some("real.txt"));
    }

    #[test]
    fn read_link_returns_error_for_regular_file() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("note.txt");
        stdfs::File::create(&path).unwrap();
        let path_str = path.to_str().unwrap();

        let (got, err): (Option<String>, Option<String>) = lua
            .load(format!("return leviathan.fs.read_link({path_str:?})"))
            .eval()
            .unwrap();
        assert!(got.is_none());
        assert!(!err.expect("expected error string").is_empty());
    }

    #[test]
    fn read_link_returns_error_for_missing_path() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let (got, err): (Option<String>, Option<String>) = lua
            .load("return leviathan.fs.read_link('/definitely/does/not/exist')")
            .eval()
            .unwrap();
        assert!(got.is_none());
        assert!(!err.expect("expected error string").is_empty());
    }

    #[test]
    fn read_file_rejects_files_over_size_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("big.bin");
        let f = stdfs::File::create(&path).unwrap();
        f.set_len(READ_FILE_BYTE_LIMIT + 1).unwrap();
        let err = read_file(path.to_str().unwrap()).unwrap_err();
        assert!(err.contains("read_file limit"));
    }

    #[test]
    fn write_file_creates_new_file_with_content() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("note.txt");
        let path_str = path.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!(
                "return leviathan.fs.write_file({path_str:?}, 'hello world')"
            ))
            .eval()
            .unwrap();
        assert!(ok);
        assert!(err.is_none());
        assert_eq!(stdfs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn write_file_overwrites_existing_file() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("note.txt");
        stdfs::write(&path, "old content").unwrap();
        let path_str = path.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!(
                "return leviathan.fs.write_file({path_str:?}, 'new')"
            ))
            .eval()
            .unwrap();
        assert!(ok);
        assert!(err.is_none());
        assert_eq!(stdfs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn write_file_returns_error_when_parent_missing() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load("return leviathan.fs.write_file('/definitely/does/not/exist/note.txt', 'x')")
            .eval()
            .unwrap();
        assert!(!ok);
        assert!(!err.expect("expected error string").is_empty());
    }

    #[test]
    fn mkdir_creates_nested_directories() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b").join("c");
        let nested_str = nested.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!("return leviathan.fs.mkdir({nested_str:?})"))
            .eval()
            .unwrap();
        assert!(ok);
        assert!(err.is_none());
        assert!(nested.is_dir());
    }

    #[test]
    fn mkdir_succeeds_when_directory_already_exists() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let path_str = tmp.path().to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!("return leviathan.fs.mkdir({path_str:?})"))
            .eval()
            .unwrap();
        assert!(ok);
        assert!(err.is_none());
    }

    #[test]
    fn mkdir_returns_error_when_path_is_an_existing_file() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blocker");
        stdfs::File::create(&path).unwrap();
        let path_str = path.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!("return leviathan.fs.mkdir({path_str:?})"))
            .eval()
            .unwrap();
        assert!(!ok);
        assert!(!err.expect("expected error string").is_empty());
    }

    #[test]
    fn rename_moves_file_to_new_path() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("from.txt");
        let dst = tmp.path().join("to.txt");
        stdfs::write(&src, "payload").unwrap();
        let src_str = src.to_str().unwrap();
        let dst_str = dst.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!(
                "return leviathan.fs.rename({src_str:?}, {dst_str:?})"
            ))
            .eval()
            .unwrap();
        assert!(ok);
        assert!(err.is_none());
        assert!(!src.exists());
        assert_eq!(stdfs::read_to_string(&dst).unwrap(), "payload");
    }

    #[test]
    fn rename_overwrites_existing_destination_file() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("from.txt");
        let dst = tmp.path().join("to.txt");
        stdfs::write(&src, "new").unwrap();
        stdfs::write(&dst, "old").unwrap();
        let src_str = src.to_str().unwrap();
        let dst_str = dst.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!(
                "return leviathan.fs.rename({src_str:?}, {dst_str:?})"
            ))
            .eval()
            .unwrap();
        assert!(ok);
        assert!(err.is_none());
        assert_eq!(stdfs::read_to_string(&dst).unwrap(), "new");
    }

    #[test]
    fn rename_returns_error_when_source_missing() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let dst = tmp.path().join("to.txt");
        let dst_str = dst.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!(
                "return leviathan.fs.rename('/definitely/does/not/exist', {dst_str:?})"
            ))
            .eval()
            .unwrap();
        assert!(!ok);
        assert!(!err.expect("expected error string").is_empty());
        assert!(!dst.exists());
    }

    #[test]
    fn copy_duplicates_file_to_new_path() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("from.txt");
        let dst = tmp.path().join("to.txt");
        stdfs::write(&src, "payload").unwrap();
        let src_str = src.to_str().unwrap();
        let dst_str = dst.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!(
                "return leviathan.fs.copy({src_str:?}, {dst_str:?})"
            ))
            .eval()
            .unwrap();
        assert!(ok);
        assert!(err.is_none());
        assert_eq!(stdfs::read_to_string(&src).unwrap(), "payload");
        assert_eq!(stdfs::read_to_string(&dst).unwrap(), "payload");
    }

    #[test]
    fn copy_overwrites_existing_destination_file() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("from.txt");
        let dst = tmp.path().join("to.txt");
        stdfs::write(&src, "new").unwrap();
        stdfs::write(&dst, "old").unwrap();
        let src_str = src.to_str().unwrap();
        let dst_str = dst.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!(
                "return leviathan.fs.copy({src_str:?}, {dst_str:?})"
            ))
            .eval()
            .unwrap();
        assert!(ok);
        assert!(err.is_none());
        assert_eq!(stdfs::read_to_string(&dst).unwrap(), "new");
    }

    #[test]
    fn copy_returns_error_when_source_missing() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let dst = tmp.path().join("to.txt");
        let dst_str = dst.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!(
                "return leviathan.fs.copy('/definitely/does/not/exist', {dst_str:?})"
            ))
            .eval()
            .unwrap();
        assert!(!ok);
        assert!(!err.expect("expected error string").is_empty());
        assert!(!dst.exists());
    }

    #[test]
    fn copy_rejects_source_over_size_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("big.bin");
        let f = stdfs::File::create(&src).unwrap();
        f.set_len(COPY_BYTE_LIMIT + 1).unwrap();
        let dst = tmp.path().join("dst.bin");
        let err = copy_file(src.to_str().unwrap(), dst.to_str().unwrap()).unwrap_err();
        assert!(err.contains("copy limit"));
        assert!(!dst.exists());
    }

    #[test]
    fn copy_returns_error_for_directory_source() {
        let tmp = tempfile::tempdir().unwrap();
        let dst = tmp.path().join("dst");
        let err = copy_file(tmp.path().to_str().unwrap(), dst.to_str().unwrap()).unwrap_err();
        assert!(err.contains("not a regular file"));
        assert!(!dst.exists());
    }

    #[test]
    fn write_file_rejects_content_over_size_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("big.bin");
        let oversized = "a".repeat((WRITE_FILE_BYTE_LIMIT + 1) as usize);
        let err = write_file(path.to_str().unwrap(), &oversized).unwrap_err();
        assert!(err.contains("write_file limit"));
        assert!(!path.exists());
    }

    #[test]
    fn append_file_creates_file_when_missing() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("log.txt");
        let path_str = path.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!(
                "return leviathan.fs.append_file({path_str:?}, 'first\\n')"
            ))
            .eval()
            .unwrap();
        assert!(ok);
        assert!(err.is_none());
        assert_eq!(stdfs::read_to_string(&path).unwrap(), "first\n");
    }

    #[test]
    fn append_file_appends_to_existing_file() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("log.txt");
        stdfs::write(&path, "first\n").unwrap();
        let path_str = path.to_str().unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load(format!(
                "return leviathan.fs.append_file({path_str:?}, 'second\\n')"
            ))
            .eval()
            .unwrap();
        assert!(ok);
        assert!(err.is_none());
        assert_eq!(stdfs::read_to_string(&path).unwrap(), "first\nsecond\n");
    }

    #[test]
    fn append_file_returns_error_when_parent_missing() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let (ok, err): (bool, Option<String>) = lua
            .load("return leviathan.fs.append_file('/definitely/does/not/exist/log.txt', 'x')")
            .eval()
            .unwrap();
        assert!(!ok);
        assert!(!err.expect("expected error string").is_empty());
    }

    #[test]
    fn append_file_rejects_content_over_size_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("big.log");
        let oversized = "a".repeat((APPEND_FILE_BYTE_LIMIT + 1) as usize);
        let err = append_file(path.to_str().unwrap(), &oversized).unwrap_err();
        assert!(err.contains("append_file limit"));
        assert!(!path.exists());
    }

    #[test]
    fn metadata_returns_entry_for_regular_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("note.txt");
        let mut f = stdfs::File::create(&path).unwrap();
        f.write_all(b"hello").unwrap();

        let e = metadata(path.to_str().unwrap()).unwrap();
        assert_eq!(e.name, "note.txt");
        assert_eq!(e.path, path.to_str().unwrap());
        assert!(!e.is_dir);
        assert!(!e.is_symlink);
        assert_eq!(e.size, 5);
        assert!(e.modified > 0);
    }

    #[test]
    fn metadata_flags_directories_and_zeros_size() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sub");
        stdfs::create_dir(&dir).unwrap();

        let e = metadata(dir.to_str().unwrap()).unwrap();
        assert!(e.is_dir);
        assert!(!e.is_symlink);
        assert_eq!(e.size, 0);
    }

    #[cfg(unix)]
    #[test]
    fn metadata_flags_symlink_without_following() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("real");
        stdfs::create_dir(&target).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let e = metadata(link.to_str().unwrap()).unwrap();
        assert!(e.is_symlink);
        assert!(!e.is_dir, "symlink to dir must not report is_dir");
    }

    #[test]
    fn metadata_errors_for_missing_path() {
        let err = metadata("/definitely/does/not/exist").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn metadata_lua_returns_table_matching_list_dir_entry_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.txt");
        let mut f = stdfs::File::create(&path).unwrap();
        f.write_all(b"abc").unwrap();

        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let script = format!(
            "local m = leviathan.fs.metadata({:?}); return m.name, m.path, m.is_dir, m.is_symlink, m.size",
            path.to_str().unwrap()
        );
        let (name, p, is_dir, is_symlink, size): (String, String, bool, bool, u64) =
            lua.load(script).eval().unwrap();
        assert_eq!(name, "a.txt");
        assert_eq!(p, path.to_str().unwrap());
        assert!(!is_dir);
        assert!(!is_symlink);
        assert_eq!(size, 3);
    }

    #[test]
    fn metadata_lua_returns_nil_and_error_for_missing_path() {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();

        let (got, err): (Option<mlua::Table>, Option<String>) = lua
            .load("return leviathan.fs.metadata('/definitely/does/not/exist')")
            .eval()
            .unwrap();
        assert!(got.is_none());
        assert!(!err.expect("expected error string").is_empty());
    }

    fn build_lua() -> Lua {
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        install_unrestricted(&lua, &leviathan);
        lua.globals().set("leviathan", leviathan).unwrap();
        lua
    }

    #[test]
    fn read_lines_splits_lf_terminated_text() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lf.txt");
        stdfs::write(&path, "alpha\nbeta\ngamma\n").unwrap();
        let lua = build_lua();
        let script = format!(
            "local t, err = leviathan.fs.read_lines({:?}); return t[1], t[2], t[3], #t, err",
            path.to_str().unwrap()
        );
        let (a, b, c, n, err): (String, String, String, i64, Option<String>) =
            lua.load(script).eval().unwrap();
        assert_eq!(a, "alpha");
        assert_eq!(b, "beta");
        assert_eq!(c, "gamma");
        assert_eq!(n, 3);
        assert!(err.is_none());
    }

    #[test]
    fn read_lines_strips_crlf_terminators() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("crlf.txt");
        stdfs::write(&path, "one\r\ntwo\r\nthree").unwrap();
        let lua = build_lua();
        let script = format!(
            "local t = leviathan.fs.read_lines({:?}); return t[1], t[2], t[3], #t",
            path.to_str().unwrap()
        );
        let (a, b, c, n): (String, String, String, i64) = lua.load(script).eval().unwrap();
        assert_eq!(a, "one");
        assert_eq!(b, "two");
        assert_eq!(c, "three");
        assert_eq!(n, 3);
    }

    #[test]
    fn read_lines_returns_empty_table_for_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.txt");
        stdfs::File::create(&path).unwrap();
        let lua = build_lua();
        let script = format!(
            "local t, err = leviathan.fs.read_lines({:?}); return #t, err",
            path.to_str().unwrap()
        );
        let (n, err): (i64, Option<String>) = lua.load(script).eval().unwrap();
        assert_eq!(n, 0);
        assert!(err.is_none());
    }

    #[test]
    fn read_lines_preserves_blank_internal_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blanks.txt");
        stdfs::write(&path, "a\n\nb\n").unwrap();
        let lua = build_lua();
        let script = format!(
            "local t = leviathan.fs.read_lines({:?}); return t[1], t[2], t[3], #t",
            path.to_str().unwrap()
        );
        let (a, b, c, n): (String, String, String, i64) = lua.load(script).eval().unwrap();
        assert_eq!(a, "a");
        assert_eq!(b, "");
        assert_eq!(c, "b");
        assert_eq!(n, 3);
    }

    #[test]
    fn read_lines_returns_nil_and_error_for_missing_path() {
        let lua = build_lua();
        let (got, err): (Option<mlua::Table>, Option<String>) = lua
            .load("return leviathan.fs.read_lines('/definitely/does/not/exist')")
            .eval()
            .unwrap();
        assert!(got.is_none());
        assert!(!err.expect("expected error string").is_empty());
    }

    #[test]
    fn touch_creates_empty_file_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("marker");
        let lua = build_lua();
        let script = format!(
            "local ok, err = leviathan.fs.touch({:?}); return ok, err",
            path.to_str().unwrap()
        );
        let (ok, err): (bool, Option<String>) = lua.load(script).eval().unwrap();
        assert!(ok);
        assert!(err.is_none());
        let meta = stdfs::metadata(&path).unwrap();
        assert!(meta.is_file());
        assert_eq!(meta.len(), 0);
    }

    #[test]
    fn touch_does_not_truncate_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("note.txt");
        stdfs::write(&path, b"hello world").unwrap();
        let lua = build_lua();
        let script = format!(
            "local ok = leviathan.fs.touch({:?}); return ok",
            path.to_str().unwrap()
        );
        let ok: bool = lua.load(script).eval().unwrap();
        assert!(ok);
        let contents = stdfs::read_to_string(&path).unwrap();
        assert_eq!(contents, "hello world");
    }

    #[test]
    fn touch_updates_modified_time_for_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("stale");
        stdfs::write(&path, b"x").unwrap();
        let backdated = SystemTime::now() - std::time::Duration::from_secs(60 * 60);
        let times = fs::FileTimes::new()
            .set_modified(backdated)
            .set_accessed(backdated);
        let f = fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_times(times).unwrap();
        drop(f);
        let before = stdfs::metadata(&path).unwrap().modified().unwrap();

        let lua = build_lua();
        let script = format!("return leviathan.fs.touch({:?})", path.to_str().unwrap());
        let _: bool = lua.load(script).eval().unwrap();

        let after = stdfs::metadata(&path).unwrap().modified().unwrap();
        assert!(after > before, "touch should advance mtime");
    }

    #[test]
    fn touch_returns_error_when_parent_dir_missing() {
        let lua = build_lua();
        let (ok, err): (bool, Option<String>) = lua
            .load("return leviathan.fs.touch('/definitely/does/not/exist/marker')")
            .eval()
            .unwrap();
        assert!(!ok);
        assert!(!err.expect("expected error string").is_empty());
    }
}

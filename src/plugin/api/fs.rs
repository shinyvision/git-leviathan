//! `leviathan.fs` — filesystem operations.
//!
//! Error convention (Lua-idiomatic): on success return the value and nil;
//! on failure return nil and an error string. Plugins use:
//!
//! ```lua
//! local entries, err = leviathan.fs.list_dir(path)
//! if not entries then leviathan.log(err) end
//! ```
//!
//! Paths are passed as strings in both directions. `list_dir` returns an
//! array-like table of `{name, path, is_dir, size, modified}` entries,
//! pre-sorted (dirs first, then case-sensitive name).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use mlua::{Lua, Table};

pub fn install(lua: &Lua, leviathan: &Table) -> mlua::Result<()> {
    let fs_tbl = lua.create_table()?;

    fs_tbl.set(
        "list_dir",
        lua.create_function(|lua_inner, path: String| {
            match list_dir(&path) {
                Ok(entries) => {
                    let arr = lua_inner.create_table()?;
                    for (i, e) in entries.into_iter().enumerate() {
                        let t = lua_inner.create_table()?;
                        t.set("name", e.name)?;
                        t.set("path", e.path)?;
                        t.set("is_dir", e.is_dir)?;
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

    fs_tbl.set(
        "delete",
        lua.create_function(|_, path: String| {
            let p = Path::new(&path);
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
        "join",
        lua.create_function(|_, (a, b): (String, String)| -> mlua::Result<String> {
            Ok(Path::new(&a).join(&b).to_string_lossy().into_owned())
        })?,
    )?;

    fs_tbl.set(
        "home",
        lua.create_function(|_, ()| -> mlua::Result<Option<String>> {
            Ok(dirs::home_dir().map(|p| p.to_string_lossy().into_owned()))
        })?,
    )?;

    fs_tbl.set(
        "exists",
        lua.create_function(|_, path: String| -> mlua::Result<bool> {
            Ok(Path::new(&path).exists())
        })?,
    )?;

    fs_tbl.set(
        "is_dir",
        lua.create_function(|_, path: String| -> mlua::Result<bool> {
            Ok(Path::new(&path).is_dir())
        })?,
    )?;

    leviathan.set("fs", fs_tbl)?;
    Ok(())
}

#[cfg_attr(test, derive(Debug))]
struct Entry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified: i64,
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
        let is_dir = meta.is_dir();
        let size = if is_dir { 0 } else { meta.len() };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(Entry { name, path: full, is_dir, size, modified });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;
    use std::io::Write;

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

    #[test]
    fn list_dir_errors_for_missing_path() {
        let err = list_dir("/definitely/does/not/exist").unwrap_err();
        assert!(!err.is_empty());
    }
}

//! `leviathan.env` — process environment access.
//!
//! `get(name)` returns `(value, nil)` when the variable is set to valid
//! UTF-8, `(nil, nil)` when it is unset, and `(nil, err)` when it is set
//! to a non-UTF-8 value the host can't surface as a Lua string. Plugins
//! use it to detect `$EDITOR`, `$SHELL`, `$LANG`, `$XDG_CONFIG_HOME`, and
//! similar without shelling out.
//!
//! ```lua
//! local editor, err = leviathan.env.get("EDITOR")
//! if err then leviathan.log(err) end
//! editor = editor or "vi"
//! ```
//!
//! `list()` returns a Lua table mapping every variable name to its value
//! for the current process. Non-UTF-8 entries (name or value) are
//! skipped silently — same Lua-string constraint as `get`'s error path —
//! so plugins enumerating the environment for diagnostics or config
//! discovery never trip on an exotic locale variable. Pairs with `get`
//! when the plugin doesn't know the name up-front.

use std::rc::Rc;

use mlua::{Lua, Table};

use crate::plugin::capabilities::CapabilityGuard;

pub fn install(lua: &Lua, leviathan: &Table, guard: Rc<CapabilityGuard>) -> mlua::Result<()> {
    let env_tbl = lua.create_table()?;

    let g = Rc::clone(&guard);
    env_tbl.set(
        "get",
        lua.create_function(move |_, name: String| {
            g.check_env().map_err(mlua::Error::external)?;
            match std::env::var(&name) {
                Ok(v) => Ok((Some(v), None::<String>)),
                Err(std::env::VarError::NotPresent) => Ok((None, None)),
                Err(std::env::VarError::NotUnicode(_)) => {
                    Ok((None, Some(format!("env var {name} is not valid UTF-8"))))
                }
            }
        })?,
    )?;

    let g = Rc::clone(&guard);
    env_tbl.set(
        "list",
        lua.create_function(move |lua_inner, ()| {
            g.check_env().map_err(mlua::Error::external)?;
            let t = lua_inner.create_table()?;
            for (name, value) in std::env::vars_os() {
                if let (Some(n), Some(v)) = (name.to_str(), value.to_str()) {
                    t.set(n, v)?;
                }
            }
            Ok(t)
        })?,
    )?;

    leviathan.set("env", env_tbl)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_lua() -> Lua {
        use crate::plugin::capability_grants::{DecidedBy, Decision, GrantStore};
        use git_leviathan_plugin_api::capability::Capability;
        let lua = Lua::new();
        let leviathan = lua.create_table().unwrap();
        let store = GrantStore::new_in_memory();
        store
            .record_decision(
                "test",
                "0.1.0",
                "env",
                Decision::Allow,
                DecidedBy::Default,
                None,
            )
            .unwrap();
        let guard = Rc::new(CapabilityGuard::new(
            "test",
            "0.1.0",
            vec![Capability::Env],
            crate::plugin::capabilities::CapabilityPaths {
                plugin_root: std::env::temp_dir(),
                state_dir: std::env::temp_dir(),
                config_dir: std::env::temp_dir(),
                workdir: None,
            },
            store,
        ));
        install(&lua, &leviathan, guard).unwrap();
        lua.globals().set("leviathan", leviathan).unwrap();
        lua
    }

    #[test]
    fn get_returns_value_for_set_variable() {
        let key = "GIT_LEVIATHAN_ENV_GET_TEST_SET";
        // SAFETY: single-threaded test, no other threads inspecting env.
        unsafe {
            std::env::set_var(key, "hello");
        }
        let lua = build_lua();
        let (got, err): (Option<String>, Option<String>) = lua
            .load(format!("return leviathan.env.get({key:?})"))
            .eval()
            .unwrap();
        unsafe {
            std::env::remove_var(key);
        }
        assert_eq!(got.as_deref(), Some("hello"));
        assert!(err.is_none());
    }

    #[test]
    fn get_returns_nil_for_missing_variable() {
        let key = "GIT_LEVIATHAN_ENV_GET_TEST_MISSING";
        unsafe {
            std::env::remove_var(key);
        }
        let lua = build_lua();
        let (got, err): (Option<String>, Option<String>) = lua
            .load(format!("return leviathan.env.get({key:?})"))
            .eval()
            .unwrap();
        assert!(got.is_none());
        assert!(err.is_none());
    }

    #[test]
    fn list_returns_table_containing_set_variable() {
        let key = "GIT_LEVIATHAN_ENV_LIST_TEST_SET";
        unsafe {
            std::env::set_var(key, "value-123");
        }
        let lua = build_lua();
        let got: Option<String> = lua
            .load(format!("return leviathan.env.list()[{key:?}]"))
            .eval()
            .unwrap();
        unsafe {
            std::env::remove_var(key);
        }
        assert_eq!(got.as_deref(), Some("value-123"));
    }

    #[test]
    fn list_omits_unset_variable() {
        let key = "GIT_LEVIATHAN_ENV_LIST_TEST_MISSING";
        unsafe {
            std::env::remove_var(key);
        }
        let lua = build_lua();
        let got: Option<String> = lua
            .load(format!("return leviathan.env.list()[{key:?}]"))
            .eval()
            .unwrap();
        assert!(got.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn list_skips_non_unicode_entries() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let key = "GIT_LEVIATHAN_ENV_LIST_TEST_BADUTF8";
        let bad = OsString::from_vec(vec![0x66, 0x80, 0x6f]);
        unsafe {
            std::env::set_var(key, &bad);
        }
        let lua = build_lua();
        let got: Option<String> = lua
            .load(format!("return leviathan.env.list()[{key:?}]"))
            .eval()
            .unwrap();
        unsafe {
            std::env::remove_var(key);
        }
        assert!(got.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn get_returns_error_for_non_unicode_value() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let key = "GIT_LEVIATHAN_ENV_GET_TEST_BADUTF8";
        let bad = OsString::from_vec(vec![0x66, 0x80, 0x6f]);
        unsafe {
            std::env::set_var(key, &bad);
        }
        let lua = build_lua();
        let (got, err): (Option<String>, Option<String>) = lua
            .load(format!("return leviathan.env.get({key:?})"))
            .eval()
            .unwrap();
        unsafe {
            std::env::remove_var(key);
        }
        assert!(got.is_none());
        assert!(err
            .expect("expected error string")
            .contains("not valid UTF-8"));
    }
}

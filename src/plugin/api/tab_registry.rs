//! `leviathan.tab_registry` — read-only snapshot of the open tabs plus
//! mutation functions that funnel through the host's pending-op queue.
//!
//! Mirrors the `leviathan.repository` shape: passive table is rebuilt by
//! [`PluginHost::sync_tab_registry`](crate::plugin::PluginHost::sync_tab_registry)
//! on every change, so plugins re-read the globals from a `TabSwitched` /
//! `TabAdded` / `TabRemoved` / `TabReordered` autocmd to observe state.
//!
//! Functions never mutate `TabManager` directly. They push a
//! [`TabRegistryOp`] into a shared `Rc<RefCell<Vec<_>>>` the host hands to
//! every plugin's Lua state at install time. The app drains the queue
//! after each `App::update` tick and applies ops through the canonical
//! `TabManager` paths (so settings persistence and tab events keep
//! working).
//!
//! Shape:
//!
//! ```text
//! leviathan.tab_registry = {
//!   list = { { path = "...", name = "..." }, ... },
//!   current = { path = "...", name = "..." } | nil,
//!   add = function(path) ... end,
//!   remove = function(path) ... end,
//!   select = function(path) ... end,
//!   reorder = function(paths) ... end,
//! }
//! ```
//!
//! `add(path)` either opens a new tab for `path` or focuses an already-open
//! tab — same semantics as the "+" button. `remove(path)` closes the tab
//! whose `path` matches (no-op if absent). `select(path)` switches to the
//! matching tab (no-op if absent). `reorder(paths)` reorders the tabs to
//! match the given sequence of paths (entries not present are appended in
//! their existing order).

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Lua, Table};

use super::super::tab_snapshot::{TabRegistryOp, TabsSnapshot};

pub type PendingOps = Rc<RefCell<Vec<TabRegistryOp>>>;

/// Build the `tab_registry` table for a freshly-loaded plugin. Functions
/// share the host's `PendingOps` queue so calls from any plugin reach the
/// same drain point in the app.
pub fn install(lua: &Lua, leviathan: &Table, ops: PendingOps) -> mlua::Result<()> {
    let registry = lua.create_table()?;
    registry.set("list", lua.create_table()?)?;
    registry.set("current", mlua::Value::Nil)?;

    let q = Rc::clone(&ops);
    registry.set(
        "add",
        lua.create_function(move |_, path: String| {
            q.borrow_mut().push(TabRegistryOp::Add(path));
            Ok(())
        })?,
    )?;

    let q = Rc::clone(&ops);
    registry.set(
        "remove",
        lua.create_function(move |_, path: String| {
            q.borrow_mut().push(TabRegistryOp::Remove(path));
            Ok(())
        })?,
    )?;

    let q = Rc::clone(&ops);
    registry.set(
        "select",
        lua.create_function(move |_, path: String| {
            q.borrow_mut().push(TabRegistryOp::Select(path));
            Ok(())
        })?,
    )?;

    let q = Rc::clone(&ops);
    registry.set(
        "reorder",
        lua.create_function(move |_, paths: mlua::Table| {
            let mut out: Vec<String> = Vec::new();
            for v in paths.sequence_values::<String>() {
                out.push(v?);
            }
            q.borrow_mut().push(TabRegistryOp::Reorder(out));
            Ok(())
        })?,
    )?;

    leviathan.set("tab_registry", registry)?;
    Ok(())
}

/// Rebuild the `list` and `current` fields from a fresh snapshot. Leaves
/// the function fields (`add`/`remove`/`select`) untouched so handlers
/// stashed by plugin code keep working.
pub fn refresh(lua: &Lua, leviathan: &Table, snapshot: &TabsSnapshot) -> mlua::Result<()> {
    let registry: Table = leviathan.get("tab_registry")?;
    let list = lua.create_table()?;
    let mut current: Option<Table> = None;
    for (i, entry) in snapshot.tabs.iter().enumerate() {
        let t = lua.create_table()?;
        t.set("path", entry.path.as_str())?;
        t.set("name", entry.name.as_str())?;
        if Some(entry.id) == snapshot.active_id {
            current = Some(t.clone());
        }
        list.raw_set(i + 1, t)?;
    }
    registry.set("list", list)?;
    match current {
        Some(t) => registry.set("current", t)?,
        None => registry.set("current", mlua::Value::Nil)?,
    }
    Ok(())
}

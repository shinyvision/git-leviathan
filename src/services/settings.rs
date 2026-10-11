use rusqlite::{Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use super::migrations;

const DB_NAME: &str = "git_leviathan.db";
const MOST_RECENT_REPO_KEY: &str = "most_recent_repo";
const PLUGIN_TABS_KEY: &str = "plugin_tabs";
const ACTIVE_WORKSPACE_KEY: &str = "active_workspace";
const WORKSPACE_ACTIVE_TABS_KEY: &str = "workspace_active_tabs";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedPluginTab {
    pub plugin_id: String,
    pub screen_id: String,
    pub title: String,
    pub bound_repo_path: Option<String>,
    pub state: Option<serde_json::Value>,
    /// Workspace the tab belongs to. Absent in tabs saved before workspaces
    /// existed, which therefore restore into the default workspace.
    #[serde(default)]
    pub workspace_id: u64,
}

/// A user-created workspace row. The default workspace is not stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedWorkspace {
    pub id: u64,
    pub name: String,
    pub icon: String,
}

pub struct SettingsService {
    conn: Connection,
}

impl SettingsService {
    pub fn new() -> Result<Self> {
        let db_path = Self::db_path()?;
        let mut conn = Connection::open(db_path)?;
        migrations::run(&mut conn)?;
        Ok(Self { conn })
    }

    fn densify_positions(conn: &Connection) -> Result<()> {
        conn.execute(
            "WITH ranked AS (
                SELECT path, ROW_NUMBER() OVER (ORDER BY position ASC, path ASC) - 1 AS new_pos
                FROM opened_repos
            )
            UPDATE opened_repos SET position = (
                SELECT new_pos FROM ranked WHERE ranked.path = opened_repos.path
            )",
            [],
        )?;
        Ok(())
    }

    /// Opened repository paths in tab order (tests; the app loads them with
    /// their workspaces via `load_repos_with_workspaces`).
    #[cfg(test)]
    pub fn load_repos(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM opened_repos ORDER BY position ASC, path ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    pub fn add_repo(&self, path: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT position FROM opened_repos WHERE path = ?1",
                [path],
                |row| row.get(0),
            )
            .ok();
        if let Some(pos) = existing {
            tx.execute(
                "UPDATE opened_repos SET position = position + 1 WHERE position < ?1",
                [pos],
            )?;
            tx.execute(
                "UPDATE opened_repos SET position = 0, opened_at = unixepoch() WHERE path = ?1",
                [path],
            )?;
        } else {
            tx.execute("UPDATE opened_repos SET position = position + 1", [])?;
            tx.execute(
                "INSERT INTO opened_repos (path, opened_at, position) VALUES (?1, unixepoch(), 0)",
                [path],
            )?;
        }
        Self::densify_positions(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn remove_repo(&self, path: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM opened_repos WHERE path = ?1", [path])?;
        Self::densify_positions(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_repo_order(&self, paths: &[String]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let offset = (paths.len() as i64) + 1;
        tx.execute("UPDATE opened_repos SET position = position + ?1", [offset])?;
        for (i, path) in paths.iter().enumerate() {
            tx.execute(
                "UPDATE opened_repos SET position = ?1 WHERE path = ?2",
                rusqlite::params![i as i64, path],
            )?;
        }
        Self::densify_positions(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_most_recent_repo(&self) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM app_state WHERE key = ?1")?;
        let mut rows = stmt.query([MOST_RECENT_REPO_KEY])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn set_most_recent_repo(&self, path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO app_state (key, value) VALUES (?1, ?2)",
            [MOST_RECENT_REPO_KEY, path],
        )?;
        Ok(())
    }

    pub fn load_plugin_tabs(&self) -> Result<Vec<PersistedPluginTab>> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM app_state WHERE key = ?1",
                [PLUGIN_TABS_KEY],
                |row| row.get(0),
            )
            .ok();
        let Some(value) = value else {
            return Ok(Vec::new());
        };
        Ok(serde_json::from_str(&value).unwrap_or_default())
    }

    pub fn save_plugin_tabs(&self, tabs: &[PersistedPluginTab]) -> Result<()> {
        let value = serde_json::to_string(tabs).unwrap_or_else(|_| "[]".to_string());
        self.conn.execute(
            "INSERT OR REPLACE INTO app_state (key, value) VALUES (?1, ?2)",
            [PLUGIN_TABS_KEY, value.as_str()],
        )?;
        Ok(())
    }

    fn get_state(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM app_state WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
    }

    fn set_state(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO app_state (key, value) VALUES (?1, ?2)",
            [key, value],
        )?;
        Ok(())
    }

    /// Opened repositories in tab order, each with the workspace it lives in.
    pub fn load_repos_with_workspaces(&self) -> Result<Vec<(String, u64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, workspace_id FROM opened_repos ORDER BY position ASC, path ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?.max(0) as u64,
            ))
        })?;
        rows.collect()
    }

    pub fn set_repo_workspace(&self, path: &str, workspace_id: u64) -> Result<()> {
        self.conn.execute(
            "UPDATE opened_repos SET workspace_id = ?1 WHERE path = ?2",
            rusqlite::params![workspace_id as i64, path],
        )?;
        Ok(())
    }

    /// User-created workspaces in their sort order.
    pub fn load_workspaces(&self) -> Result<Vec<PersistedWorkspace>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, icon FROM workspaces ORDER BY position ASC, id ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok(PersistedWorkspace {
                id: row.get::<_, i64>(0)?.max(0) as u64,
                name: row.get(1)?,
                icon: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    /// Insert a workspace after the existing ones and return its id.
    pub fn create_workspace(&self, name: &str, icon: &str) -> Result<u64> {
        self.conn.execute(
            "INSERT INTO workspaces (name, icon, position)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(position) + 1, 0) FROM workspaces))",
            [name, icon],
        )?;
        Ok(self.conn.last_insert_rowid().max(0) as u64)
    }

    pub fn update_workspace(&self, id: u64, name: &str, icon: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE workspaces SET name = ?1, icon = ?2 WHERE id = ?3",
            rusqlite::params![name, icon, id as i64],
        )?;
        Ok(())
    }

    /// Delete a workspace. Its repositories fall back to the default
    /// workspace rather than being forgotten.
    pub fn delete_workspace(&self, id: u64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM workspaces WHERE id = ?1", [id as i64])?;
        tx.execute(
            "UPDATE opened_repos SET workspace_id = 0 WHERE workspace_id = ?1",
            [id as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_workspace_order(&self, ids: &[u64]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE workspaces SET position = ?1 WHERE id = ?2",
                rusqlite::params![i as i64, *id as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_active_workspace(&self) -> Result<Option<u64>> {
        Ok(self
            .get_state(ACTIVE_WORKSPACE_KEY)?
            .and_then(|value| value.parse().ok()))
    }

    pub fn set_active_workspace(&self, id: u64) -> Result<()> {
        self.set_state(ACTIVE_WORKSPACE_KEY, &id.to_string())
    }

    /// Tab path each workspace last had active, so switching back (or
    /// restarting) lands on the same tab.
    pub fn load_workspace_active_tabs(&self) -> Result<BTreeMap<u64, String>> {
        Ok(self
            .get_state(WORKSPACE_ACTIVE_TABS_KEY)?
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default())
    }

    pub fn save_workspace_active_tabs(&self, tabs: &BTreeMap<u64, String>) -> Result<()> {
        let value = serde_json::to_string(tabs).unwrap_or_else(|_| "{}".to_string());
        self.set_state(WORKSPACE_ACTIVE_TABS_KEY, &value)
    }

    /// Load per-section expanded state for a repo. Returns `None` when the
    /// repo has no stored config yet (caller should apply defaults); returns
    /// `Some(Vec<(section_kind_key, expanded)>)` otherwise.
    pub fn load_sidebar_sections(&self, repo_path: &str) -> Result<Option<Vec<(String, bool)>>> {
        let mut stmt = self.conn.prepare(
            "SELECT section_kind, expanded FROM repo_sidebar_sections WHERE repo_path = ?1",
        )?;
        let rows: Vec<(String, bool)> = stmt
            .query_map([repo_path], |row| {
                let kind: String = row.get(0)?;
                let expanded: i64 = row.get(1)?;
                Ok((kind, expanded != 0))
            })?
            .collect::<Result<Vec<_>>>()?;
        if rows.is_empty() {
            Ok(None)
        } else {
            Ok(Some(rows))
        }
    }

    pub fn set_sidebar_section(
        &self,
        repo_path: &str,
        section_kind: &str,
        expanded: bool,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO repo_sidebar_sections (repo_path, section_kind, expanded) VALUES (?1, ?2, ?3)",
            rusqlite::params![repo_path, section_kind, expanded as i64],
        )?;
        Ok(())
    }

    fn db_path() -> Result<PathBuf> {
        let config_dir =
            dirs::config_dir().ok_or_else(|| rusqlite::Error::SqliteSingleThreadedMode)?;
        let app_dir = config_dir.join("git_leviathan");
        if let Err(e) = std::fs::create_dir_all(&app_dir) {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(e),
            ));
        }
        Ok(app_dir.join(DB_NAME))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn temp_db() -> (SettingsService, NamedTempFile) {
        let temp = NamedTempFile::new().unwrap();
        let mut conn = Connection::open(temp.path()).unwrap();
        migrations::run(&mut conn).unwrap();
        (SettingsService { conn }, temp)
    }

    #[test]
    fn load_empty_repos() {
        let (service, _temp) = temp_db();
        let repos = service.load_repos().unwrap();
        assert!(repos.is_empty());
    }

    #[test]
    fn add_and_load_repos() {
        let (service, _temp) = temp_db();
        service.add_repo("/repo/a").unwrap();
        service.add_repo("/repo/b").unwrap();
        let repos = service.load_repos().unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0], "/repo/b");
        assert_eq!(repos[1], "/repo/a");
    }

    #[test]
    fn remove_repo() {
        let (service, _temp) = temp_db();
        service.add_repo("/repo/a").unwrap();
        service.add_repo("/repo/b").unwrap();
        service.remove_repo("/repo/a").unwrap();
        let repos = service.load_repos().unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0], "/repo/b");
    }

    #[test]
    fn sidebar_sections_roundtrip() {
        let (service, _temp) = temp_db();
        assert!(service.load_sidebar_sections("/repo/a").unwrap().is_none());
        service
            .set_sidebar_section("/repo/a", "local", true)
            .unwrap();
        service
            .set_sidebar_section("/repo/a", "tags", false)
            .unwrap();
        service
            .set_sidebar_section("/repo/b", "local", false)
            .unwrap();
        let mut a = service.load_sidebar_sections("/repo/a").unwrap().unwrap();
        a.sort();
        assert_eq!(
            a,
            vec![("local".to_string(), true), ("tags".to_string(), false)]
        );
        let b = service.load_sidebar_sections("/repo/b").unwrap().unwrap();
        assert_eq!(b, vec![("local".to_string(), false)]);
    }

    #[test]
    fn sidebar_section_overwrite() {
        let (service, _temp) = temp_db();
        service
            .set_sidebar_section("/repo/a", "local", true)
            .unwrap();
        service
            .set_sidebar_section("/repo/a", "local", false)
            .unwrap();
        let a = service.load_sidebar_sections("/repo/a").unwrap().unwrap();
        assert_eq!(a, vec![("local".to_string(), false)]);
    }

    #[test]
    fn plugin_tabs_roundtrip() {
        let (service, _temp) = temp_db();
        let tabs = vec![PersistedPluginTab {
            plugin_id: "p".into(),
            screen_id: "main".into(),
            title: "Main".into(),
            bound_repo_path: Some("/repo".into()),
            state: Some(serde_json::json!({ "n": 2 })),
            workspace_id: 3,
        }];
        service.save_plugin_tabs(&tabs).unwrap();
        assert_eq!(service.load_plugin_tabs().unwrap(), tabs);
    }

    #[test]
    fn add_duplicate_updates_timestamp() {
        let (service, _temp) = temp_db();
        service.add_repo("/repo/a").unwrap();
        service.add_repo("/repo/a").unwrap();
        let repos = service.load_repos().unwrap();
        assert_eq!(repos.len(), 1);
    }

    #[test]
    fn plugin_tabs_saved_before_workspaces_load_into_default() {
        let (service, _temp) = temp_db();
        service
            .set_state(
                PLUGIN_TABS_KEY,
                r#"[{"plugin_id":"p","screen_id":"s","title":"T","bound_repo_path":null,"state":null}]"#,
            )
            .unwrap();
        let tabs = service.load_plugin_tabs().unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].workspace_id, 0);
    }

    #[test]
    fn workspaces_create_load_update_order() {
        let (service, _temp) = temp_db();
        assert!(service.load_workspaces().unwrap().is_empty());
        let a = service.create_workspace("Alpha", "rocket").unwrap();
        let b = service.create_workspace("Beta", "code").unwrap();
        assert_ne!(a, 0);
        assert_ne!(a, b);
        let names: Vec<String> = service
            .load_workspaces()
            .unwrap()
            .into_iter()
            .map(|w| w.name)
            .collect();
        assert_eq!(names, vec!["Alpha", "Beta"]);

        service.update_workspace(a, "Alpha 2", "bug").unwrap();
        service.set_workspace_order(&[b, a]).unwrap();
        let loaded = service.load_workspaces().unwrap();
        assert_eq!(
            loaded,
            vec![
                PersistedWorkspace {
                    id: b,
                    name: "Beta".into(),
                    icon: "code".into()
                },
                PersistedWorkspace {
                    id: a,
                    name: "Alpha 2".into(),
                    icon: "bug".into()
                },
            ]
        );
        // New workspaces still land after the reordered ones.
        let c = service.create_workspace("Gamma", "star").unwrap();
        assert_eq!(service.load_workspaces().unwrap().last().unwrap().id, c);
    }

    #[test]
    fn repo_workspace_membership_roundtrip_and_delete_falls_back() {
        let (service, _temp) = temp_db();
        let ws = service.create_workspace("Work", "briefcase").unwrap();
        service.add_repo("/repo/a").unwrap();
        service.add_repo("/repo/b").unwrap();
        service.set_repo_workspace("/repo/a", ws).unwrap();
        let repos = service.load_repos_with_workspaces().unwrap();
        assert_eq!(
            repos,
            vec![("/repo/b".to_string(), 0), ("/repo/a".to_string(), ws)]
        );
        // Re-adding an already-open repo keeps its workspace.
        service.add_repo("/repo/a").unwrap();
        assert!(service
            .load_repos_with_workspaces()
            .unwrap()
            .contains(&("/repo/a".to_string(), ws)));

        service.delete_workspace(ws).unwrap();
        assert!(service.load_workspaces().unwrap().is_empty());
        assert!(service
            .load_repos_with_workspaces()
            .unwrap()
            .iter()
            .all(|(_, w)| *w == 0));
    }

    #[test]
    fn active_workspace_and_per_workspace_tabs_roundtrip() {
        let (service, _temp) = temp_db();
        assert_eq!(service.get_active_workspace().unwrap(), None);
        service.set_active_workspace(4).unwrap();
        assert_eq!(service.get_active_workspace().unwrap(), Some(4));

        assert!(service.load_workspace_active_tabs().unwrap().is_empty());
        let mut map = BTreeMap::new();
        map.insert(0, "/repo/a".to_string());
        map.insert(4, "/repo/b".to_string());
        service.save_workspace_active_tabs(&map).unwrap();
        assert_eq!(service.load_workspace_active_tabs().unwrap(), map);
    }

    fn positions(service: &SettingsService) -> Vec<(String, i64)> {
        let mut stmt = service
            .conn
            .prepare("SELECT path, position FROM opened_repos ORDER BY position ASC")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .unwrap()
            .collect::<Result<_>>()
            .unwrap()
    }

    #[test]
    fn positions_are_dense_after_add() {
        let (service, _temp) = temp_db();
        service.add_repo("/repo/a").unwrap();
        service.add_repo("/repo/b").unwrap();
        service.add_repo("/repo/c").unwrap();
        let pos = positions(&service);
        assert_eq!(
            pos,
            vec![
                ("/repo/c".into(), 0),
                ("/repo/b".into(), 1),
                ("/repo/a".into(), 2),
            ]
        );
    }

    #[test]
    fn positions_are_dense_after_remove() {
        let (service, _temp) = temp_db();
        service.add_repo("/repo/a").unwrap();
        service.add_repo("/repo/b").unwrap();
        service.add_repo("/repo/c").unwrap();
        service.add_repo("/repo/d").unwrap();
        service.remove_repo("/repo/b").unwrap();
        service.remove_repo("/repo/c").unwrap();
        let pos = positions(&service);
        let values: Vec<i64> = pos.iter().map(|(_, p)| *p).collect();
        assert_eq!(values, vec![0, 1]);
    }

    #[test]
    fn set_repo_order_writes_dense_positions() {
        let (service, _temp) = temp_db();
        service.add_repo("/repo/a").unwrap();
        service.add_repo("/repo/b").unwrap();
        service.add_repo("/repo/c").unwrap();
        let new_order = vec![
            "/repo/a".to_string(),
            "/repo/c".to_string(),
            "/repo/b".to_string(),
        ];
        service.set_repo_order(&new_order).unwrap();
        let repos = service.load_repos().unwrap();
        assert_eq!(repos, new_order);
        let pos = positions(&service);
        let values: Vec<i64> = pos.iter().map(|(_, p)| *p).collect();
        assert_eq!(values, vec![0, 1, 2]);
    }

    #[test]
    fn set_repo_order_densifies_when_paths_missing() {
        let (service, _temp) = temp_db();
        service.add_repo("/repo/a").unwrap();
        service.add_repo("/repo/b").unwrap();
        service.add_repo("/repo/c").unwrap();
        let partial = vec!["/repo/c".to_string(), "/repo/a".to_string()];
        service.set_repo_order(&partial).unwrap();
        let pos = positions(&service);
        let values: Vec<i64> = pos.iter().map(|(_, p)| *p).collect();
        assert_eq!(values, vec![0, 1, 2]);
        let repos = service.load_repos().unwrap();
        assert_eq!(repos[0], "/repo/c");
        assert_eq!(repos[1], "/repo/a");
    }

    #[test]
    fn readd_existing_moves_to_front_without_gaps() {
        let (service, _temp) = temp_db();
        service.add_repo("/repo/a").unwrap();
        service.add_repo("/repo/b").unwrap();
        service.add_repo("/repo/c").unwrap();
        service.add_repo("/repo/a").unwrap();
        let repos = service.load_repos().unwrap();
        assert_eq!(repos, vec!["/repo/a", "/repo/c", "/repo/b"]);
        let pos = positions(&service);
        let values: Vec<i64> = pos.iter().map(|(_, p)| *p).collect();
        assert_eq!(values, vec![0, 1, 2]);
    }
}

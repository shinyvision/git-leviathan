use rusqlite::{Connection, Result};
use std::path::PathBuf;

const DB_NAME: &str = "git_leviathan.db";
const SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS opened_repos (
        path TEXT PRIMARY KEY,
        opened_at INTEGER NOT NULL DEFAULT (unixepoch())
    );
    CREATE INDEX IF NOT EXISTS idx_opened_at ON opened_repos(opened_at);
    CREATE TABLE IF NOT EXISTS app_state (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS repo_sidebar_sections (
        repo_path TEXT NOT NULL,
        section_kind TEXT NOT NULL,
        expanded INTEGER NOT NULL,
        PRIMARY KEY (repo_path, section_kind)
    );
"#;

const MOST_RECENT_REPO_KEY: &str = "most_recent_repo";

pub struct SettingsService {
    conn: Connection,
}

impl SettingsService {
    pub fn new() -> Result<Self> {
        let db_path = Self::db_path()?;
        let conn = Connection::open(db_path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn load_repos(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM opened_repos ORDER BY opened_at DESC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    pub fn add_repo(&self, path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO opened_repos (path, opened_at) VALUES (?1, unixepoch())",
            [path],
        )?;
        Ok(())
    }

    pub fn remove_repo(&self, path: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM opened_repos WHERE path = ?1", [path])?;
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
        let conn = Connection::open(temp.path()).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
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
        service.set_sidebar_section("/repo/a", "local", true).unwrap();
        service.set_sidebar_section("/repo/a", "tags", false).unwrap();
        service.set_sidebar_section("/repo/b", "local", false).unwrap();
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
        service.set_sidebar_section("/repo/a", "local", true).unwrap();
        service.set_sidebar_section("/repo/a", "local", false).unwrap();
        let a = service.load_sidebar_sections("/repo/a").unwrap().unwrap();
        assert_eq!(a, vec![("local".to_string(), false)]);
    }

    #[test]
    fn add_duplicate_updates_timestamp() {
        let (service, _temp) = temp_db();
        service.add_repo("/repo/a").unwrap();
        service.add_repo("/repo/a").unwrap();
        let repos = service.load_repos().unwrap();
        assert_eq!(repos.len(), 1);
    }
}

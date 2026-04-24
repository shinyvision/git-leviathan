use rusqlite::{Result, Transaction};

pub fn up(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        r#"
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
        "#,
    )
}

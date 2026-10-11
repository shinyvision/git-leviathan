use rusqlite::{Result, Transaction};

/// Workspaces: named groups of tabs. The built-in default workspace is id 0
/// and has no row; every `opened_repos` row starts out in it.
pub fn up(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS workspaces (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            icon TEXT NOT NULL,
            position INTEGER NOT NULL DEFAULT 0
        );
        "#,
    )?;
    let has_column: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('opened_repos') WHERE name = 'workspace_id'",
        [],
        |r| r.get(0),
    )?;
    if has_column == 0 {
        tx.execute(
            "ALTER TABLE opened_repos ADD COLUMN workspace_id INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

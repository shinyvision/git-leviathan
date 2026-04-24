use rusqlite::{Result, Transaction};

pub fn up(tx: &Transaction) -> Result<()> {
    let has_column: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('opened_repos') WHERE name = 'position'",
        [],
        |r| r.get(0),
    )?;
    if has_column == 0 {
        tx.execute(
            "ALTER TABLE opened_repos ADD COLUMN position INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        tx.execute(
            "UPDATE opened_repos SET position = (
                SELECT rn - 1 FROM (
                    SELECT path, ROW_NUMBER() OVER (ORDER BY opened_at DESC, path ASC) AS rn
                    FROM opened_repos
                ) r WHERE r.path = opened_repos.path
            )",
            [],
        )?;
    }
    Ok(())
}

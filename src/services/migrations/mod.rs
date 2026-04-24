use rusqlite::{Connection, Result, Transaction};

mod m001_initial;
mod m002_position_column;

type MigrationFn = fn(&Transaction) -> Result<()>;

const MIGRATIONS: &[(i64, MigrationFn)] = &[
    (1, m001_initial::up),
    (2, m002_position_column::up),
];

pub fn run(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (version, up) in MIGRATIONS {
        if current < *version {
            let tx = conn.transaction()?;
            up(&tx)?;
            tx.pragma_update(None, "user_version", *version)?;
            tx.commit()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pragma_user_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn fresh_db_applies_all_migrations() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        assert_eq!(pragma_user_version(&conn), MIGRATIONS.last().unwrap().0);
        let has_position: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('opened_repos') WHERE name = 'position'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_position, 1);
    }

    #[test]
    fn running_twice_is_noop() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        run(&mut conn).unwrap();
        assert_eq!(pragma_user_version(&conn), MIGRATIONS.last().unwrap().0);
    }

    #[test]
    fn legacy_db_at_version_zero_gets_upgraded() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE opened_repos (
                path TEXT PRIMARY KEY,
                opened_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE app_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE repo_sidebar_sections (
                repo_path TEXT NOT NULL,
                section_kind TEXT NOT NULL,
                expanded INTEGER NOT NULL,
                PRIMARY KEY (repo_path, section_kind)
            );
            INSERT INTO opened_repos (path, opened_at) VALUES ('/a', 100);
            INSERT INTO opened_repos (path, opened_at) VALUES ('/b', 200);
            "#,
        )
        .unwrap();
        assert_eq!(pragma_user_version(&conn), 0);
        run(&mut conn).unwrap();
        assert_eq!(pragma_user_version(&conn), MIGRATIONS.last().unwrap().0);
        let rows: Vec<(String, i64)> = conn
            .prepare("SELECT path, position FROM opened_repos ORDER BY position")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_>>()
            .unwrap();
        assert_eq!(rows, vec![("/b".into(), 0), ("/a".into(), 1)]);
    }
}

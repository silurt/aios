//! State database and migrations.
//!
//! Migrations are a plain ordered list keyed off `PRAGMA user_version`. That is
//! deliberately less machinery than a migration framework: the list is readable
//! in full, runs in a transaction, and adds no build-time macro expansion.

use crate::config;
use crate::error::Result;
use rusqlite::Connection;

/// Ordered, append-only. Never edit or reorder a migration that has shipped —
/// add a new one.
const MIGRATIONS: &[&str] = &[
    // 1: the project registry.
    r#"
    CREATE TABLE projects (
        id              TEXT PRIMARY KEY,
        slug            TEXT NOT NULL UNIQUE,
        name            TEXT NOT NULL,
        path            TEXT NOT NULL UNIQUE,
        git_remote      TEXT,
        default_branch  TEXT,
        languages       TEXT NOT NULL DEFAULT '[]',
        package_manager TEXT,
        issue_prefix    TEXT,
        created_at      TEXT NOT NULL,
        updated_at      TEXT NOT NULL
    );

    -- Tags are a table rather than a JSON column because they are queried.
    -- `languages` stays JSON: it is detected, display-only, and never filtered on.
    CREATE TABLE project_tags (
        project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        tag        TEXT NOT NULL,
        PRIMARY KEY (project_id, tag)
    );
    CREATE INDEX idx_project_tags_tag ON project_tags(tag);
    "#,
];

pub fn open() -> Result<Connection> {
    config::ensure_home()?;
    open_at(&config::state_db_path())
}

pub fn open_at(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    // WAL so a reader (a CLI invocation) never blocks the daemon's writes once
    // both exist. foreign_keys is off by default in SQLite and we rely on the
    // cascade above.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let target = MIGRATIONS.len() as i64;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        conn.execute_batch(&format!(
            "BEGIN; {sql} PRAGMA user_version = {}; COMMIT;",
            i + 1
        ))?;
    }
    debug_assert!(current <= target, "state.db is newer than this binary");
    Ok(())
}

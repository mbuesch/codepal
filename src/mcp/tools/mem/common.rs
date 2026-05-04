use rusqlite as sql;

pub const MEMORY_MAX_KEYS: usize = 64;
pub const MEMORY_MAX_KEY_LEN: usize = 256;
pub const MEMORY_MAX_VALUE_LEN: usize = 64 * 1024;
pub const MEMORY_DB_FILENAME: &str = ".agents-codepal-memory.sqlite";

/// Memory: Ensure the database schema for the key-value store exists.
pub fn create_mem_tables(conn: &sql::Connection) -> sql::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mem_values (
            id INTEGER PRIMARY KEY,
            value TEXT NOT NULL UNIQUE
        );
        CREATE TABLE IF NOT EXISTS memory (
            key TEXT PRIMARY KEY,
            value_id INTEGER NOT NULL REFERENCES mem_values(id),
            stored_at TEXT NOT NULL DEFAULT (datetime('now')),
            accessed_at TEXT NOT NULL DEFAULT (datetime('now')),
            access_count INTEGER NOT NULL DEFAULT 0
        );",
    )
}

/// Memory: Clean up values that are no longer referenced by any key.
pub fn prune_unreferenced_values(conn: &sql::Connection) -> sql::Result<()> {
    conn.execute(
        "DELETE FROM mem_values WHERE id NOT IN (SELECT value_id FROM memory)",
        [],
    )?;
    Ok(())
}

/// Memory: Prune entries that haven't been accessed in a while.
pub fn prune_expired_entries(conn: &sql::Connection, max_age_days: u64) -> sql::Result<()> {
    let modifier = format!("-{max_age_days} days");
    let n = conn.execute(
        "DELETE FROM memory WHERE accessed_at < datetime('now', ?1)",
        sql::params![modifier],
    )?;
    if n > 0 {
        eprintln!(
            "memory: pruned {n} expired entr{}.",
            if n == 1 { "y" } else { "ies" }
        );
        prune_unreferenced_values(conn)?;
    }
    Ok(())
}

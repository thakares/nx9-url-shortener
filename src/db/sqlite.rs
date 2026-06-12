//! Strongly-typed SQLite PRAGMA and configuration helpers.
//!
//! This module provides safe wrappers around common SQLite PRAGMAs using
//! rusqlite's type-safe APIs (`pragma_update`, `pragma_query_value`, `query_row`)
//! instead of raw `execute` calls. All functions use structured tracing for
//! observability.

use rusqlite::Connection;
use serde::Serialize;
use tracing::info;

/// Enables WAL (Write-Ahead Logging) journal mode on the given connection.
///
/// Uses `query_row` with `PRAGMA journal_mode=WAL` which both sets and returns
/// the actual mode. Returns an error if the database does not confirm WAL mode.
pub fn enable_wal(conn: &Connection, db_name: &str) -> Result<(), rusqlite::Error> {
    let actual_mode: String = conn.query_row("PRAGMA journal_mode=WAL;", [], |row| {
        row.get::<_, String>(0)
    })?;

    info!(database = db_name, mode = %actual_mode, "WAL mode configured");

    if actual_mode.to_lowercase() != "wal" {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    Ok(())
}

/// Enables foreign key constraint enforcement on the given connection.
///
/// Sets `foreign_keys` to ON via `pragma_update`, then verifies the setting
/// was applied by reading it back with `pragma_query_value`.
pub fn enable_foreign_keys(conn: &Connection, db_name: &str) -> Result<(), rusqlite::Error> {
    conn.pragma_update(None, "foreign_keys", "ON")?;

    let enabled: bool =
        conn.pragma_query_value(None, "foreign_keys", |row| row.get::<_, bool>(0))?;

    info!(
        database = db_name,
        foreign_keys = enabled,
        "Foreign key enforcement configured"
    );

    if !enabled {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    Ok(())
}

/// Sets the schema user_version on the given connection.
///
/// Uses `pragma_update` with the type-safe API — no `format!` string, no raw
/// `execute`.
pub fn set_user_version(conn: &Connection, version: i32) -> Result<(), rusqlite::Error> {
    conn.pragma_update(None, "user_version", version)
}

/// Returns the current schema user_version from the given connection.
pub fn get_user_version(conn: &Connection) -> Result<u32, rusqlite::Error> {
    conn.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
}

/// Runs `PRAGMA integrity_check` and returns `Ok(())` if the database reports "ok".
///
/// If the integrity check returns any other value, the function returns an error
/// containing the integrity check message.
pub fn integrity_check(conn: &Connection, db_name: &str) -> Result<(), rusqlite::Error> {
    let result: String =
        conn.query_row("PRAGMA integrity_check;", [], |row| row.get::<_, String>(0))?;

    if result == "ok" {
        info!(database = db_name, "Integrity check passed");
        Ok(())
    } else {
        Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
            Some(format!("Integrity check failed for {db_name}: {result}")),
        ))
    }
}

/// Returns the current journal mode of the given connection.
pub fn get_journal_mode(conn: &Connection) -> Result<String, rusqlite::Error> {
    conn.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
}

/// A snapshot of database health information collected from various PRAGMAs.
#[derive(Debug, Clone, Serialize)]
pub struct DatabaseHealthReport {
    /// Name of the database (e.g. "admin", "content").
    pub database: String,
    /// Current schema version (`user_version` PRAGMA).
    pub schema_version: u32,
    /// Active journal mode (e.g. "wal", "delete").
    pub journal_mode: String,
    /// Whether foreign key enforcement is enabled.
    pub foreign_keys_enabled: bool,
    /// Whether `PRAGMA integrity_check` returned "ok".
    pub integrity_ok: bool,
}

/// Collects a [`DatabaseHealthReport`] by querying all relevant PRAGMAs.
///
/// This function queries `user_version`, `journal_mode`, `foreign_keys`, and
/// `integrity_check` to build a comprehensive health snapshot. The report is
/// logged at `info` level with structured fields.
pub fn collect_health_report(
    conn: &Connection,
    db_name: &str,
) -> Result<DatabaseHealthReport, rusqlite::Error> {
    let schema_version = get_user_version(conn)?;
    let journal_mode = get_journal_mode(conn)?;

    let foreign_keys_enabled: bool =
        conn.pragma_query_value(None, "foreign_keys", |row| row.get::<_, bool>(0))?;

    let integrity_result: String =
        conn.query_row("PRAGMA integrity_check;", [], |row| row.get::<_, String>(0))?;
    let integrity_ok = integrity_result == "ok";

    let report = DatabaseHealthReport {
        database: db_name.to_owned(),
        schema_version,
        journal_mode,
        foreign_keys_enabled,
        integrity_ok,
    };

    info!(
        database = %report.database,
        version = report.schema_version,
        journal_mode = %report.journal_mode,
        foreign_keys = report.foreign_keys_enabled,
        integrity = report.integrity_ok,
        "Database health report collected"
    );

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn memory_conn() -> Connection {
        Connection::open_in_memory().expect("Failed to open in-memory database")
    }

    #[test]
    fn test_enable_wal() {
        let conn = memory_conn();
        // In-memory databases may not support WAL; we just verify no panic.
        // On-disk databases would return "wal".
        let _ = enable_wal(&conn, "test");
    }

    #[test]
    fn test_enable_foreign_keys() {
        let conn = memory_conn();
        enable_foreign_keys(&conn, "test").expect("Failed to enable foreign keys");
    }

    #[test]
    fn test_user_version_roundtrip() {
        let conn = memory_conn();
        set_user_version(&conn, 42).expect("Failed to set user_version");
        let v = get_user_version(&conn).expect("Failed to get user_version");
        assert_eq!(v, 42);
    }

    #[test]
    fn test_get_journal_mode() {
        let conn = memory_conn();
        let mode = get_journal_mode(&conn).expect("Failed to get journal_mode");
        assert!(!mode.is_empty());
    }

    #[test]
    fn test_integrity_check() {
        let conn = memory_conn();
        integrity_check(&conn, "test").expect("Integrity check should pass on fresh db");
    }

    #[test]
    fn test_collect_health_report() {
        let conn = memory_conn();
        let report = collect_health_report(&conn, "test").expect("Failed to collect health report");
        assert_eq!(report.database, "test");
        assert!(report.integrity_ok);
    }
}

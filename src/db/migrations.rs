use std::sync::Mutex;

use chrono::Utc;
use rusqlite::Connection;
use tracing::info;
use uuid::Uuid;

/// A single versioned migration with a human-readable name.
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// Run all pending migrations against `conn`, recording audit entries in `system_db_opt`.
///
/// Migrations are applied in order. Each migration runs inside a transaction,
/// and the schema version is bumped only after a successful commit.
pub fn run_migrations(
    conn: &mut Connection,
    db_name: &str,
    migrations: &[Migration],
    system_db_opt: Option<&Mutex<Connection>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let current_version = crate::db::sqlite::get_user_version(conn)?;
    let target_version = migrations.last().map_or(0, |m| m.version);

    if current_version < target_version {
        for m in migrations.iter().filter(|m| m.version > current_version) {
            info!(database = db_name, version = m.version, name = m.name, "Applying migration");

            let tx = conn.transaction()?;
            tx.execute_batch(m.sql)?;
            tx.commit()?;

            crate::db::sqlite::set_user_version(conn, m.version as i32)?;

            info!(database = db_name, version = m.version, name = m.name, "Migration completed");

            // Write audit record to system.db.migrations
            if let Some(sys_db_mutex) = system_db_opt {
                if let Ok(sys_db) = sys_db_mutex.lock() {
                    let id = Uuid::new_v4().to_string();
                    let now = Utc::now().to_rfc3339();
                    let _ = sys_db.execute(
                        "INSERT INTO migrations (id, db_name, version, applied_at) VALUES (?1, ?2, ?3, ?4);",
                        rusqlite::params![id, db_name, m.version as i32, now],
                    );
                }
            } else if db_name == "system" {
                // If migrating system.db itself, write directly to its own migrations table
                let id = Uuid::new_v4().to_string();
                let now = Utc::now().to_rfc3339();
                let _ = conn.execute(
                    "INSERT INTO migrations (id, db_name, version, applied_at) VALUES (?1, ?2, ?3, ?4);",
                    rusqlite::params![id, db_name, m.version as i32, now],
                );
            }
        }
    } else {
        info!(database = db_name, version = current_version, "Database up to date");
    }

    Ok(())
}

/// Print a dry-run migration plan to stdout without applying any changes.
pub fn print_migration_plan(
    conn: &Connection,
    db_name: &str,
    migrations: &[Migration],
) -> Result<(), Box<dyn std::error::Error>> {
    let current_version = crate::db::sqlite::get_user_version(conn)?;
    let target_version = migrations.last().map_or(0, |m| m.version);

    println!("Database: {db_name}");
    println!("  Current version: {current_version}");
    println!("  Target version:  {target_version}");

    let pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > current_version).collect();

    if pending.is_empty() {
        println!("  Status: up to date");
    } else {
        for m in pending {
            println!("  Would apply: v{} {}", m.version, m.name);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Migration definitions
// ---------------------------------------------------------------------------

pub const ADMIN_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: r#"
    CREATE TABLE IF NOT EXISTS users (
        id TEXT PRIMARY KEY,
        username TEXT NOT NULL UNIQUE,
        password_hash TEXT NOT NULL,
        created_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS api_keys (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        key_hash TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL,
        created_at TEXT NOT NULL,
        last_used_at TEXT,
        FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS audit_logs (
        id TEXT PRIMARY KEY,
        timestamp TEXT NOT NULL,
        username TEXT NOT NULL,
        action TEXT NOT NULL,
        object_type TEXT,
        object_id TEXT,
        ip_address TEXT,
        user_agent TEXT
    );

    CREATE TABLE IF NOT EXISTS config (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    "#,
    },
];

pub const CONTENT_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: r#"
    CREATE TABLE IF NOT EXISTS urls (
        id TEXT PRIMARY KEY,
        code TEXT NOT NULL UNIQUE,
        destination TEXT NOT NULL,
        title TEXT,
        description TEXT,
        status TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS landing_pages (
        id TEXT PRIMARY KEY,
        code TEXT NOT NULL UNIQUE,
        slug TEXT NOT NULL,
        title TEXT NOT NULL,
        html_content TEXT NOT NULL,
        state TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS tags (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL UNIQUE
    );

    CREATE TABLE IF NOT EXISTS url_tags (
        url_id TEXT NOT NULL,
        tag_id TEXT NOT NULL,
        PRIMARY KEY (url_id, tag_id),
        FOREIGN KEY(url_id) REFERENCES urls(id) ON DELETE CASCADE,
        FOREIGN KEY(tag_id) REFERENCES tags(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_urls_code ON urls(code);
    CREATE INDEX IF NOT EXISTS idx_pages_code ON landing_pages(code);
    "#,
    },
];

pub const ANALYTICS_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: r#"
    CREATE TABLE IF NOT EXISTS visits (
        id TEXT PRIMARY KEY,
        target_type TEXT NOT NULL,
        target_id TEXT NOT NULL,
        timestamp TEXT NOT NULL,
        ip_address TEXT NOT NULL,
        user_agent TEXT NOT NULL,
        referer TEXT NOT NULL,
        accept_language TEXT NOT NULL,
        country TEXT NOT NULL,
        status_code INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS daily_summaries (
        date TEXT NOT NULL,
        target_type TEXT NOT NULL,
        target_id TEXT NOT NULL,
        metric_type TEXT NOT NULL,
        metric_key TEXT NOT NULL,
        metric_value INTEGER NOT NULL,
        PRIMARY KEY (date, target_type, target_id, metric_type, metric_key)
    );

    CREATE TABLE IF NOT EXISTS monthly_summaries (
        year_month TEXT NOT NULL,
        target_type TEXT NOT NULL,
        target_id TEXT NOT NULL,
        metric_type TEXT NOT NULL,
        metric_key TEXT NOT NULL,
        metric_value INTEGER NOT NULL,
        PRIMARY KEY (year_month, target_type, target_id, metric_type, metric_key)
    );

    CREATE TABLE IF NOT EXISTS yearly_summaries (
        year TEXT NOT NULL,
        target_type TEXT NOT NULL,
        target_id TEXT NOT NULL,
        metric_type TEXT NOT NULL,
        metric_key TEXT NOT NULL,
        metric_value INTEGER NOT NULL,
        PRIMARY KEY (year, target_type, target_id, metric_type, metric_key)
    );

    CREATE INDEX IF NOT EXISTS idx_visits_timestamp ON visits(timestamp);
    CREATE INDEX IF NOT EXISTS idx_visits_target ON visits(target_type, target_id);
    "#,
    },
];

pub const SYSTEM_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: r#"
    CREATE TABLE IF NOT EXISTS migrations (
        id TEXT PRIMARY KEY,
        db_name TEXT NOT NULL,
        version INTEGER NOT NULL,
        applied_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS job_history (
        id TEXT PRIMARY KEY,
        job_name TEXT NOT NULL,
        status TEXT NOT NULL,
        started_at TEXT NOT NULL,
        finished_at TEXT,
        error_message TEXT
    );

    CREATE TABLE IF NOT EXISTS health_checks (
        id TEXT PRIMARY KEY,
        object_type TEXT NOT NULL,
        object_id TEXT NOT NULL,
        checked_at TEXT NOT NULL,
        status_code INTEGER,
        error_message TEXT,
        is_healthy INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS backup_history (
        id TEXT PRIMARY KEY,
        backup_path TEXT NOT NULL,
        status TEXT NOT NULL,
        created_at TEXT NOT NULL,
        size_bytes INTEGER,
        error_message TEXT
    );

    CREATE TABLE IF NOT EXISTS system_events (
        id TEXT PRIMARY KEY,
        event_type TEXT NOT NULL,
        timestamp TEXT NOT NULL,
        details TEXT NOT NULL
    );
    "#,
    },
];

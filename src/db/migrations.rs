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
            info!(
                database = db_name,
                version = m.version,
                name = m.name,
                "Applying migration"
            );

            let tx = conn.transaction()?;
            tx.execute_batch(m.sql)?;
            tx.commit()?;

            crate::db::sqlite::set_user_version(conn, m.version as i32)?;

            info!(
                database = db_name,
                version = m.version,
                name = m.name,
                "Migration completed"
            );

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
        info!(
            database = db_name,
            version = current_version,
            "Database up to date"
        );
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

    let pending: Vec<&Migration> = migrations
        .iter()
        .filter(|m| m.version > current_version)
        .collect();

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
    Migration {
        version: 2,
        name: "remove_api_keys_fk",
        sql: r#"
    CREATE TABLE api_keys_new (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        key_hash TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL,
        created_at TEXT NOT NULL,
        last_used_at TEXT
    );
    INSERT INTO api_keys_new (id, user_id, key_hash, name, created_at, last_used_at)
    SELECT id, user_id, key_hash, name, created_at, last_used_at FROM api_keys;
    DROP TABLE api_keys;
    ALTER TABLE api_keys_new RENAME TO api_keys;
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
    Migration {
        version: 2,
        name: "features_expansion",
        sql: r#"
    -- Expiring Links
    ALTER TABLE urls ADD COLUMN expires_at TEXT NULL;
    ALTER TABLE urls ADD COLUMN expired INTEGER NOT NULL DEFAULT 0;

    -- Password Protected Links
    ALTER TABLE urls ADD COLUMN password_hash TEXT NULL;

    -- Link Health Dashboard (extended columns)
    ALTER TABLE urls ADD COLUMN last_status TEXT;
    ALTER TABLE urls ADD COLUMN last_latency_ms INTEGER;

    -- One-Time Links
    ALTER TABLE urls ADD COLUMN max_access_count INTEGER NULL;
    ALTER TABLE urls ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0;

    -- Smart Landing Pages / Link Preview
    CREATE TABLE IF NOT EXISTS link_preview (
        id TEXT PRIMARY KEY,
        url_id TEXT NOT NULL UNIQUE,
        title TEXT,
        description TEXT,
        logo_url TEXT,
        button_text TEXT DEFAULT 'Continue',
        FOREIGN KEY(url_id) REFERENCES urls(id) ON DELETE CASCADE
    );

    -- QR Code style metadata
    CREATE TABLE IF NOT EXISTS qr_codes (
        id TEXT PRIMARY KEY,
        url_id TEXT NOT NULL,
        style TEXT NOT NULL DEFAULT 'default',
        created_at TEXT NOT NULL,
        FOREIGN KEY(url_id) REFERENCES urls(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_urls_expired ON urls(expired);
    CREATE INDEX IF NOT EXISTS idx_urls_expires_at ON urls(expires_at);
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
    Migration {
        version: 2,
        name: "qr_access_log",
        sql: r#"
    CREATE TABLE IF NOT EXISTS qr_access_log (
        id TEXT PRIMARY KEY,
        url_id TEXT NOT NULL,
        timestamp TEXT NOT NULL,
        ip TEXT,
        user_agent TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_qr_access_url ON qr_access_log(url_id);
    CREATE INDEX IF NOT EXISTS idx_qr_access_ts ON qr_access_log(timestamp);
    "#,
    },
    Migration {
        version: 3,
        name: "add_owner_user_id",
        sql: "ALTER TABLE visits ADD COLUMN owner_user_id INTEGER;",
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
    Migration {
        version: 2,
        name: "audit_events",
        sql: r#"
    CREATE TABLE IF NOT EXISTS audit_events (
        id TEXT PRIMARY KEY,
        actor TEXT NOT NULL,
        action TEXT NOT NULL,
        object_type TEXT NOT NULL,
        object_id TEXT NOT NULL,
        timestamp TEXT NOT NULL,
        metadata TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_events(actor);
    CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit_events(timestamp);
    CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_events(action);
    "#,
    },
    Migration {
        version: 3,
        name: "global_slugs_and_moderation",
        sql: r#"
    CREATE TABLE IF NOT EXISTS global_slugs (
        slug TEXT PRIMARY KEY,
        owner_user_id INTEGER NOT NULL,
        target_type TEXT NOT NULL,
        target_id TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        status TEXT NOT NULL,
        deleted_at TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_global_slugs_owner ON global_slugs(owner_user_id);
    CREATE INDEX IF NOT EXISTS idx_global_slugs_status ON global_slugs(status);
    CREATE INDEX IF NOT EXISTS idx_global_slugs_target ON global_slugs(target_type, target_id);

    CREATE TABLE IF NOT EXISTS moderation_events (
        id TEXT PRIMARY KEY,
        timestamp TEXT NOT NULL,
        admin_username TEXT NOT NULL,
        target_user_id INTEGER NOT NULL,
        target_username TEXT,
        resource_type TEXT NOT NULL,
        resource_identifier TEXT NOT NULL,
        action TEXT NOT NULL,
        severity TEXT NOT NULL,
        reason TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS slug_history (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        slug TEXT NOT NULL,
        old_owner_user_id INTEGER,
        new_owner_user_id INTEGER,
        action TEXT NOT NULL,
        timestamp TEXT NOT NULL,
        admin_username TEXT
    );

    CREATE TABLE IF NOT EXISTS reserved_slugs (
        slug TEXT PRIMARY KEY,
        reason TEXT
    );

    CREATE TABLE IF NOT EXISTS schema_version (
        version INTEGER PRIMARY KEY,
        applied_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    -- Seed defaults
    INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (3, datetime('now'));
    
    INSERT OR IGNORE INTO settings (key, value) VALUES ('soft_delete_retention_days', '30');
    INSERT OR IGNORE INTO settings (key, value) VALUES ('quota_reconcile_interval_hours', '24');
    INSERT OR IGNORE INTO settings (key, value) VALUES ('allow_registration', 'false');
    INSERT OR IGNORE INTO settings (key, value) VALUES ('maintenance_mode', 'false');

    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('admin', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('login', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('logout', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('dashboard', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('api', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('docs', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('assets', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('static', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('favicon.ico', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('robots.txt', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('health', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('metrics', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('install', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('setup', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('support', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('help', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('security', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('abuse', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('billing', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('status', 'System route');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('legacy_admin', 'System reserved');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('administrator', 'System reserved');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('system', 'System reserved');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('root', 'System reserved');
    INSERT OR IGNORE INTO reserved_slugs (slug, reason) VALUES ('www', 'System reserved');
    "#,
    },
];

pub const USERS_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: r#"
    CREATE TABLE IF NOT EXISTS users (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        username TEXT UNIQUE NOT NULL,
        password_hash TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'active',
        created_at TEXT NOT NULL,
        last_login TEXT,
        account_type TEXT DEFAULT 'standard',
        organization_id INTEGER NULL,
        metadata TEXT
    );

    CREATE TABLE IF NOT EXISTS quotas (
        user_id INTEGER PRIMARY KEY,
        max_urls INTEGER DEFAULT 100,
        max_landings INTEGER DEFAULT 10,
        max_api_tokens INTEGER DEFAULT 5,
        max_storage_mb INTEGER DEFAULT 100,
        current_urls INTEGER DEFAULT 0,
        current_landings INTEGER DEFAULT 0,
        current_api_tokens INTEGER DEFAULT 0,
        current_storage_mb INTEGER DEFAULT 0,
        FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS api_tokens (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        user_id INTEGER NOT NULL,
        token_hash TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY,
        user_id INTEGER NOT NULL,
        expires_at TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS username_history (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        user_id INTEGER NOT NULL,
        old_username TEXT NOT NULL,
        new_username TEXT NOT NULL,
        changed_at TEXT NOT NULL,
        FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
    );
    "#,
    },
    Migration {
        version: 2,
        name: "repair_admin_account_type",
        sql: r#"
    UPDATE users
    SET account_type = 'admin'
    WHERE username = 'admin' AND account_type = 'standard';
    "#,
    },
];

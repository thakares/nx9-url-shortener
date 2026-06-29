use crate::config::Config;
use crate::db::migrations::{
    run_migrations, ADMIN_MIGRATIONS, ANALYTICS_MIGRATIONS, CONTENT_MIGRATIONS, SYSTEM_MIGRATIONS,
    USERS_MIGRATIONS,
};
use crate::db::sqlite::{enable_foreign_keys, enable_wal};
use rusqlite::Connection;
use std::fs;
use std::sync::{Arc, Mutex};

pub mod admin;
pub mod analytics;
pub mod audit_events;
pub mod content;
pub mod migrations;
pub mod preview;
pub mod qr;
pub mod sqlite;
pub mod users;

#[derive(Clone)]
pub struct Db {
    pub admin: Arc<Mutex<Connection>>,
    pub content: Arc<Mutex<Connection>>,
    pub analytics: Arc<Mutex<Connection>>,
    pub system: Arc<Mutex<Connection>>,
    pub users: Arc<Mutex<Connection>>,
    pub data_dir: std::path::PathBuf,
}

impl Db {
    pub fn init(config: &Config) -> Result<Self, Box<dyn std::error::Error>> {
        use chrono::Utc;
        use tracing::info;

        // Ensure data directory exists
        if !config.data_dir.exists() {
            fs::create_dir_all(&config.data_dir)?;
        }

        let admin_dir = config.data_dir.join("admin");
        let users_dir = config.data_dir.join("users");
        fs::create_dir_all(&admin_dir)?;
        fs::create_dir_all(&users_dir)?;

        // Automated Legacy Migration: check if legacy files are at the root
        let legacy_admin_db = config.data_dir.join("admin.db");
        let legacy_content_db = config.data_dir.join("content.db");
        let legacy_analytics_db = config.data_dir.join("analytics.db");

        // 1. If legacy admin.db exists at root, move admin/system DBs to config.data_dir/admin/
        if legacy_admin_db.exists() {
            tracing::warn!("LEGACY DETECTED: admin.db found at root. Moving administrative databases to multi-tenant admin/ subfolder...");
            let files = vec![
                "admin.db",
                "admin.db-wal",
                "admin.db-shm",
                "system.db",
                "system.db-wal",
                "system.db-shm",
            ];
            for f in files {
                let src = config.data_dir.join(f);
                if src.exists() {
                    let dst = admin_dir.join(f);
                    let _ = fs::rename(&src, &dst);
                }
            }
        }

        // Pre-migration safety net: audit slug namespace for duplicates / format errors
        match crate::db::users::audit_slug_namespace(config) {
            Ok(report) => {
                if !report.duplicates.is_empty() {
                    tracing::error!(
                        "Namespace conflicts detected before database migration: {:?}",
                        report.duplicates
                    );
                    return Err(format!(
                        "Database upgrade aborted due to slug conflicts: {:?}",
                        report.duplicates
                    )
                    .into());
                }
            }
            Err(e) => {
                tracing::warn!("Failed to audit slug namespace before migration: {}", e);
            }
        }

        let admin_path = admin_dir.join("admin.db");
        let system_path = admin_dir.join("system.db");
        let users_db_path = admin_dir.join("users.db");

        info!("Opening admin.db");
        let mut admin_conn = Connection::open(admin_path)?;
        info!("Opening system.db");
        let mut system_conn = Connection::open(system_path)?;
        info!("Opening users.db");
        let mut users_conn = Connection::open(users_db_path)?;

        enable_wal(&admin_conn, "admin")?;
        enable_wal(&system_conn, "system")?;
        enable_wal(&users_conn, "users")?;

        enable_foreign_keys(&admin_conn, "admin")?;
        enable_foreign_keys(&system_conn, "system")?;
        enable_foreign_keys(&users_conn, "users")?;

        // Run migrations for system.db first
        info!("Running system migrations");
        run_migrations(&mut system_conn, "system", SYSTEM_MIGRATIONS, None)?;
        let system_arc = Arc::new(Mutex::new(system_conn));

        // Pre-migration detection of admin account repair
        let repair_needed = {
            let stmt = users_conn.prepare(
                "SELECT EXISTS(SELECT 1 FROM users WHERE username = 'admin' AND account_type = 'standard');"
            );
            match stmt {
                Ok(mut s) => s
                    .query_row([], |row| row.get::<_, bool>(0))
                    .unwrap_or(false),
                Err(_) => false,
            }
        };

        // Run migrations for admin.db and users.db
        info!("Running admin migrations");
        run_migrations(
            &mut admin_conn,
            "admin",
            ADMIN_MIGRATIONS,
            Some(&system_arc),
        )?;
        info!("Running users migrations");
        run_migrations(
            &mut users_conn,
            "users",
            USERS_MIGRATIONS,
            Some(&system_arc),
        )?;

        // Post-migration: audit log if repaired
        if repair_needed {
            let admin_is_now_admin: bool = users_conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM users WHERE username = 'admin' AND account_type = 'admin');",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(false);

            if admin_is_now_admin {
                let system_conn = system_arc.lock().unwrap();
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    "admin",
                    "migration_repair",
                    "users",
                    "admin",
                    Some("Repaired standard account type to admin"),
                );
            }
        }

        // Clean up expired sessions from users.db on startup
        let now = Utc::now().to_rfc3339();
        let _ = users_conn.execute("DELETE FROM sessions WHERE expires_at < ?1;", [now]);

        // 2. If legacy content.db/analytics.db exists, move them to users/1/ (for legacy_admin)
        let legacy_migration_needed = legacy_content_db.exists() || legacy_analytics_db.exists();

        // Ensure legacy_admin (user ID 1) exists in users.db
        let legacy_admin_id = 1i64;
        let legacy_admin_exists: bool = users_conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1);",
                [legacy_admin_id],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !legacy_admin_exists {
            // Get copied administrator password hash
            let admin_password_hash: String = admin_conn
                .query_row(
                    "SELECT password_hash FROM users ORDER BY created_at ASC LIMIT 1;",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| {
                    // If admin_db is empty, hash a default password
                    crate::auth::password::hash_password("legacy_admin_pass").unwrap_or_default()
                });

            let now = Utc::now().to_rfc3339();
            users_conn.execute(
                "INSERT INTO users (id, username, password_hash, status, created_at, account_type) 
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
                rusqlite::params![
                    legacy_admin_id,
                    "legacy_admin",
                    admin_password_hash,
                    "disabled",
                    now,
                    "system"
                ],
            )?;

            // Seed quotas
            users_conn.execute(
                "INSERT INTO quotas (user_id) VALUES (?1);",
                [legacy_admin_id],
            )?;
        }

        let legacy_user_dir = users_dir.join(legacy_admin_id.to_string());
        fs::create_dir_all(&legacy_user_dir)?;

        if legacy_content_db.exists() || legacy_analytics_db.exists() {
            tracing::warn!("LEGACY DETECTED: content/analytics databases found at root. Moving to multi-tenant user ID 1 directory...");
            let content_files = vec!["content.db", "content.db-wal", "content.db-shm"];
            for f in content_files {
                let src = config.data_dir.join(f);
                if src.exists() {
                    let dst = legacy_user_dir.join(f);
                    let _ = fs::rename(&src, &dst);
                }
            }
            let analytics_files = vec!["analytics.db", "analytics.db-wal", "analytics.db-shm"];
            for f in analytics_files {
                let src = config.data_dir.join(f);
                if src.exists() {
                    let dst = legacy_user_dir.join(f);
                    let _ = fs::rename(&src, &dst);
                }
            }
        }

        // Open the legacy_admin databases (user ID 1) as db.content and db.analytics
        let content_path = legacy_user_dir.join("content.db");
        let analytics_path = legacy_user_dir.join("analytics.db");

        let mut content_conn = Connection::open(content_path)?;
        let mut analytics_conn = Connection::open(analytics_path)?;

        enable_wal(&content_conn, "content")?;
        enable_wal(&analytics_conn, "analytics")?;

        enable_foreign_keys(&content_conn, "content")?;
        enable_foreign_keys(&analytics_conn, "analytics")?;

        // Run migrations for content.db and analytics.db
        run_migrations(
            &mut content_conn,
            "content",
            CONTENT_MIGRATIONS,
            Some(&system_arc),
        )?;
        run_migrations(
            &mut analytics_conn,
            "analytics",
            ANALYTICS_MIGRATIONS,
            Some(&system_arc),
        )?;

        // If we just migrated legacy content, populate the global_slugs table in system.db
        if legacy_migration_needed {
            info!("Populating global slug index with legacy content...");
            let mut sys_lock = system_arc.lock().unwrap();
            let tx = sys_lock.transaction()?;

            // Extract urls from content.db and insert into global_slugs
            {
                let mut stmt =
                    content_conn.prepare("SELECT code, id, created_at, status FROM urls;")?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let slug: String = row.get(0)?;
                    let target_id: String = row.get(1)?;
                    let created_at: String = row.get(2)?;
                    let status: String = row.get(3)?;
                    let global_status = if status == "dead" {
                        "disabled"
                    } else {
                        "active"
                    };
                    let now = Utc::now().to_rfc3339();

                    let _ = tx.execute(
                        "INSERT OR IGNORE INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                        rusqlite::params![slug, legacy_admin_id, "url", target_id, created_at, now, global_status],
                    );
                }
            }

            // Extract landing pages from content.db and insert into global_slugs
            {
                let mut stmt = content_conn
                    .prepare("SELECT code, id, created_at, state FROM landing_pages;")?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let slug: String = row.get(0)?;
                    let target_id: String = row.get(1)?;
                    let created_at: String = row.get(2)?;
                    let state: String = row.get(3)?;
                    let now = Utc::now().to_rfc3339();

                    let status = if state == "published" {
                        "active"
                    } else {
                        "disabled"
                    };

                    let _ = tx.execute(
                        "INSERT OR IGNORE INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                        rusqlite::params![slug, legacy_admin_id, "page", target_id, created_at, now, status],
                    );
                }
            }

            tx.commit()?;
            info!("Global slug index populated successfully.");
        }

        let db = Self {
            admin: Arc::new(Mutex::new(admin_conn)),
            content: Arc::new(Mutex::new(content_conn)),
            analytics: Arc::new(Mutex::new(analytics_conn)),
            system: system_arc,
            users: Arc::new(Mutex::new(users_conn)),
            data_dir: config.data_dir.clone(),
        };

        let _ = db.reconcile_global_slugs(config);

        // Post-init: Clean up stale reservations
        {
            let system_conn = db.system.lock().unwrap();
            match crate::db::users::cleanup_stale_reservations(&system_conn, &config.data_dir) {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!("Cleaned up {} stale reserving slugs", count);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to clean up stale reservations: {}", e);
                }
            }
        }

        // Post-init: Verify global registry integrity
        {
            let system_conn = db.system.lock().unwrap();
            let users_conn = db.users.lock().unwrap();
            match crate::db::users::verify_global_slug_registry_integrity(
                &system_conn,
                &users_conn,
                &config.data_dir,
            ) {
                Ok((errors, warnings)) => {
                    for err in errors {
                        tracing::error!("Global registry integrity error: {}", err);
                    }
                    for warn in warnings {
                        tracing::warn!("Global registry integrity warning: {}", warn);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to verify global registry integrity: {}", e);
                }
            }
        }

        Ok(db)
    }

    pub fn compact(&self) -> Result<(), rusqlite::Error> {
        let admin = self.admin.lock().unwrap();
        let _ = admin.execute("VACUUM;", []);

        let content = self.content.lock().unwrap();
        let _ = content.execute("VACUUM;", []);

        let analytics = self.analytics.lock().unwrap();
        let _ = analytics.execute("VACUUM;", []);

        let system = self.system.lock().unwrap();
        let _ = system.execute("VACUUM;", []);

        let users = self.users.lock().unwrap();
        let _ = users.execute("VACUUM;", []);

        Ok(())
    }

    pub fn init_user_databases(&self, user_id: i64) -> Result<(), Box<dyn std::error::Error>> {
        let user_dir = self.data_dir.join("users").join(user_id.to_string());
        fs::create_dir_all(&user_dir)?;

        let content_path = user_dir.join("content.db");
        let analytics_path = user_dir.join("analytics.db");
        let profile_path = user_dir.join("profile.db");

        let mut content_conn = Connection::open(content_path)?;
        let mut analytics_conn = Connection::open(analytics_path)?;
        let profile_conn = Connection::open(profile_path)?;

        enable_wal(&content_conn, "content")?;
        enable_wal(&analytics_conn, "analytics")?;
        enable_wal(&profile_conn, "profile")?;

        enable_foreign_keys(&content_conn, "content")?;
        enable_foreign_keys(&analytics_conn, "analytics")?;
        enable_foreign_keys(&profile_conn, "profile")?;

        run_migrations(
            &mut content_conn,
            "content",
            CONTENT_MIGRATIONS,
            Some(&self.system),
        )?;
        run_migrations(
            &mut analytics_conn,
            "analytics",
            ANALYTICS_MIGRATIONS,
            Some(&self.system),
        )?;

        profile_conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;

        Ok(())
    }

    pub fn reconcile_global_slugs(
        &self,
        config: &Config,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use chrono::Utc;

        let system_conn = self.system.lock().unwrap();
        let users_conn = self.users.lock().unwrap();

        // Get all user IDs
        let mut stmt = users_conn.prepare("SELECT id FROM users;")?;
        let mut rows = stmt.query([])?;
        let mut user_ids = vec![1i64]; // Start with legacy admin
        while let Some(row) = rows.next()? {
            user_ids.push(row.get(0)?);
        }
        drop(rows);
        drop(stmt);

        for user_id in user_ids {
            let user_dir = config.data_dir.join("users").join(user_id.to_string());
            let content_path = user_dir.join("content.db");

            if content_path.exists() {
                let content_conn = Connection::open(&content_path)?;

                // Sync URLs
                let mut stmt =
                    content_conn.prepare("SELECT code, id, created_at, status FROM urls;")?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let code: String = row.get(0)?;
                    let target_id: String = row.get(1)?;
                    let created_at: String = row.get(2)?;
                    let status: String = row.get(3)?;
                    let global_status = if status == "dead" {
                        "disabled"
                    } else {
                        "active"
                    };
                    let now = Utc::now().to_rfc3339();

                    let _ = system_conn.execute(
                        "INSERT OR IGNORE INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                        rusqlite::params![code, user_id, "url", target_id, created_at, now, global_status],
                    );
                }

                // Sync Landing Pages
                let mut stmt = content_conn
                    .prepare("SELECT code, id, created_at, state FROM landing_pages;")?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let code: String = row.get(0)?;
                    let target_id: String = row.get(1)?;
                    let created_at: String = row.get(2)?;
                    let state: String = row.get(3)?;
                    let global_status = if state == "published" {
                        "active"
                    } else {
                        "disabled"
                    };
                    let now = Utc::now().to_rfc3339();

                    let _ = system_conn.execute(
                        "INSERT OR IGNORE INTO global_slugs (slug, owner_user_id, target_type, target_id, created_at, updated_at, status) 
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                        rusqlite::params![code, user_id, "page", target_id, created_at, now, global_status],
                    );
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod db_init_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_db_init() {
        let temp_dir = PathBuf::from("./temp_test_db_dir");
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        let mut config = Config::load();
        config.data_dir = temp_dir.clone();
        let db = Db::init(&config);

        // Cleanup
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }

        assert!(db.is_ok(), "Failed to init DB: {:?}", db.err());
    }
}

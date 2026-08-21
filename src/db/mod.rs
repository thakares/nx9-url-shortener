use crate::config::Config;
use crate::db::migrations::{
    run_migrations, ADMIN_MIGRATIONS, ANALYTICS_MIGRATIONS, CONTENT_MIGRATIONS, SYSTEM_MIGRATIONS,
    USERS_MIGRATIONS,
};
use crate::db::sqlite::{enable_foreign_keys, enable_wal};
use rusqlite::Connection;
use std::fs;
use std::sync::{Arc, Mutex};
use tracing::info;

pub mod admin;
pub mod analytics;
pub mod audit_events;
pub mod content;
pub mod identity_migrate;
pub mod migrations;
pub mod preview;
pub mod qr;
pub mod schema_v08;
pub mod slug_migrate;
pub mod slugs;
pub mod sqlite;
pub mod tenant;
pub mod topology;
pub mod users;

use crate::db::schema_v08::{
    GLOBAL_LANDING_PAGES_MIGRATIONS, GLOBAL_URLS_MIGRATIONS, RESERVED_SLUGS_MIGRATIONS,
};
use crate::db::topology::Topology;

#[derive(Clone)]
pub struct Db {
    pub admin: Arc<Mutex<Connection>>,
    pub system: Arc<Mutex<Connection>>,
    pub users: Arc<Mutex<Connection>>,
    pub global_urls: Arc<Mutex<Connection>>,
    pub global_landing_pages: Arc<Mutex<Connection>>,
    pub reserved: Arc<Mutex<Connection>>,
    pub data_dir: std::path::PathBuf,
    pub topology: Topology,
}

fn open_prepared(
    path: &std::path::Path,
    name: &str,
) -> Result<Connection, Box<dyn std::error::Error>> {
    info!("Opening {}.db", name);
    let conn = Connection::open(path)?;
    enable_wal(&conn, name)?;
    enable_foreign_keys(&conn, name)?;
    Ok(conn)
}

impl Db {
    pub fn init(config: &Config) -> Result<Self, Box<dyn std::error::Error>> {
        use chrono::Utc;

        let topology = Topology::new(&config.data_dir);

        if !topology.root().exists() {
            fs::create_dir_all(topology.root())?;
        }
        topology.ensure_core_dirs()?;

        let admin_dir = topology.admin_dir();

        // Automated Legacy Migration: check if legacy files are at the root
        let legacy_admin_db = topology.legacy_flat_admin_db();

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
                let src = topology.root().join(f);
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

        let mut admin_conn = open_prepared(&topology.admin_db(), "admin")?;
        let mut system_conn = open_prepared(&topology.system_db(), "system")?;
        let mut users_conn = open_prepared(&topology.users_registry_db(), "users")?;

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

        let mut global_urls_conn = open_prepared(&topology.global_urls_db(), "global_urls")?;
        let mut global_landing_pages_conn =
            open_prepared(&topology.global_landing_pages_db(), "global_landing_pages")?;
        let mut reserved_conn = open_prepared(&topology.reserved_db(), "reserved")?;

        run_migrations(
            &mut global_urls_conn,
            "global_urls",
            GLOBAL_URLS_MIGRATIONS,
            Some(&system_arc),
        )?;
        run_migrations(
            &mut global_landing_pages_conn,
            "global_landing_pages",
            GLOBAL_LANDING_PAGES_MIGRATIONS,
            Some(&system_arc),
        )?;
        run_migrations(
            &mut reserved_conn,
            "reserved",
            RESERVED_SLUGS_MIGRATIONS,
            Some(&system_arc),
        )?;
        crate::db::slugs::seed_reserved_slugs(&reserved_conn)?;

        let db = Self {
            admin: Arc::new(Mutex::new(admin_conn)),
            system: system_arc,
            users: Arc::new(Mutex::new(users_conn)),
            global_urls: Arc::new(Mutex::new(global_urls_conn)),
            global_landing_pages: Arc::new(Mutex::new(global_landing_pages_conn)),
            reserved: Arc::new(Mutex::new(reserved_conn)),
            data_dir: config.data_dir.clone(),
            topology,
        };

        // Post-init: Clean up stale reservations from v0.8 slug databases (older than 15 mins)
        {
            if let (Ok(urls_conn), Ok(pages_conn)) =
                (db.global_urls.lock(), db.global_landing_pages.lock())
            {
                match crate::db::slugs::cleanup_stale_reservations(&urls_conn, &pages_conn, 900) {
                    Ok(count) => {
                        if count > 0 {
                            tracing::info!(
                                "Cleaned up {} stale reserving slugs from v0.8 registry",
                                count
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to clean up stale reservations: {}", e);
                    }
                }
            }
        }

        Ok(db)
    }

    pub fn compact(&self) -> Result<(), rusqlite::Error> {
        let admin = self.admin.lock().unwrap();
        let _ = admin.execute("VACUUM;", []);

        let system = self.system.lock().unwrap();
        let _ = system.execute("VACUUM;", []);

        let users = self.users.lock().unwrap();
        let _ = users.execute("VACUUM;", []);

        let global_urls = self.global_urls.lock().unwrap();
        let _ = global_urls.execute("VACUUM;", []);

        let global_landing_pages = self.global_landing_pages.lock().unwrap();
        let _ = global_landing_pages.execute("VACUUM;", []);

        let reserved = self.reserved.lock().unwrap();
        let _ = reserved.execute("VACUUM;", []);

        Ok(())
    }

    pub fn init_user_databases(&self, user_id: i64) -> Result<(), Box<dyn std::error::Error>> {
        let user = {
            let conn = self.users.lock().unwrap();
            crate::db::users::get_user_by_id(&conn, user_id)?
                .ok_or_else(|| format!("cannot provision databases for unknown user {user_id}"))?
        };
        let location = crate::db::tenant::location_for_user(&user)?;
        let user_dir = location.dir(&self.topology)?;
        fs::create_dir_all(user_dir.join("extensions"))?;

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
        _config: &Config,
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
            let content_path = match self.topology.content_db_i64(user_id) {
                Ok(p) => p,
                Err(_) => continue,
            };

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

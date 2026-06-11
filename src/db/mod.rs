use std::fs;
use std::sync::{Arc, Mutex};
use rusqlite::Connection;
use crate::config::Config;
use crate::db::migrations::{run_migrations, ADMIN_MIGRATIONS, CONTENT_MIGRATIONS, ANALYTICS_MIGRATIONS, SYSTEM_MIGRATIONS};
use crate::db::sqlite::{enable_foreign_keys, enable_wal};

pub mod migrations;
pub mod sqlite;
pub mod admin;
pub mod content;
pub mod analytics;

#[derive(Clone)]
pub struct Db {
    pub admin: Arc<Mutex<Connection>>,
    pub content: Arc<Mutex<Connection>>,
    pub analytics: Arc<Mutex<Connection>>,
    pub system: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn init(config: &Config) -> Result<Self, Box<dyn std::error::Error>> {
        // Ensure data directory exists
        if !config.data_dir.exists() {
            fs::create_dir_all(&config.data_dir)?;
        }

        let admin_path = config.data_dir.join("admin.db");
        let content_path = config.data_dir.join("content.db");
        let analytics_path = config.data_dir.join("analytics.db");
        let system_path = config.data_dir.join("system.db");

        use tracing::info;

        info!("Opening admin.db");
        let mut admin_conn = Connection::open(admin_path)?;
        info!("Opening content.db");
        let mut content_conn = Connection::open(content_path)?;
        info!("Opening analytics.db");
        let mut analytics_conn = Connection::open(analytics_path)?;
        info!("Opening system.db");
        let mut system_conn = Connection::open(system_path)?;

        // Enable WAL mode for better concurrency and write performance
        info!(database = "admin", "Enabling WAL mode on admin.db");
        enable_wal(&admin_conn, "admin")?;
        info!(database = "content", "Enabling WAL mode on content.db");
        enable_wal(&content_conn, "content")?;
        info!(database = "analytics", "Enabling WAL mode on analytics.db");
        enable_wal(&analytics_conn, "analytics")?;
        info!(database = "system", "Enabling WAL mode on system.db");
        enable_wal(&system_conn, "system")?;

        // Enable foreign key support
        info!(database = "admin", "Enabling foreign key enforcement on admin.db");
        enable_foreign_keys(&admin_conn, "admin")?;
        info!(database = "content", "Enabling foreign key enforcement on content.db");
        enable_foreign_keys(&content_conn, "content")?;
        info!(database = "analytics", "Enabling foreign key enforcement on analytics.db");
        enable_foreign_keys(&analytics_conn, "analytics")?;
        info!(database = "system", "Enabling foreign key enforcement on system.db");
        enable_foreign_keys(&system_conn, "system")?;

        // 1. Run migrations for system.db first, as it receives secondary audit records
        info!("Running system migrations");
        run_migrations(&mut system_conn, "system", SYSTEM_MIGRATIONS, None)?;

        let system_arc = Arc::new(Mutex::new(system_conn));

        // 2. Run migrations for other databases with system.db logging
        info!("Running admin migrations");
        run_migrations(&mut admin_conn, "admin", ADMIN_MIGRATIONS, Some(&system_arc))?;
        info!("Running content migrations");
        run_migrations(&mut content_conn, "content", CONTENT_MIGRATIONS, Some(&system_arc))?;
        info!("Running analytics migrations");
        run_migrations(&mut analytics_conn, "analytics", ANALYTICS_MIGRATIONS, Some(&system_arc))?;

        Ok(Self {
            admin: Arc::new(Mutex::new(admin_conn)),
            content: Arc::new(Mutex::new(content_conn)),
            analytics: Arc::new(Mutex::new(analytics_conn)),
            system: system_arc,
        })
    }

    pub fn compact(&self) -> Result<(), rusqlite::Error> {
        let admin = self.admin.lock().unwrap();
        admin.execute("VACUUM;", [])?;

        let content = self.content.lock().unwrap();
        content.execute("VACUUM;", [])?;

        let analytics = self.analytics.lock().unwrap();
        analytics.execute("VACUUM;", [])?;

        let system = self.system.lock().unwrap();
        system.execute("VACUUM;", [])?;

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

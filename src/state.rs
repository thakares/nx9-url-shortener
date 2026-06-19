use crate::analytics::queue::AnalyticsQueue;
use crate::config::Config;
use crate::db::Db;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone)]
pub struct UserDbs {
    pub content: Arc<Mutex<Connection>>,
    pub analytics: Arc<Mutex<Connection>>,
    pub profile: Arc<Mutex<Connection>>,
}

#[derive(Clone)]
pub struct AppState {
    pub admin_db: Arc<Mutex<Connection>>,
    pub content_db: Arc<Mutex<Connection>>,
    pub analytics_db: Arc<Mutex<Connection>>,
    pub system_db: Arc<Mutex<Connection>>,
    pub users_db: Arc<Mutex<Connection>>,
    pub user_dbs: Arc<Mutex<HashMap<i64, UserDbs>>>,
    pub db: Db,
    pub config: Config,
    pub analytics_queue: AnalyticsQueue,
    pub start_time: Instant,
}

impl AppState {
    pub fn get_user_dbs(&self, user_id: i64) -> Result<UserDbs, crate::error::AppError> {
        let mut pool = self.user_dbs.lock().unwrap();
        if let Some(dbs) = pool.get(&user_id) {
            return Ok(dbs.clone());
        }

        // Open connection and run migrations
        let user_dir = self.config.data_dir.join("users").join(user_id.to_string());
        std::fs::create_dir_all(&user_dir)?;

        let content_path = user_dir.join("content.db");
        let analytics_path = user_dir.join("analytics.db");
        let profile_path = user_dir.join("profile.db");

        let mut content_conn = Connection::open(content_path)?;
        let mut analytics_conn = Connection::open(analytics_path)?;
        let profile_conn = Connection::open(profile_path)?;

        crate::db::sqlite::enable_wal(&content_conn, "content")?;
        crate::db::sqlite::enable_wal(&analytics_conn, "analytics")?;
        crate::db::sqlite::enable_wal(&profile_conn, "profile")?;

        crate::db::sqlite::enable_foreign_keys(&content_conn, "content")?;
        crate::db::sqlite::enable_foreign_keys(&analytics_conn, "analytics")?;
        crate::db::sqlite::enable_foreign_keys(&profile_conn, "profile")?;

        // Run migrations
        crate::db::migrations::run_migrations(
            &mut content_conn,
            "content",
            crate::db::migrations::CONTENT_MIGRATIONS,
            Some(&self.system_db),
        )
        .map_err(|e| crate::error::AppError::Internal(e.to_string()))?;
        crate::db::migrations::run_migrations(
            &mut analytics_conn,
            "analytics",
            crate::db::migrations::ANALYTICS_MIGRATIONS,
            Some(&self.system_db),
        )
        .map_err(|e| crate::error::AppError::Internal(e.to_string()))?;

        profile_conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;

        let dbs = UserDbs {
            content: Arc::new(Mutex::new(content_conn)),
            analytics: Arc::new(Mutex::new(analytics_conn)),
            profile: Arc::new(Mutex::new(profile_conn)),
        };

        pool.insert(user_id, dbs.clone());
        Ok(dbs)
    }

    pub fn db_compact(&self) -> Result<(), rusqlite::Error> {
        self.admin_db.lock().unwrap().execute("VACUUM;", [])?;
        self.content_db.lock().unwrap().execute("VACUUM;", [])?;
        self.analytics_db.lock().unwrap().execute("VACUUM;", [])?;
        self.system_db.lock().unwrap().execute("VACUUM;", [])?;
        self.users_db.lock().unwrap().execute("VACUUM;", [])?;
        Ok(())
    }
}

use std::sync::{Arc, Mutex};
use std::time::Instant;
use rusqlite::Connection;
use crate::config::Config;
use crate::analytics::queue::AnalyticsQueue;
use crate::db::Db;

#[derive(Clone)]
pub struct AppState {
    pub admin_db: Arc<Mutex<Connection>>,
    pub content_db: Arc<Mutex<Connection>>,
    pub analytics_db: Arc<Mutex<Connection>>,
    pub system_db: Arc<Mutex<Connection>>,
    pub db: Db,
    pub config: Config,
    pub analytics_queue: AnalyticsQueue,
    pub start_time: Instant,
}

impl AppState {
    pub fn db_compact(&self) -> Result<(), rusqlite::Error> {
        self.admin_db.lock().unwrap().execute("VACUUM;", [])?;
        self.content_db.lock().unwrap().execute("VACUUM;", [])?;
        self.analytics_db.lock().unwrap().execute("VACUUM;", [])?;
        self.system_db.lock().unwrap().execute("VACUUM;", [])?;
        Ok(())
    }
}

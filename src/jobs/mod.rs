use chrono::Utc;
use rusqlite::{params, Connection};
use std::sync::Mutex;
use uuid::Uuid;

pub mod aggregate;
pub mod backup;
pub mod expiry;
pub mod healthcheck;
pub mod retention;

pub use aggregate::{perform_aggregation, run_aggregator};
pub use expiry::run_expiry_checker;
pub use healthcheck::{perform_link_check, run_link_checker};
pub use retention::run_retention_cleaner;

pub fn log_job_start(conn: &Mutex<Connection>, job_name: &str) -> String {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    if let Ok(c) = conn.lock() {
        let _ = c.execute(
            "INSERT INTO job_history (id, job_name, status, started_at) VALUES (?1, ?2, ?3, ?4);",
            params![id, job_name, "running", now],
        );
    }
    id
}

pub fn log_job_end(conn: &Mutex<Connection>, id: &str, status: &str, err_msg: Option<&str>) {
    let now = Utc::now().to_rfc3339();
    if let Ok(c) = conn.lock() {
        let _ = c.execute(
            "UPDATE job_history SET status = ?1, finished_at = ?2, error_message = ?3 WHERE id = ?4;",
            params![status, now, err_msg, id],
        );
    }
}

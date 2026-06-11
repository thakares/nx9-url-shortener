use std::sync::Mutex;
use rusqlite::{Connection, params};
use uuid::Uuid;
use chrono::Utc;

pub mod healthcheck;
pub mod retention;
pub mod aggregate;
pub mod backup;

pub use healthcheck::{run_link_checker, perform_link_check};
pub use retention::run_retention_cleaner;
pub use aggregate::{run_aggregator, perform_aggregation};

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

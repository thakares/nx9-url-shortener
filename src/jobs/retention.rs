use std::time::Duration;
use tracing::{error, info};

use super::{log_job_end, log_job_start};
use crate::db::Db;

pub async fn run_retention_cleaner(db: Db, retention_days_opt: Option<i64>) {
    let retention_days = match retention_days_opt {
        Some(days) => days,
        None => return,
    };

    loop {
        // Check once every 24 hours
        tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
        info!("Running background data retention cleanup...");

        let user_ids: Vec<i64> = {
            let conn = db.users.lock().unwrap();
            let mut stmt = match conn.prepare("SELECT id FROM users;") {
                Ok(s) => s,
                Err(_) => continue,
            };
            let rows = match stmt.query_map([], |row| row.get(0)) {
                Ok(r) => r,
                Err(_) => continue,
            };
            rows.filter_map(|r| r.ok()).collect()
        };

        let job_id = log_job_start(&db.system, "retention_cleaner");
        let mut total_cleaned = 0;
        let mut failed = false;
        let mut err_msg = None;

        for user_id in user_ids {
            match super::open_user_analytics_conn(&db, user_id) {
                Ok(conn) => match crate::db::analytics::retention_cleanup(&conn, retention_days) {
                    Ok(count) => total_cleaned += count,
                    Err(e) => {
                        failed = true;
                        err_msg = Some(e.to_string());
                    }
                },
                Err(e) => {
                    failed = true;
                    err_msg = Some(e.to_string());
                }
            }
        }

        if failed {
            let err_str = err_msg.unwrap_or_else(|| "Unknown error".to_string());
            error!("Error running retention cleaner: {:?}", err_str);
            log_job_end(&db.system, &job_id, "failed", Some(&err_str));
        } else {
            info!(
                "Cleaned up {} expired visits across all user databases",
                total_cleaned
            );
            log_job_end(&db.system, &job_id, "success", None);
        }
    }
}

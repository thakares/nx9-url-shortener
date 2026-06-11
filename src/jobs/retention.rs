use std::time::Duration;
use tracing::{info, error};

use crate::db::Db;
use super::{log_job_start, log_job_end};

pub async fn run_retention_cleaner(db: Db, retention_days_opt: Option<i64>) {
    let retention_days = match retention_days_opt {
        Some(days) => days,
        None => return,
    };

    loop {
        // Check once every 24 hours
        tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
        info!("Running background data retention cleanup...");
        
        let job_id = log_job_start(&db.system, "retention_cleaner");
        let conn = db.analytics.lock().unwrap();
        match crate::db::analytics::retention_cleanup(&conn, retention_days) {
            Ok(count) => {
                info!("Cleaned up {} expired visits from database", count);
                log_job_end(&db.system, &job_id, "success", None);
            }
            Err(e) => {
                let err_str = e.to_string();
                error!("Error running retention cleaner: {:?}", err_str);
                log_job_end(&db.system, &job_id, "failed", Some(&err_str));
            }
        }
    }
}

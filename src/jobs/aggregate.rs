use std::time::Duration;
use tracing::{error, info};

use super::{log_job_end, log_job_start};
use crate::analytics::aggregate_day;
use crate::db::Db;

pub async fn run_aggregator(db: Db, interval_mins: u64) {
    loop {
        tokio::time::sleep(Duration::from_secs(interval_mins * 60)).await;
        info!("Running background analytics aggregator...");

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

        let job_id = log_job_start(&db.system, "analytics_aggregator");
        let mut failed = false;
        let mut err_msg = None;

        for user_id in user_ids {
            if let Err(e) = perform_aggregation(&db, user_id).await {
                failed = true;
                err_msg = Some(e.to_string());
            }
        }

        if failed {
            let err_str = err_msg.unwrap_or_else(|| "Unknown error".to_string());
            error!("Error performing aggregation: {}", err_str);
            log_job_end(&db.system, &job_id, "failed", Some(&err_str));
        } else {
            log_job_end(&db.system, &job_id, "success", None);
        }
    }
}

pub async fn perform_aggregation(db: &Db, user_id: i64) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = super::open_user_analytics_conn(db, user_id)?;
    let date_range = crate::db::analytics::get_visits_date_range(&conn)?;

    if let Some((min_date, max_date)) = date_range {
        let min = chrono::NaiveDate::parse_from_str(&min_date, "%Y-%m-%d")?;
        let max = chrono::NaiveDate::parse_from_str(&max_date, "%Y-%m-%d")?;

        let mut curr = min;
        while curr <= max {
            let date_str = curr.format("%Y-%m-%d").to_string();
            aggregate_day(&mut conn, &date_str)?;
            if curr == max {
                break;
            }
            if let Some(next) = curr.succ_opt() {
                curr = next;
            } else {
                break;
            }
        }
    }
    Ok(())
}

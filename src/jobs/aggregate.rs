use std::time::Duration;
use tracing::{info, error};

use crate::db::Db;
use crate::analytics::aggregate_day;
use super::{log_job_start, log_job_end};

pub async fn run_aggregator(db: Db, interval_mins: u64) {
    loop {
        tokio::time::sleep(Duration::from_secs(interval_mins * 60)).await;
        info!("Running background analytics aggregator...");
        
        let job_id = log_job_start(&db.system, "analytics_aggregator");
        match perform_aggregation(&db).await {
            Ok(_) => log_job_end(&db.system, &job_id, "success", None),
            Err(e) => {
                let err_str = e.to_string();
                error!("Error performing aggregation: {}", err_str);
                log_job_end(&db.system, &job_id, "failed", Some(&err_str));
            }
        }
    }
}

pub async fn perform_aggregation(db: &Db) -> Result<(), Box<dyn std::error::Error>> {
    let date_range = {
        let conn = db.analytics.lock().unwrap();
        crate::db::analytics::get_visits_date_range(&conn)?
    };

    if let Some((min_date, max_date)) = date_range {
        let min = chrono::NaiveDate::parse_from_str(&min_date, "%Y-%m-%d")?;
        let max = chrono::NaiveDate::parse_from_str(&max_date, "%Y-%m-%d")?;

        let mut curr = min;
        while curr <= max {
            let date_str = curr.format("%Y-%m-%d").to_string();
            {
                let mut conn = db.analytics.lock().unwrap();
                aggregate_day(&mut conn, &date_str)?;
            }
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

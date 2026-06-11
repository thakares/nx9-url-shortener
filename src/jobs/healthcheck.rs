use reqwest::Client;
use std::time::Duration;
use tracing::{info, error};
use uuid::Uuid;
use chrono::Utc;
use rusqlite::params;

use crate::db::Db;
use super::{log_job_start, log_job_end};

pub async fn run_link_checker(db: Db, interval_mins: u64) {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("bzod-link-checker/0.1")
        .build()
        .unwrap_or_default();

    loop {
        // Sleep first to give server time to start up
        tokio::time::sleep(Duration::from_secs(interval_mins * 60)).await;
        info!("Running background link health check...");
        
        let job_id = log_job_start(&db.system, "link_checker");
        match perform_link_check(&db, &client).await {
            Ok(_) => log_job_end(&db.system, &job_id, "success", None),
            Err(e) => {
                let err_str = e.to_string();
                error!("Error performing link health check: {}", err_str);
                log_job_end(&db.system, &job_id, "failed", Some(&err_str));
            }
        }
    }
}

pub async fn perform_link_check(db: &Db, client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    let urls = {
        let conn = db.content.lock().unwrap();
        crate::db::content::list_urls_for_health_check(&conn)?
    };

    for (id, dest) in urls {
        let (status, status_code, err_msg) = check_url_health(client, &dest).await;
        {
            let conn = db.content.lock().unwrap();
            crate::db::content::update_url_health(&conn, &id, &status)?;
        }
        
        // Log to system.db.health_checks
        {
            let conn = db.system.lock().unwrap();
            let hc_id = Uuid::new_v4().to_string();
            let now = Utc::now().to_rfc3339();
            let is_healthy = if status == "healthy" { 1 } else { 0 };
            let _ = conn.execute(
                "INSERT INTO health_checks (id, object_type, object_id, checked_at, status_code, error_message, is_healthy) 
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                params![hc_id, "url", id, now, status_code, err_msg, is_healthy],
            );
        }

        // Rate limiting sleep between external requests
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Ok(())
}

async fn check_url_health(client: &Client, url: &str) -> (String, Option<u16>, Option<String>) {
    let res = client.get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await;

    match res {
        Ok(response) => {
            let status = response.status();
            let code = status.as_u16();
            if status.is_success() || status.is_redirection() {
                ("healthy".to_string(), Some(code), None)
            } else if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
                ("dead".to_string(), Some(code), Some(format!("HTTP {}", code)))
            } else {
                ("suspect".to_string(), Some(code), Some(format!("HTTP {}", code)))
            }
        }
        Err(err) => {
            let err_str = err.to_string();
            if err.is_timeout() || err.is_connect() {
                ("suspect".to_string(), None, Some(err_str))
            } else {
                ("dead".to_string(), None, Some(err_str))
            }
        }
    }
}

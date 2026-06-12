use chrono::Utc;
use reqwest::Client;
use rusqlite::params;
use std::time::{Duration, Instant};
use tracing::{error, info};
use uuid::Uuid;

use super::{log_job_end, log_job_start};
use crate::db::Db;

pub async fn run_link_checker(db: Db, interval_mins: u64) {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("bzod-link-checker/0.1")
        .redirect(reqwest::redirect::Policy::limited(10))
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

pub async fn perform_link_check(
    db: &Db,
    client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let urls = {
        let conn = db.content.lock().unwrap();
        crate::db::content::list_urls_for_health_check(&conn)?
    };

    for (id, dest) in urls {
        let (status, detail_status, status_code, latency_ms, err_msg) =
            check_url_health(client, &dest).await;
        {
            let conn = db.content.lock().unwrap();
            crate::db::content::update_url_health_extended(
                &conn,
                &id,
                &status,
                &detail_status,
                Some(latency_ms),
            )?;
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

/// Check URL health with detailed classification and latency measurement.
///
/// Returns: (general_status, detail_status, status_code, latency_ms, error_message)
async fn check_url_health(
    client: &Client,
    url: &str,
) -> (String, String, Option<u16>, i64, Option<String>) {
    let start = Instant::now();
    let res = client.get(url).timeout(Duration::from_secs(5)).send().await;
    let latency_ms = start.elapsed().as_millis() as i64;

    match res {
        Ok(response) => {
            let status = response.status();
            let code = status.as_u16();
            if status.is_success() || status.is_redirection() {
                (
                    "healthy".to_string(),
                    "healthy".to_string(),
                    Some(code),
                    latency_ms,
                    None,
                )
            } else if status == reqwest::StatusCode::NOT_FOUND
                || status == reqwest::StatusCode::GONE
            {
                (
                    "dead".to_string(),
                    "dead".to_string(),
                    Some(code),
                    latency_ms,
                    Some(format!("HTTP {}", code)),
                )
            } else {
                (
                    "suspect".to_string(),
                    format!("http_{}", code),
                    Some(code),
                    latency_ms,
                    Some(format!("HTTP {}", code)),
                )
            }
        }
        Err(err) => {
            let err_str = err.to_string();
            let detail = if err.is_timeout() {
                "timeout".to_string()
            } else if err.is_connect() {
                if err_str.contains("dns") || err_str.contains("resolve") {
                    "dns_failure".to_string()
                } else if err_str.contains("tls")
                    || err_str.contains("ssl")
                    || err_str.contains("certificate")
                {
                    "tls_error".to_string()
                } else {
                    "connection_refused".to_string()
                }
            } else if err.is_redirect() {
                "redirect_loop".to_string()
            } else {
                "unknown_error".to_string()
            };

            let general = if err.is_timeout() || err.is_connect() {
                "suspect"
            } else {
                "dead"
            };

            (general.to_string(), detail, None, latency_ms, Some(err_str))
        }
    }
}

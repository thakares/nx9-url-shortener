use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use axum_extra::extract::CookieJar;
use serde::Serialize;

use crate::auth::{authenticate_api_key, authenticate_session};
use crate::db::admin::get_user_count;
use crate::state::AppState;
use crate::utils::{get_db_file_info, get_memory_usage};

// Helper: authenticate system request via header or session cookie
fn authenticate_request(state: &AppState, jar: &CookieJar, headers: &HeaderMap) -> bool {
    // 1. Try Authorization header
    if let Some(auth_header) = headers.get("Authorization").and_then(|h| h.to_str().ok()) {
        let conn = state.admin_db.lock().unwrap();
        if let Ok(Some(_)) = authenticate_api_key(&conn, auth_header) {
            return true;
        }
    }

    // 2. Try cookie session
    let conn = state.admin_db.lock().unwrap();
    if let Ok(Some(_)) = authenticate_session(&conn, jar) {
        return true;
    }

    false
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub application: &'static str,
    pub database: String,
    pub queue_size: usize,
    pub memory_usage: String,
    pub uptime_seconds: u64,
    pub version: &'static str,
    pub git_commit: &'static str,
}

// GET /status
pub async fn status_endpoint(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Response {
    if !authenticate_request(&state, &jar, &headers) {
        // Return public basic status for container/load-balancer health checks
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "application": "Healthy" })),
        )
            .into_response();
    }

    let is_db_ok = {
        let conn = state.admin_db.lock().unwrap();
        get_user_count(&conn).is_ok()
    };

    let db_status = if is_db_ok {
        format!(
            "Connected (WAL Mode enabled). Files Info:\n{}",
            get_db_file_info(&state.config.data_dir)
        )
    } else {
        "Disconnected".to_string()
    };

    let uptime = state.start_time.elapsed().as_secs();

    Json(StatusResponse {
        application: "Healthy",
        database: db_status,
        queue_size: 0,
        memory_usage: get_memory_usage(),
        uptime_seconds: uptime,
        version: "0.1.0",
        git_commit: "unknown",
    })
    .into_response()
}

// GET /metrics
pub async fn metrics_endpoint(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Response {
    if !authenticate_request(&state, &jar, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // 1. Gather stats from DBs
    let (total_urls, active_links, dead_links) = {
        let conn = state.content_db.lock().unwrap();
        crate::db::content::get_url_counts(&conn).unwrap_or((0, 0, 0))
    };

    let total_pages = {
        let conn = state.content_db.lock().unwrap();
        crate::db::content::get_landing_page_count(&conn).unwrap_or(0)
    };

    let (total_clicks, total_page_views) = {
        let conn = state.analytics_db.lock().unwrap();
        (
            crate::db::analytics::get_total_clicks(&conn).unwrap_or(0),
            crate::db::analytics::get_total_page_views(&conn).unwrap_or(0),
        )
    };

    // Calculate memory in bytes
    let mut mem_bytes = 0;
    if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
        if let Some(pages_str) = statm.split_whitespace().next() {
            if let Ok(pages) = pages_str.parse::<u64>() {
                mem_bytes = pages * 4096;
            }
        }
    }

    let uptime = state.start_time.elapsed().as_secs();

    // 2. Format as Prometheus metrics text
    let metrics_text = format!(
        r#"# HELP bzod_urls_total Total number of registered short URLs
# TYPE bzod_urls_total gauge
bzod_urls_total {total_urls}

# HELP bzod_active_urls Total number of active/healthy short URLs
# TYPE bzod_active_urls gauge
bzod_active_urls {active_links}

# HELP bzod_dead_urls Total number of dead short URLs
# TYPE bzod_dead_urls gauge
bzod_dead_urls {dead_links}

# HELP bzod_pages_total Total number of registered landing pages
# TYPE bzod_pages_total gauge
bzod_pages_total {total_pages}

# HELP bzod_clicks_total Total number of URL clicks recorded
# TYPE bzod_clicks_total counter
bzod_clicks_total {total_clicks}

# HELP bzod_page_views_total Total number of page views recorded
# TYPE bzod_page_views_total counter
bzod_page_views_total {total_page_views}

# HELP bzod_memory_bytes Memory usage of the bzod process in bytes
# TYPE bzod_memory_bytes gauge
bzod_memory_bytes {mem_bytes}

# HELP bzod_uptime_seconds Uptime of the bzod process in seconds
# TYPE bzod_uptime_seconds counter
bzod_uptime_seconds {uptime}
"#,
        total_urls = total_urls,
        active_links = active_links,
        dead_links = dead_links,
        total_pages = total_pages,
        total_clicks = total_clicks,
        total_page_views = total_page_views,
        mem_bytes = mem_bytes,
        uptime = uptime
    );

    (
        StatusCode::OK,
        [("Content-Type", "text/plain; version=0.0.4; charset=utf-8")],
        metrics_text,
    )
        .into_response()
}

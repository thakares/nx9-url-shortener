//! Admin web UI handlers (feature-split modules).
//!
//! Handlers are organized by domain; shared auth, audit, export, and helpers
//! live in this module root so child modules can access them via `super`.

use crate::auth::{
    authenticate_admin_session, authenticate_user_session, generate_csrf_token, generate_token,
    hash_password, verify_csrf, verify_password, verify_sha256,
};
use crate::charts::{generate_bar_chart, generate_line_chart};
use crate::db::admin::{
    create_api_key, delete_api_key, get_config, get_user_count, list_api_keys, set_config,
    write_audit_log as write_audit_log_legacy,
};
use crate::db::analytics::{
    clean_referrer, get_clicks_trend, get_clicks_trend_raw, get_metric_rankings,
    get_metric_rankings_raw, get_monthly_clicks_trend, get_target_unique_visitors,
    get_target_visit_total_filtered, get_target_visits_all_in_memory, get_target_visits_paginated,
    get_total_clicks, get_visits_schema_columns, parse_ua,
};
use crate::db::content::{
    create_landing_page, delete_landing_page, delete_url, get_landing_page_by_id,
    get_landing_page_count, get_url_by_id, get_url_count_by_tag, get_url_counts,
    list_landing_pages, list_urls,
};
use crate::models::User;
use crate::state::AppState;
use crate::utils::{get_client_ip, get_db_file_info, get_memory_usage};
use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use chrono::Utc;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::net::SocketAddr;
use tar::Builder;
use uuid::Uuid;

// --- Shared constants, auth, audit, and export helpers ---

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_audit_log(
    conn: &rusqlite::Connection,
    state: &AppState,
    username: &str,
    action: &str,
    object_type: Option<&str>,
    object_id: Option<&str>,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
) -> rusqlite::Result<crate::models::AuditLog> {
    let res = write_audit_log_legacy(
        conn,
        username,
        action,
        object_type,
        object_id,
        ip_address,
        user_agent,
    );

    // Also write to unified audit events in system.db
    let system_conn = state.system_db.lock().unwrap();
    let metadata = format!("IP: {:?}, UA: {:?}", ip_address, user_agent);
    let _ = crate::db::audit_events::write_audit_event(
        &system_conn,
        username,
        action,
        object_type.unwrap_or(""),
        object_id.unwrap_or(""),
        Some(&metadata),
    );

    res
}
pub(crate) const PAGE_SIZE: usize = 25;
pub(crate) const ANALYTICS_PAGE_SIZE: usize = 50;
pub(crate) const MAX_JSON_EXPORT_ROWS: usize = 50_000;
// Helper: Verify admin session and return user or redirect to login
pub(crate) async fn require_auth(
    state: &AppState,
    jar: &CookieJar,
) -> Result<(User, String), Redirect> {
    let conn = match state.users_db.lock() {
        Ok(c) => c,
        Err(_) => return Err(Redirect::to("/admin/login")),
    };
    match authenticate_admin_session(&conn, jar) {
        Ok(Some((user, session_id))) => Ok((user, session_id)),
        _ => Err(Redirect::to("/admin/login")),
    }
}
// Helper: Verify tenant user session and return user or redirect to login
pub(crate) async fn require_user_auth(
    state: &AppState,
    jar: &CookieJar,
) -> Result<(crate::models::TenantUser, String), Redirect> {
    let conn = match state.users_db.lock() {
        Ok(c) => c,
        Err(_) => return Err(Redirect::to("/login")),
    };
    match authenticate_user_session(&conn, jar) {
        Ok(Some((user, session_id))) => Ok((user, session_id)),
        _ => Err(Redirect::to("/login")),
    }
}
#[derive(Deserialize)]
pub struct AnalyticsQuery {
    pub analytics_page: Option<usize>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}
pub(crate) fn validate_date_filters(
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> Result<(Option<String>, Option<String>), StatusCode> {
    let from_parsed = match date_from {
        Some(df) if !df.is_empty() => match chrono::NaiveDate::parse_from_str(df, "%Y-%m-%d") {
            Ok(d) => Some(d),
            Err(_) => return Err(StatusCode::BAD_REQUEST),
        },
        _ => None,
    };
    let to_parsed = match date_to {
        Some(dt) if !dt.is_empty() => match chrono::NaiveDate::parse_from_str(dt, "%Y-%m-%d") {
            Ok(d) => Some(d),
            Err(_) => return Err(StatusCode::BAD_REQUEST),
        },
        _ => None,
    };
    if let (Some(f), Some(t)) = (from_parsed, to_parsed) {
        if f > t {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    Ok((
        from_parsed.map(|d| d.format("%Y-%m-%d").to_string()),
        to_parsed.map(|d| d.format("%Y-%m-%d").to_string()),
    ))
}
pub(crate) fn escape_csv_field(field: &str) -> String {
    let needs_escaping =
        field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r');
    if needs_escaping {
        let escaped = field.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        field.to_string()
    }
}
pub(crate) struct DbExportStream {
    receiver: tokio::sync::mpsc::Receiver<Result<axum::body::Bytes, std::convert::Infallible>>,
}

impl futures_util::stream::Stream for DbExportStream {
    type Item = Result<axum::body::Bytes, std::convert::Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

pub(crate) async fn perform_csv_export(
    state: AppState,
    target_type: &'static str,
    id: String,
    date_from: Option<String>,
    date_to: Option<String>,
) -> Response {
    let (clean_date_from, clean_date_to) =
        match validate_date_filters(date_from.as_deref(), date_to.as_deref()) {
            Ok(res) => res,
            Err(status) => return status.into_response(),
        };

    let target_exists = {
        let conn = state.content_db.lock().unwrap();
        if target_type == "url" {
            get_url_by_id(&conn, &id)
                .map(|u| u.is_some())
                .unwrap_or(false)
        } else {
            get_landing_page_by_id(&conn, &id)
                .map(|p| p.is_some())
                .unwrap_or(false)
        }
    };
    if !target_exists {
        return (StatusCode::NOT_FOUND, "Target not found").into_response();
    }

    let count = {
        let conn = state.analytics_db.lock().unwrap();
        get_target_visit_total_filtered(
            &conn,
            target_type,
            &id,
            clean_date_from.as_deref(),
            clean_date_to.as_deref(),
        )
        .unwrap_or(0)
    };

    let (has_utm_source, has_utm_campaign) = {
        let conn = state.analytics_db.lock().unwrap();
        let cols = get_visits_schema_columns(&conn).unwrap_or_default();
        (cols.contains("utm_source"), cols.contains("utm_campaign"))
    };

    let (tx, rx) =
        tokio::sync::mpsc::channel::<Result<axum::body::Bytes, std::convert::Infallible>>(32);
    let analytics_db = state.analytics_db.clone();
    let target_id = id.clone();

    tokio::task::spawn_blocking(move || {
        let conn = analytics_db.lock().unwrap();

        let mut header = "Timestamp,IP Address,Country,Referrer,Browser,User-Agent".to_string();
        if has_utm_source {
            header.push_str(",UTM Source");
        }
        if has_utm_campaign {
            header.push_str(",UTM Campaign");
        }
        header.push('\n');

        if tx
            .blocking_send(Ok(axum::body::Bytes::from(header)))
            .is_err()
        {
            return;
        }

        let select_fields = if has_utm_source && has_utm_campaign {
            "timestamp, ip_address, country, referer, user_agent, utm_source, utm_campaign"
        } else if has_utm_source {
            "timestamp, ip_address, country, referer, user_agent, utm_source"
        } else if has_utm_campaign {
            "timestamp, ip_address, country, referer, user_agent, utm_campaign"
        } else {
            "timestamp, ip_address, country, referer, user_agent"
        };

        let mut sql = format!(
            "SELECT {} FROM visits WHERE target_type = ?1 AND target_id = ?2",
            select_fields
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(target_type.to_string()), Box::new(target_id)];

        if let Some(df) = clean_date_from.as_deref() {
            sql.push_str(&format!(" AND timestamp >= ?{}", params.len() + 1));
            params.push(Box::new(format!("{}T00:00:00Z", df)));
        }

        if let Some(dt) = clean_date_to.as_deref() {
            if let Ok(parsed_date) = chrono::NaiveDate::parse_from_str(dt, "%Y-%m-%d") {
                let next_day = parsed_date + chrono::Duration::days(1);
                sql.push_str(&format!(" AND timestamp < ?{}", params.len() + 1));
                params.push(Box::new(format!(
                    "{}T00:00:00Z",
                    next_day.format("%Y-%m-%d")
                )));
            }
        }

        sql.push_str(" ORDER BY timestamp DESC, id DESC");

        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return,
        };

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut rows = match stmt.query(rusqlite::params_from_iter(param_refs)) {
            Ok(r) => r,
            Err(_) => return,
        };

        let mut csv_buffer = String::new();

        while let Ok(Some(row)) = rows.next() {
            let timestamp: String = row.get(0).unwrap_or_default();
            let ip_address: String = row.get(1).unwrap_or_default();
            let country: String = row.get(2).unwrap_or_default();
            let referer: String = row.get(3).unwrap_or_default();
            let user_agent: String = row.get(4).unwrap_or_default();

            let (browser, _, _) = parse_ua(&user_agent);
            let referrer = clean_referrer(&referer);
            let country_display = if country.is_empty() {
                "Unknown".to_string()
            } else {
                country
            };

            let mut line = format!(
                "{},{},{},{},{},{}",
                escape_csv_field(&timestamp),
                escape_csv_field(&ip_address),
                escape_csv_field(&country_display),
                escape_csv_field(&referrer),
                escape_csv_field(&browser),
                escape_csv_field(&user_agent)
            );

            let mut col_idx = 5;
            if has_utm_source {
                let utm_src: String = row.get(col_idx).unwrap_or_default();
                line.push_str(&format!(",{}", escape_csv_field(&utm_src)));
                col_idx += 1;
            }
            if has_utm_campaign {
                let utm_camp: String = row.get(col_idx).unwrap_or_default();
                line.push_str(&format!(",{}", escape_csv_field(&utm_camp)));
            }
            line.push('\n');

            csv_buffer.push_str(&line);
            if csv_buffer.len() >= 8192 {
                let bytes = axum::body::Bytes::from(csv_buffer);
                if tx.blocking_send(Ok(bytes)).is_err() {
                    return;
                }
                csv_buffer = String::new();
            }
        }

        if !csv_buffer.is_empty() {
            let _ = tx.blocking_send(Ok(axum::body::Bytes::from(csv_buffer)));
        }
    });

    let stream = DbExportStream { receiver: rx };
    let filename = if target_type == "url" {
        format!("url_{}_analytics.csv", id)
    } else {
        format!("page_{}_analytics.csv", id)
    };

    (
        StatusCode::OK,
        [
            ("Content-Type", "text/csv"),
            (
                "Content-Disposition",
                &format!("attachment; filename=\"{}\"", filename),
            ),
            ("X-BZOD-Export-Records", &count.to_string()),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response()
}
pub(crate) async fn perform_json_export(
    state: AppState,
    target_type: &'static str,
    id: String,
    date_from: Option<String>,
    date_to: Option<String>,
) -> Response {
    let (clean_date_from, clean_date_to) =
        match validate_date_filters(date_from.as_deref(), date_to.as_deref()) {
            Ok(res) => res,
            Err(status) => return status.into_response(),
        };

    let target_exists = {
        let conn = state.content_db.lock().unwrap();
        if target_type == "url" {
            get_url_by_id(&conn, &id)
                .map(|u| u.is_some())
                .unwrap_or(false)
        } else {
            get_landing_page_by_id(&conn, &id)
                .map(|p| p.is_some())
                .unwrap_or(false)
        }
    };
    if !target_exists {
        return (StatusCode::NOT_FOUND, "Target not found").into_response();
    }

    let count = {
        let conn = state.analytics_db.lock().unwrap();
        get_target_visit_total_filtered(
            &conn,
            target_type,
            &id,
            clean_date_from.as_deref(),
            clean_date_to.as_deref(),
        )
        .unwrap_or(0)
    };

    if count > MAX_JSON_EXPORT_ROWS as i64 {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    let visits_raw = {
        let conn = state.analytics_db.lock().unwrap();
        match get_target_visits_all_in_memory(
            &conn,
            target_type,
            &id,
            clean_date_from.as_deref(),
            clean_date_to.as_deref(),
        ) {
            Ok(v) => v,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        }
    };

    #[derive(serde::Serialize)]
    struct JsonExportRow {
        timestamp: String,
        ip_address: String,
        country: String,
        referrer: String,
        browser: String,
        user_agent: String,
    }

    let export_rows: Vec<JsonExportRow> = visits_raw
        .into_iter()
        .map(|r| {
            let (browser, _, _) = parse_ua(&r.user_agent);
            let referrer = clean_referrer(&r.referer);
            let country_display = if r.country.is_empty() {
                "Unknown".to_string()
            } else {
                r.country
            };
            JsonExportRow {
                timestamp: r.timestamp,
                ip_address: r.ip_address,
                country: country_display,
                referrer,
                browser,
                user_agent: r.user_agent,
            }
        })
        .collect();

    let body_str = match serde_json::to_string(&export_rows) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Serialization error").into_response()
        }
    };

    let filename = if target_type == "url" {
        format!("url_{}_analytics.json", id)
    } else {
        format!("page_{}_analytics.json", id)
    };

    (
        StatusCode::OK,
        [
            ("Content-Type", "application/json"),
            (
                "Content-Disposition",
                &format!("attachment; filename=\"{}\"", filename),
            ),
            ("X-BZOD-Export-Records", &count.to_string()),
        ],
        body_str,
    )
        .into_response()
}
// Helpers
pub(crate) fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
pub(crate) fn get_dir_size(dir: &std::path::Path) -> std::io::Result<u64> {
    let mut total = 0;
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                total += get_dir_size(&path)?;
            } else {
                total += entry.metadata()?.len();
            }
        }
    }
    Ok(total)
}
pub(crate) async fn perform_user_csv_export(
    user_dbs: crate::state::UserDbs,
    target_type: &'static str,
    id: String,
    date_from: Option<String>,
    date_to: Option<String>,
) -> Response {
    let (clean_date_from, clean_date_to) =
        match validate_date_filters(date_from.as_deref(), date_to.as_deref()) {
            Ok(res) => res,
            Err(status) => return status.into_response(),
        };

    let target_exists = {
        let conn = user_dbs.content.lock().unwrap();
        if target_type == "url" {
            get_url_by_id(&conn, &id)
                .map(|u| u.is_some())
                .unwrap_or(false)
        } else {
            get_landing_page_by_id(&conn, &id)
                .map(|p| p.is_some())
                .unwrap_or(false)
        }
    };
    if !target_exists {
        return (StatusCode::NOT_FOUND, "Target not found").into_response();
    }

    let count = {
        let conn = user_dbs.analytics.lock().unwrap();
        get_target_visit_total_filtered(
            &conn,
            target_type,
            &id,
            clean_date_from.as_deref(),
            clean_date_to.as_deref(),
        )
        .unwrap_or(0)
    };

    let (has_utm_source, has_utm_campaign) = {
        let conn = user_dbs.analytics.lock().unwrap();
        let cols = get_visits_schema_columns(&conn).unwrap_or_default();
        (cols.contains("utm_source"), cols.contains("utm_campaign"))
    };

    let (tx, rx) =
        tokio::sync::mpsc::channel::<Result<axum::body::Bytes, std::convert::Infallible>>(32);
    let analytics_db = user_dbs.analytics.clone();
    let target_id = id.clone();

    tokio::task::spawn_blocking(move || {
        let conn = analytics_db.lock().unwrap();

        let mut header = "Timestamp,IP Address,Country,Referrer,Browser,User-Agent".to_string();
        if has_utm_source {
            header.push_str(",UTM Source");
        }
        if has_utm_campaign {
            header.push_str(",UTM Campaign");
        }
        header.push('\n');

        if tx
            .blocking_send(Ok(axum::body::Bytes::from(header)))
            .is_err()
        {
            return;
        }

        let select_fields = if has_utm_source && has_utm_campaign {
            "timestamp, ip_address, country, referer, user_agent, utm_source, utm_campaign"
        } else if has_utm_source {
            "timestamp, ip_address, country, referer, user_agent, utm_source"
        } else if has_utm_campaign {
            "timestamp, ip_address, country, referer, user_agent, utm_campaign"
        } else {
            "timestamp, ip_address, country, referer, user_agent"
        };

        let mut sql = format!(
            "SELECT {} FROM visits WHERE target_type = ?1 AND target_id = ?2",
            select_fields
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(target_type.to_string()), Box::new(target_id)];

        if let Some(df) = clean_date_from.as_deref() {
            sql.push_str(&format!(" AND timestamp >= ?{}", params.len() + 1));
            params.push(Box::new(format!("{}T00:00:00Z", df)));
        }

        if let Some(dt) = clean_date_to.as_deref() {
            if let Ok(parsed_date) = chrono::NaiveDate::parse_from_str(dt, "%Y-%m-%d") {
                let next_day = parsed_date + chrono::Duration::days(1);
                sql.push_str(&format!(" AND timestamp < ?{}", params.len() + 1));
                params.push(Box::new(format!(
                    "{}T00:00:00Z",
                    next_day.format("%Y-%m-%d")
                )));
            }
        }

        sql.push_str(" ORDER BY timestamp DESC, id DESC");

        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return,
        };

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut rows = match stmt.query(rusqlite::params_from_iter(param_refs)) {
            Ok(r) => r,
            Err(_) => return,
        };

        let mut csv_buffer = String::new();

        while let Ok(Some(row)) = rows.next() {
            let timestamp: String = row.get(0).unwrap_or_default();
            let ip_address: String = row.get(1).unwrap_or_default();
            let country: String = row.get(2).unwrap_or_default();
            let referer: String = row.get(3).unwrap_or_default();
            let user_agent: String = row.get(4).unwrap_or_default();

            let (browser, _, _) = parse_ua(&user_agent);
            let referrer = clean_referrer(&referer);
            let country_display = if country.is_empty() {
                "Unknown".to_string()
            } else {
                country
            };

            let mut line = format!(
                "{},{},{},{},{},{}",
                escape_csv_field(&timestamp),
                escape_csv_field(&ip_address),
                escape_csv_field(&country_display),
                escape_csv_field(&referrer),
                escape_csv_field(&browser),
                escape_csv_field(&user_agent)
            );

            let mut col_idx = 5;
            if has_utm_source {
                let utm_src: String = row.get(col_idx).unwrap_or_default();
                line.push_str(&format!(",{}", escape_csv_field(&utm_src)));
                col_idx += 1;
            }
            if has_utm_campaign {
                let utm_camp: String = row.get(col_idx).unwrap_or_default();
                line.push_str(&format!(",{}", escape_csv_field(&utm_camp)));
            }
            line.push('\n');

            csv_buffer.push_str(&line);
            if csv_buffer.len() >= 8192 {
                let bytes = axum::body::Bytes::from(csv_buffer);
                if tx.blocking_send(Ok(bytes)).is_err() {
                    return;
                }
                csv_buffer = String::new();
            }
        }

        if !csv_buffer.is_empty() {
            let _ = tx.blocking_send(Ok(axum::body::Bytes::from(csv_buffer)));
        }
    });

    let stream = DbExportStream { receiver: rx };
    let filename = if target_type == "url" {
        format!("url_{}_analytics.csv", id)
    } else {
        format!("page_{}_analytics.csv", id)
    };

    (
        StatusCode::OK,
        [
            ("Content-Type", "text/csv"),
            (
                "Content-Disposition",
                &format!("attachment; filename=\"{}\"", filename),
            ),
            ("X-BZOD-Export-Records", &count.to_string()),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response()
}
pub(crate) async fn perform_user_json_export(
    user_dbs: crate::state::UserDbs,
    target_type: &'static str,
    id: String,
    date_from: Option<String>,
    date_to: Option<String>,
) -> Response {
    let (clean_date_from, clean_date_to) =
        match validate_date_filters(date_from.as_deref(), date_to.as_deref()) {
            Ok(res) => res,
            Err(status) => return status.into_response(),
        };

    let target_exists = {
        let conn = user_dbs.content.lock().unwrap();
        if target_type == "url" {
            get_url_by_id(&conn, &id)
                .map(|u| u.is_some())
                .unwrap_or(false)
        } else {
            get_landing_page_by_id(&conn, &id)
                .map(|p| p.is_some())
                .unwrap_or(false)
        }
    };
    if !target_exists {
        return (StatusCode::NOT_FOUND, "Target not found").into_response();
    }

    let count = {
        let conn = user_dbs.analytics.lock().unwrap();
        get_target_visit_total_filtered(
            &conn,
            target_type,
            &id,
            clean_date_from.as_deref(),
            clean_date_to.as_deref(),
        )
        .unwrap_or(0)
    };

    if count > MAX_JSON_EXPORT_ROWS as i64 {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    let visits_raw = {
        let conn = user_dbs.analytics.lock().unwrap();
        match get_target_visits_all_in_memory(
            &conn,
            target_type,
            &id,
            clean_date_from.as_deref(),
            clean_date_to.as_deref(),
        ) {
            Ok(v) => v,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        }
    };

    #[derive(serde::Serialize)]
    struct JsonExportRow {
        timestamp: String,
        ip_address: String,
        country: String,
        referrer: String,
        browser: String,
        user_agent: String,
    }

    let export_rows: Vec<JsonExportRow> = visits_raw
        .into_iter()
        .map(|r| {
            let (browser, _, _) = parse_ua(&r.user_agent);
            let referrer = clean_referrer(&r.referer);
            let country_display = if r.country.is_empty() {
                "Unknown".to_string()
            } else {
                r.country
            };
            JsonExportRow {
                timestamp: r.timestamp,
                ip_address: r.ip_address,
                country: country_display,
                referrer,
                browser,
                user_agent: r.user_agent,
            }
        })
        .collect();

    let body_str = match serde_json::to_string(&export_rows) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Serialization error").into_response()
        }
    };

    let filename = if target_type == "url" {
        format!("url_{}_analytics.json", id)
    } else {
        format!("page_{}_analytics.json", id)
    };

    (
        StatusCode::OK,
        [
            ("Content-Type", "application/json"),
            (
                "Content-Disposition",
                &format!("attachment; filename=\"{}\"", filename),
            ),
            ("X-BZOD-Export-Records", &count.to_string()),
        ],
        body_str,
    )
        .into_response()
}

// --- Feature modules ---

mod auth;
pub use auth::*;
mod dashboard;
pub use dashboard::*;
mod urls;
pub use urls::*;
mod pages;
pub use pages::*;
mod users;
pub use users::*;
mod analytics;
pub use analytics::*;
mod settings;
pub use settings::*;
mod audit;
pub use audit::*;
mod sessions;
pub use sessions::*;
mod quotas;
pub use quotas::*;
mod health;
pub use health::*;
mod backups;
pub use backups::*;
mod api_keys;
pub use api_keys::*;
mod moderation;
pub use moderation::*;

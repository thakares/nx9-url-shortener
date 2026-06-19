use crate::services::qr::{generate_qr_png, generate_qr_svg};
use crate::state::AppState;
use crate::utils::get_client_ip;
use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;

use serde_json::json;

// GET /api/qr/:file (e.g. /api/qr/abcdef.png or /api/qr/abcdef.svg or JSON stats /api/qr/abcdef)
// GET /api/qr/:file (e.g. /api/qr/abcdef.png or /api/qr/abcdef.svg or JSON stats /api/qr/abcdef)
pub async fn qr_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Path(file): Path<String>,
) -> Response {
    let parts: Vec<&str> = file.split('.').collect();
    if parts.len() != 2 {
        // No extension: this is a JSON stats request!
        let auth_header = headers.get("Authorization").and_then(|h| h.to_str().ok());

        let authenticated = if let Some(auth) = auth_header {
            let admin_conn = state.admin_db.lock().unwrap();
            let users_conn = state.users_db.lock().unwrap();
            matches!(
                crate::auth::session::authenticate_api_key(&admin_conn, &users_conn, auth),
                Ok(Some(_user))
            )
        } else {
            false
        };

        if !authenticated {
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }

        // We need to look up owner_user_id and status from global_slugs
        let (owner_user_id, slug_status) = {
            let system_conn = state.system_db.lock().unwrap();
            let mut stmt = match system_conn
                .prepare("SELECT owner_user_id, status FROM global_slugs WHERE slug = ?1;")
            {
                Ok(s) => s,
                Err(_) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
                }
            };
            use rusqlite::OptionalExtension;
            match stmt
                .query_row(rusqlite::params![&file], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .optional()
            {
                Ok(Some((uid, status))) => (uid, status),
                Ok(None) => (1, "active".to_string()), // fallback to admin
                Err(_) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
                }
            }
        };

        if slug_status != "active" {
            return (StatusCode::NOT_FOUND, "URL not found").into_response();
        }

        let user_dbs = match state.get_user_dbs(owner_user_id) {
            Ok(dbs) => dbs,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        };

        let url_opt = {
            let conn = user_dbs.content.lock().unwrap();
            match crate::db::content::get_url_by_code(&conn, &file) {
                Ok(u) => u,
                Err(_) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
                }
            }
        };

        let url = match url_opt {
            Some(u) => u,
            None => return (StatusCode::NOT_FOUND, "URL not found").into_response(),
        };

        let qr_scans = {
            let conn = user_dbs.analytics.lock().unwrap();
            crate::db::qr::get_qr_scan_count(&conn, &url.id).unwrap_or(0)
        };

        let direct_clicks = {
            let conn = user_dbs.analytics.lock().unwrap();
            conn.query_row(
                "SELECT COUNT(*) FROM visits WHERE target_type = 'url' AND target_id = ?1;",
                rusqlite::params![url.id],
                |row| row.get(0),
            )
            .unwrap_or(0)
        };

        return axum::response::Json(json!({
            "direct_clicks": direct_clicks,
            "qr_scans": qr_scans
        }))
        .into_response();
    }

    let code = parts[0];
    let ext = parts[1].to_lowercase();

    if !crate::utils::validation::validate_redirect_code(code) {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    // We need to look up owner_user_id and status from global_slugs
    let (owner_user_id, slug_status) = {
        let system_conn = state.system_db.lock().unwrap();
        let mut stmt = match system_conn
            .prepare("SELECT owner_user_id, status FROM global_slugs WHERE slug = ?1;")
        {
            Ok(s) => s,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        };
        use rusqlite::OptionalExtension;
        match stmt
            .query_row(rusqlite::params![code], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .optional()
        {
            Ok(Some((uid, status))) => (uid, status),
            Ok(None) => (1, "active".to_string()), // fallback to admin
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        }
    };

    if slug_status != "active" {
        return (StatusCode::NOT_FOUND, "Url not found").into_response();
    }

    let user_dbs = match state.get_user_dbs(owner_user_id) {
        Ok(dbs) => dbs,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let url_opt = {
        let conn = user_dbs.content.lock().unwrap();
        match crate::db::content::get_url_by_code(&conn, code) {
            Ok(u) => u,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        }
    };

    let url = match url_opt {
        Some(u) => u,
        None => return (StatusCode::NOT_FOUND, "Url not found").into_response(),
    };

    // Construct public base URL
    let proto = if state.config.cookie_secure {
        "https"
    } else {
        "http"
    };
    let host_header = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost:8654");

    let base_url = state
        .config
        .base_url
        .clone()
        .unwrap_or_else(|| format!("{}://{}", proto, host_header));

    let full_url = format!("{}/{}", base_url.trim_end_matches('/'), code);

    // Generate QR code based on format
    let (body, content_type) = if ext == "svg" {
        match generate_qr_svg(&full_url) {
            Ok(svg) => (svg.into_bytes(), "image/svg+xml"),
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("QR generation error: {}", e),
                )
                    .into_response()
            }
        }
    } else if ext == "png" {
        match generate_qr_png(&full_url, 256) {
            Ok(png) => (png, "image/png"),
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("QR generation error: {}", e),
                )
                    .into_response()
            }
        }
    } else {
        return (
            StatusCode::BAD_REQUEST,
            "Unsupported format. Use .png or .svg",
        )
            .into_response();
    };

    // Log the QR access event
    let ip = get_client_ip(&headers, connect_info);
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    {
        let analytics_conn = user_dbs.analytics.lock().unwrap();
        let _ = crate::db::qr::log_qr_access(
            &analytics_conn,
            &url.id,
            Some(ip.as_str()),
            user_agent.as_deref(),
        );
    }

    Response::builder()
        .header("content-type", content_type)
        .header("cache-control", "public, max-age=86400") // cache for 1 day
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

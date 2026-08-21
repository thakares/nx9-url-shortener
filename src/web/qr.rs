use crate::db::tenant::TenantOpenMode;
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

        // Look up slug info from v0.8 slug registry (with fallback)
        let info = match state.lookup_slug(&file) {
            Ok(Some(info)) => info,
            Ok(None) => return (StatusCode::NOT_FOUND, "URL not found").into_response(),
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        };

        if info.status == "disabled" {
            return (StatusCode::GONE, "This content has been disabled").into_response();
        } else if info.status != "active" {
            return (StatusCode::NOT_FOUND, "URL not found").into_response();
        }

        let user_dbs = match state
            .open_slug_owner(&info.owner_tenant_id, TenantOpenMode::PublicContent)
        {
            Ok(dbs) => dbs,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        };

        let qr_scans = {
            let conn = user_dbs.analytics.lock().unwrap();
            crate::db::qr::get_qr_scan_count(&conn, &info.target_id).unwrap_or(0)
        };

        let direct_clicks = {
            let conn = user_dbs.analytics.lock().unwrap();
            conn.query_row(
                "SELECT COUNT(*) FROM visits WHERE target_id = ?1;",
                rusqlite::params![info.target_id],
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

    if !crate::utils::validation::validate_redirect_code(code)
        && !crate::utils::validation::validate_page_code(code)
    {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    // Look up slug info from v0.8 slug registry (with fallback)
    let info = match state.lookup_slug(code) {
        Ok(Some(info)) => info,
        Ok(None) => return (StatusCode::NOT_FOUND, "Not Found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    if info.status == "disabled" {
        return (StatusCode::GONE, "This content has been disabled").into_response();
    } else if info.status != "active" {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

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

    let full_url = if info.target_type == crate::db::slugs::SlugTargetType::LandingPage {
        format!("{}/p/{}", base_url.trim_end_matches('/'), code)
    } else {
        format!("{}/{}", base_url.trim_end_matches('/'), code)
    };

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

    // Log the QR access event in a try-catch style
    let _ = {
        let ip = get_client_ip(&headers, connect_info);
        let user_agent = headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        if let Ok(user_dbs) =
            state.open_slug_owner(&info.owner_tenant_id, TenantOpenMode::PublicContent)
        {
            if let Ok(analytics_conn) = user_dbs.analytics.lock() {
                let _ = crate::db::qr::log_qr_access(
                    &analytics_conn,
                    &info.target_id,
                    Some(ip.as_str()),
                    user_agent.as_deref(),
                );
            }
        }
    };

    Response::builder()
        .header("content-type", content_type)
        .header("cache-control", "public, max-age=86400") // cache for 1 day
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

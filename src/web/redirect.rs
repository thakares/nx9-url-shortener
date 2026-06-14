use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use chrono::Utc;
use std::net::SocketAddr;
use uuid::Uuid;

use crate::analytics::get_client_country;
use crate::models::VisitRecord;
use crate::services::shortener::get_url_by_code;
use crate::state::AppState;
use crate::templates::PreviewTemplate;
use crate::utils::get_client_ip;

// GET /:code
// Resolve and redirect
pub async fn resolve_redirect(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(code): Path<String>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Response {
    // Basic validation of code (must be 6 hex characters or a valid custom slug)
    if !crate::utils::validation::validate_redirect_code(&code) {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    let url_opt = match get_url_by_code(&state.db, &code) {
        Ok(url) => url,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let url = match url_opt {
        Some(u) => u,
        None => return (StatusCode::NOT_FOUND, "Short code not found").into_response(),
    };

    // 1. Expiration check
    if url.expired {
        return (StatusCode::GONE, "This link has expired").into_response();
    }

    if let Some(ref expires_at_str) = url.expires_at {
        if let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at_str) {
            if expires_at.with_timezone(&Utc) < Utc::now() {
                // Mark as expired in DB asynchronously/immediately
                {
                    let conn = state.db.content.lock().unwrap();
                    let _ = conn.execute(
                        "UPDATE urls SET expired = 1 WHERE id = ?1;",
                        [url.id.clone()],
                    );
                }
                return (StatusCode::GONE, "This link has expired").into_response();
            }
        }
    }

    // 2. Access limit check
    if url.is_access_exhausted() {
        return (
            StatusCode::GONE,
            "This link has reached its maximum access limit",
        )
            .into_response();
    }

    // 3. Password protection check
    if url.is_password_protected() {
        let cookie_name = format!("bzod_gate_{}", code);
        let authorized = jar
            .get(&cookie_name)
            .map(|c| c.value() == "authorized")
            .unwrap_or(false);

        if !authorized {
            return Redirect::temporary(&format!("/gate/{}", code)).into_response();
        }
    }

    // 4. Increment access count & retrieve preview config
    let _new_access_count = {
        let conn = state.db.content.lock().unwrap();
        crate::db::content::increment_access_count(&conn, &url.id).unwrap_or(url.access_count + 1)
    };

    let preview_opt = {
        let conn = state.db.content.lock().unwrap();
        crate::db::preview::get_preview(&conn, &url.id).unwrap_or(None)
    };

    // Asynchronously record analytics
    let ip = get_client_ip(&headers, connect_info);
    let country = get_client_country(&headers);
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("Unknown")
        .to_string();
    let referer = headers
        .get("referer")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("Direct")
        .to_string();
    let accept_language = headers
        .get("accept-language")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("Unknown")
        .to_string();

    let record = VisitRecord {
        id: Uuid::new_v4().to_string(),
        target_type: "url".to_string(),
        target_id: url.id.clone(),
        timestamp: Utc::now().to_rfc3339(),
        ip_address: ip,
        user_agent,
        referer,
        accept_language,
        country,
        status_code: if preview_opt.is_some() { 200 } else { 302 },
    };

    // Push to memory queue (non-blocking)
    state.analytics_queue.push(record);

    // 5. Render Preview or Redirect
    if let Some(preview) = preview_opt {
        PreviewTemplate {
            code,
            title: preview.title,
            description: preview.description,
            logo_url: preview.logo_url,
            button_text: preview.button_text,
            destination: url.destination,
        }
        .into_response()
    } else {
        Redirect::temporary(&url.destination).into_response()
    }
}

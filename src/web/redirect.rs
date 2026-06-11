use axum::{
    extract::{Path, State, ConnectInfo},
    http::{HeaderMap, StatusCode},
    response::{Redirect, Response, IntoResponse},
};
use std::net::SocketAddr;
use uuid::Uuid;
use chrono::Utc;

use crate::state::AppState;
use crate::models::VisitRecord;
use crate::utils::get_client_ip;
use crate::analytics::get_client_country;
use crate::services::shortener::get_url_by_code;

// GET /:code
// Resolve and redirect
pub async fn resolve_redirect(
    State(state): State<AppState>,
    Path(code): Path<String>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Response {
    // Basic validation of code (must be 6 hex characters)
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    let url_opt = match get_url_by_code(&state.db, &code) {
        Ok(url) => url,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    match url_opt {
        Some(url) => {
            // Asynchronously record analytics
            let ip = get_client_ip(&headers, connect_info);
            let country = get_client_country(&headers);
            let user_agent = headers.get("user-agent")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("Unknown")
                .to_string();
            let referer = headers.get("referer")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("Direct")
                .to_string();
            let accept_language = headers.get("accept-language")
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
                status_code: 302,
            };

            // Push to memory queue (non-blocking)
            state.analytics_queue.push(record);

            // Perform redirect
            Redirect::temporary(&url.destination).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Short code not found").into_response(),
    }
}

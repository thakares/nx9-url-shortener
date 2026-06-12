use crate::auth::generate_token;
use crate::auth::password::hash_password;
use crate::auth::ApiUser;
use crate::state::AppState;
use crate::utils::get_client_ip;
use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Deserialize)]
pub struct BulkQrRequest {
    pub ids: Vec<String>,
    pub format: Option<String>,
}

#[derive(Deserialize)]
pub struct BulkUrlItem {
    pub destination: String,
    pub code: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub expires_at: Option<String>,
    pub password: Option<String>,
    pub max_access_count: Option<i64>,
}

#[derive(Serialize)]
pub struct BulkErrorResponse {
    pub error: String,
}

// POST /api/v1/bulk/qr
pub async fn api_bulk_qr(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: ApiUser,
    Json(payload): Json<BulkQrRequest>,
) -> Response {
    if payload.ids.len() > 500 {
        return (
            StatusCode::BAD_REQUEST,
            Json(BulkErrorResponse {
                error: "Maximum 500 QR codes allowed per bulk request".to_string(),
            }),
        )
            .into_response();
    }

    let format = payload
        .format
        .unwrap_or_else(|| "png".to_string())
        .to_lowercase();
    if format != "png" && format != "svg" {
        return (
            StatusCode::BAD_REQUEST,
            Json(BulkErrorResponse {
                error: "Invalid format. Supported: png, svg".to_string(),
            }),
        )
            .into_response();
    }

    // Retrieve URLs from database
    let mut urls = Vec::new();
    {
        let conn = state.content_db.lock().unwrap();
        for id in &payload.ids {
            match crate::db::content::get_url_by_id(&conn, id) {
                Ok(Some(url)) => urls.push(url),
                Ok(None) => {}
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(BulkErrorResponse {
                            error: format!("Database error fetching URL {}: {}", id, e),
                        }),
                    )
                        .into_response();
                }
            }
        }
    }

    if urls.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(BulkErrorResponse {
                error: "No valid URLs found for the provided IDs".to_string(),
            }),
        )
            .into_response();
    }

    // Base URL configuration check
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

    // Generate ZIP
    match crate::services::bulk::export_qr_zip(&urls, &format, &base_url) {
        Ok(zip_data) => {
            // Write Audit Log
            {
                let system_conn = state.db.system.lock().unwrap();
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    &user.0.username,
                    "BULK_QR_EXPORT",
                    "bulk",
                    "qr",
                    Some(&format!("Count: {}, Format: {}", urls.len(), format)),
                );
            }

            Response::builder()
                .header("content-type", "application/zip")
                .header(
                    "content-disposition",
                    "attachment; filename=\"qr_codes.zip\"",
                )
                .body(axum::body::Body::from(zip_data))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(BulkErrorResponse {
                error: format!("Error generating ZIP: {}", e),
            }),
        )
            .into_response(),
    }
}

// POST /api/v1/bulk/url
pub async fn api_bulk_url(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    user: ApiUser,
    Json(payload): Json<Vec<BulkUrlItem>>,
) -> Response {
    if payload.len() > 500 {
        return (
            StatusCode::BAD_REQUEST,
            Json(BulkErrorResponse {
                error: "Maximum 500 URLs allowed per bulk creation".to_string(),
            }),
        )
            .into_response();
    }

    let mut conn = state.content_db.lock().unwrap();
    let tx = match conn.transaction() {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(BulkErrorResponse {
                    error: format!("Failed to start database transaction: {}", e),
                }),
            )
                .into_response()
        }
    };

    let mut created_urls = Vec::new();

    for item in payload {
        let mut code = item.code.unwrap_or_default().trim().to_lowercase();
        if code.is_empty() {
            code = generate_token(3); // 6 hex
        } else {
            if code.len() != 6 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
                let _ = tx.rollback();
                return (
                    StatusCode::BAD_REQUEST,
                    Json(BulkErrorResponse {
                        error: format!("Short code '{}' must be 6 hex characters", code),
                    }),
                )
                    .into_response();
            }
        }

        let password_hash = if let Some(ref pwd) = item.password {
            match hash_password(pwd) {
                Ok(h) => Some(h),
                Err(e) => {
                    let _ = tx.rollback();
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(BulkErrorResponse {
                            error: format!("Password hashing error: {}", e),
                        }),
                    )
                        .into_response();
                }
            }
        } else {
            None
        };

        let tags = item.tags.unwrap_or_default();
        match crate::db::content::create_url_extended(
            &tx,
            &code,
            &item.destination,
            item.title.as_deref(),
            item.description.as_deref(),
            &tags,
            item.expires_at.as_deref(),
            password_hash.as_deref(),
            item.max_access_count,
        ) {
            Ok(url) => created_urls.push(url),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                let _ = tx.rollback();
                return (
                    StatusCode::CONFLICT,
                    Json(BulkErrorResponse {
                        error: format!("Short code '{}' already exists", code),
                    }),
                )
                    .into_response();
            }
            Err(e) => {
                let _ = tx.rollback();
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(BulkErrorResponse {
                        error: format!("Database insert error: {}", e),
                    }),
                )
                    .into_response();
            }
        }
    }

    if let Err(e) = tx.commit() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(BulkErrorResponse {
                error: format!("Failed to commit transaction: {}", e),
            }),
        )
            .into_response();
    }

    // Write Audit Log for the entire batch
    let ip = get_client_ip(&headers, connect_info);
    let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
    {
        let system_conn = state.db.system.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            &user.0.username,
            "BULK_URL_CREATION",
            "bulk",
            "url",
            Some(&format!(
                "Count: {}, IP: {:?}, User-Agent: {:?}",
                created_urls.len(),
                ip,
                user_agent
            )),
        );
    }

    (StatusCode::CREATED, Json(created_urls)).into_response()
}

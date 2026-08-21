use crate::auth::ApiUser;
use crate::services::bulk_urls::{
    create_urls_bulk, ensure_url_quota, BulkUrlCreateItem, BulkUrlError,
};
use crate::state::AppState;
use crate::utils::{get_client_ip, lock_db};
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
    let content_db = match user.0 {
        crate::models::ApiActor::User(ref u) => {
            let user_dbs = match state.get_user_dbs(u.id) {
                Ok(dbs) => dbs,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(BulkErrorResponse {
                            error: "Database error".to_string(),
                        }),
                    )
                        .into_response();
                }
            };
            user_dbs.content.clone()
        }
        crate::models::ApiActor::Admin(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(BulkErrorResponse {
                    error: "Admin is a platform operator and cannot export application URLs without tenant context".to_string(),
                }),
            )
                .into_response();
        }
    };

    let mut urls = Vec::new();
    {
        let conn = match lock_db(&content_db, "content_db") {
            Ok(c) => c,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(BulkErrorResponse {
                        error: e.to_string(),
                    }),
                )
                    .into_response();
            }
        };
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
            // Write Audit Log (best-effort; do not fail the download on audit lock poison)
            if let Ok(system_conn) = lock_db(&state.db.system, "system_db") {
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    user.0.username(),
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

fn bulk_url_error_response(err: BulkUrlError) -> Response {
    let (status, msg) = match &err {
        BulkUrlError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
        BulkUrlError::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
        BulkUrlError::Forbidden(m) => (StatusCode::FORBIDDEN, m.clone()),
        BulkUrlError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m.clone()),
    };
    (status, Json(BulkErrorResponse { error: msg })).into_response()
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

    // Dynamically resolve target user ID, TenantId, and content DB
    let (target_user_id, target_tenant_id, content_db) = match user.0 {
        crate::models::ApiActor::Admin(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(BulkErrorResponse {
                    error: "Admin is a platform operator and cannot create application URLs directly without tenant context".to_string(),
                }),
            )
                .into_response();
        }
        crate::models::ApiActor::User(ref u) => {
            let tenant_id = match u.tenant_id {
                Some(tid) => tid,
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(BulkErrorResponse {
                            error: "User has no tenant context".to_string(),
                        }),
                    )
                        .into_response();
                }
            };
            let user_dbs = match state.get_user_dbs(u.id) {
                Ok(dbs) => dbs,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(BulkErrorResponse {
                            error: "Database error".to_string(),
                        }),
                    )
                        .into_response()
                }
            };
            (u.id, tenant_id, user_dbs.content.clone())
        }
    };

    if let Err(e) = ensure_url_quota(&state.users_db, target_user_id, payload.len() as i64) {
        return bulk_url_error_response(e);
    }

    let items: Vec<BulkUrlCreateItem> = payload
        .into_iter()
        .map(|item| BulkUrlCreateItem {
            destination: item.destination,
            code: item.code,
            title: item.title,
            description: item.description,
            tags: item.tags,
            expires_at: item.expires_at,
            password: item.password,
            max_access_count: item.max_access_count,
        })
        .collect();

    let created_urls = match create_urls_bulk(
        &content_db,
        &state.db.reserved,
        &state.db.global_urls,
        &state.db.global_landing_pages,
        &state.users_db,
        target_user_id,
        target_tenant_id,
        items,
    ) {
        Ok(urls) => urls,
        Err(e) => return bulk_url_error_response(e),
    };

    // Write Audit Log for the entire batch
    let ip = get_client_ip(&headers, connect_info);
    let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
    if let Ok(system_conn) = lock_db(&state.db.system, "system_db") {
        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            user.0.username(),
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

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::auth::generate_token;
use crate::auth::password::hash_password;
use crate::auth::ApiUser;
use crate::db::admin::write_audit_log;
use crate::db::analytics::{
    get_clicks_trend, get_clicks_trend_raw, get_metric_rankings, get_metric_rankings_raw,
};
use crate::db::content::{
    create_landing_page, delete_landing_page, delete_url, get_landing_page_by_id, get_url_by_id,
    list_landing_pages, list_urls, update_landing_page, update_url,
};
use crate::state::AppState;
use crate::utils::get_client_ip;

// JSON Payload Structs
#[derive(Deserialize)]
pub struct CreateUrlRequest {
    pub destination: String,
    pub code: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub expires_at: Option<String>,
    pub password: Option<String>,
    pub max_access_count: Option<i64>,
    pub utm_source: Option<String>,
    pub utm_medium: Option<String>,
    pub utm_campaign: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateUrlRequest {
    pub destination: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: String, // 'healthy', 'suspect', 'dead'
    pub tags: Option<Vec<String>>,
    pub expires_at: Option<String>,
    pub password: Option<String>,
    pub max_access_count: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreatePageRequest {
    pub slug: String,
    pub title: String,
    pub html_content: String,
    pub state: String, // 'draft', 'published', 'archived'
    pub code: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdatePageRequest {
    pub slug: String,
    pub title: String,
    pub html_content: String,
    pub state: String,
}

// Error JSON Response
#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
}

#[allow(clippy::result_large_err)]
fn get_api_tenant_dbs(state: &AppState, user: &ApiUser) -> Result<crate::state::UserDbs, Response> {
    match user.0 {
        crate::models::ApiActor::User(ref u) => {
            state.get_user_dbs(u.id).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError {
                        error: "Database error".to_string(),
                    }),
                )
                    .into_response()
            })
        }
        crate::models::ApiActor::Admin(_) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Admin is a platform operator and cannot access application content directly without tenant context".to_string(),
            }),
        )
            .into_response()),
    }
}

// --- URL Endpoints ---

// POST /api/v1/urls
pub async fn api_create_url(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    user: ApiUser,
    Json(payload): Json<CreateUrlRequest>,
) -> Response {
    let mut code = payload.code.unwrap_or_default().trim().to_lowercase();
    if code.is_empty() {
        code = generate_token(3); // 6 hex
    } else {
        if !crate::utils::validation::validate_redirect_code(&code) {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "Short code must be 6 hex characters or a custom slug starting with !"
                        .to_string(),
                }),
            )
                .into_response();
        }
    }

    let dest = match crate::services::urls::prepare_destination(
        &payload.destination,
        crate::services::urls::UtmParams {
            source: payload.utm_source.as_deref(),
            medium: payload.utm_medium.as_deref(),
            campaign: payload.utm_campaign.as_deref(),
        },
    ) {
        Ok(d) => d,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };

    let password_hash = if let Some(ref pwd) = payload.password {
        if pwd.is_empty() {
            None
        } else {
            match hash_password(pwd) {
                Ok(h) => Some(h),
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiError {
                            error: format!("Password hashing error: {}", e),
                        }),
                    )
                        .into_response()
                }
            }
        }
    } else {
        None
    };

    // Dynamically resolve target user ID, tenant ID, and content DB
    let (target_user_id, target_tenant_id, content_db) = match user.0 {
        crate::models::ApiActor::Admin(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "Admin is a platform operator and cannot create application URLs directly without tenant context".to_string(),
                }),
            )
                .into_response();
        }
        crate::models::ApiActor::User(ref u) => {
            let user_dbs = match state.get_user_dbs(u.id) {
                Ok(dbs) => dbs,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiError {
                            error: "Database error".to_string(),
                        }),
                    )
                        .into_response()
                }
            };
            let tid = u
                .tenant_id
                .unwrap_or_else(crate::identity::TenantId::generate);
            (u.id, tid, user_dbs.content.clone())
        }
    };

    // Check quota
    {
        let users_conn = state.users_db.lock().unwrap();
        if !crate::db::users::check_quota_limit(&users_conn, target_user_id, "urls")
            .unwrap_or(false)
        {
            return (
                StatusCode::FORBIDDEN,
                Json(ApiError {
                    error: "Quota limit exceeded".to_string(),
                }),
            )
                .into_response();
        }
    }

    // Check availability and reserve in v0.8 slug registry
    {
        let reserved_conn = state.db.reserved.lock().unwrap();
        let urls_conn = state.db.global_urls.lock().unwrap();
        let pages_conn = state.db.global_landing_pages.lock().unwrap();
        if let Err(e) = crate::db::slugs::reserve_url_slug(
            &reserved_conn,
            &urls_conn,
            &pages_conn,
            &code,
            &target_tenant_id,
        ) {
            return (
                StatusCode::CONFLICT,
                Json(ApiError {
                    error: format!("Short code unavailable: {}", e),
                }),
            )
                .into_response();
        }
    }

    let tags = payload.tags.unwrap_or_default();
    let res = {
        let conn = content_db.lock().unwrap();
        crate::db::content::create_url_extended(
            &conn,
            &code,
            &dest,
            payload.title.as_deref(),
            payload.description.as_deref(),
            &tags,
            payload.expires_at.as_deref(),
            password_hash.as_deref(),
            payload.max_access_count,
        )
    };

    match res {
        Ok(url) => {
            // Activate slug in v0.8 global_urls.db
            {
                let urls_conn = state.db.global_urls.lock().unwrap();
                let _ = crate::db::slugs::activate_url_slug(&urls_conn, &code, &url.id);
            }
            // Increment quota
            {
                let users_conn = state.users_db.lock().unwrap();
                let _ =
                    crate::db::users::increment_quota_counter(&users_conn, target_user_id, "urls");
            }

            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                user.0.username(),
                "URL_CREATION",
                Some("url"),
                Some(&url.id),
                Some(&ip),
                user_agent,
            );

            // Log to system.db
            {
                let system_conn = state.system_db.lock().unwrap();
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    user.0.username(),
                    "URL_CREATION",
                    "url",
                    &url.id,
                    Some(&format!("IP: {:?}, User-Agent: {:?}", ip, user_agent)),
                );
            }
            (StatusCode::CREATED, Json(url)).into_response()
        }
        Err(e) => {
            let urls_conn = state.db.global_urls.lock().unwrap();
            let _ = crate::db::slugs::release_url_slug(&urls_conn, &code, &target_tenant_id);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            )
                .into_response()
        }
    }
}

// GET /api/v1/urls
#[derive(Deserialize)]
pub struct ListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub tag: Option<String>,
}

pub async fn api_list_urls(
    State(state): State<AppState>,
    user: ApiUser,
    Query(query): Query<ListQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let conn = dbs.content.lock().unwrap();
    match list_urls(&conn, limit, offset, query.tag.as_deref()) {
        Ok(urls) => Json(urls).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// GET /api/v1/urls/:uuid
pub async fn api_get_url(
    State(state): State<AppState>,
    user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let conn = dbs.content.lock().unwrap();
    match get_url_by_id(&conn, &uuid) {
        Ok(Some(url)) => Json(url).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "URL not found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// PUT /api/v1/urls/:uuid
pub async fn api_update_url(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    user: ApiUser,
    Path(uuid): Path<String>,
    Json(payload): Json<UpdateUrlRequest>,
) -> Response {
    let tags = payload.tags.unwrap_or_default();
    if !crate::utils::validation::validate_redirect_destination(&payload.destination) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Destination must be a valid http(s) URL without control characters"
            })),
        )
            .into_response();
    }
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let conn = dbs.content.lock().unwrap();
    match update_url(
        &conn,
        &uuid,
        &payload.destination,
        payload.title.as_deref(),
        payload.description.as_deref(),
        &payload.status,
        &tags,
    ) {
        Ok(Some(url)) => {
            // Update password
            if let Some(ref pwd) = payload.password {
                if pwd.is_empty() {
                    let _ = crate::db::content::remove_url_password(&conn, &uuid);
                } else {
                    if let Ok(hash) = hash_password(pwd) {
                        let _ = crate::db::content::set_url_password(&conn, &uuid, &hash);
                    }
                }
            }

            // Update expires_at and max_access_count
            let _ = conn.execute(
                "UPDATE urls SET expires_at = ?1, max_access_count = ?2 WHERE id = ?3;",
                rusqlite::params![
                    payload.expires_at.as_deref(),
                    payload.max_access_count,
                    uuid
                ],
            );

            // Fetch the fully updated URL
            let updated_url = get_url_by_id(&conn, &uuid).unwrap_or(Some(url)).unwrap();

            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                user.0.username(),
                "URL_UPDATE",
                Some("url"),
                Some(&uuid),
                Some(&ip),
                user_agent,
            );

            // Log to system.db
            {
                let system_conn = state.system_db.lock().unwrap();
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    user.0.username(),
                    "URL_UPDATE",
                    "url",
                    &uuid,
                    Some(&format!("IP: {:?}, User-Agent: {:?}", ip, user_agent)),
                );
            }

            Json(updated_url).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "URL not found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// DELETE /api/v1/urls/:uuid
pub async fn api_delete_url(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let conn = dbs.content.lock().unwrap();
    let code_opt = match get_url_by_id(&conn, &uuid) {
        Ok(Some(u)) => Some(u.code),
        _ => None,
    };
    match delete_url(&conn, &uuid) {
        Ok(true) => {
            if let Some(code) = code_opt {
                let urls_conn = state.db.global_urls.lock().unwrap();
                let _ = urls_conn.execute("DELETE FROM global_urls WHERE slug = ?1;", [&code]);
            }
            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                user.0.username(),
                "URL_DELETION",
                Some("url"),
                Some(&uuid),
                Some(&ip),
                user_agent,
            );

            // Log to system.db
            {
                let system_conn = state.system_db.lock().unwrap();
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    user.0.username(),
                    "URL_DELETION",
                    "url",
                    &uuid,
                    Some(&format!("IP: {:?}, User-Agent: {:?}", ip, user_agent)),
                );
            }

            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "URL not found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// --- Landing Page Endpoints ---

// POST /api/v1/pages
pub async fn api_create_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    user: ApiUser,
    Json(payload): Json<CreatePageRequest>,
) -> Response {
    let mut code = payload.code.unwrap_or_default().trim().to_lowercase();
    if code.is_empty() {
        code = generate_token(2); // 4 hex
    } else {
        if !crate::utils::validation::validate_page_code(&code) {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "Short code must be 4 hex characters or start with ! followed by 1-24 characters of a-z, 0-9, -, _".to_string(),
                }),
            )
                .into_response();
        }
    }

    // Dynamically resolve target user ID, tenant ID, and content DB
    let (target_user_id, target_tenant_id, content_db) = match user.0 {
        crate::models::ApiActor::Admin(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "Admin is a platform operator and cannot create application landing pages directly without tenant context".to_string(),
                }),
            )
                .into_response();
        }
        crate::models::ApiActor::User(ref u) => {
            let user_dbs = match state.get_user_dbs(u.id) {
                Ok(dbs) => dbs,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiError {
                            error: "Database error".to_string(),
                        }),
                    )
                        .into_response()
                }
            };
            let tid = u
                .tenant_id
                .unwrap_or_else(crate::identity::TenantId::generate);
            (u.id, tid, user_dbs.content.clone())
        }
    };

    // Check quota
    {
        let users_conn = state.users_db.lock().unwrap();
        if !crate::db::users::check_quota_limit(&users_conn, target_user_id, "landings")
            .unwrap_or(false)
        {
            return (
                StatusCode::FORBIDDEN,
                Json(ApiError {
                    error: "Quota limit exceeded".to_string(),
                }),
            )
                .into_response();
        }
    }

    // Check availability and reserve in v0.8 slug registry
    {
        let reserved_conn = state.db.reserved.lock().unwrap();
        let urls_conn = state.db.global_urls.lock().unwrap();
        let pages_conn = state.db.global_landing_pages.lock().unwrap();
        if let Err(e) = crate::db::slugs::reserve_landing_page_slug(
            &reserved_conn,
            &urls_conn,
            &pages_conn,
            &code,
            &target_tenant_id,
        ) {
            return (
                StatusCode::CONFLICT,
                Json(ApiError {
                    error: format!("Short code unavailable: {}", e),
                }),
            )
                .into_response();
        }
    }

    let res = {
        let conn = content_db.lock().unwrap();
        create_landing_page(
            &conn,
            &code,
            &payload.slug,
            &payload.title,
            &payload.html_content,
            &payload.state,
        )
    };

    match res {
        Ok(page) => {
            // Activate slug in v0.8 global_landing_pages.db
            {
                let pages_conn = state.db.global_landing_pages.lock().unwrap();
                let _ = crate::db::slugs::activate_landing_page_slug(&pages_conn, &code, &page.id);
            }
            // Increment quota
            {
                let users_conn = state.users_db.lock().unwrap();
                let _ = crate::db::users::increment_quota_counter(
                    &users_conn,
                    target_user_id,
                    "landings",
                );
            }

            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                user.0.username(),
                "PAGE_CREATION",
                Some("page"),
                Some(&page.id),
                Some(&ip),
                user_agent,
            );
            (StatusCode::CREATED, Json(page)).into_response()
        }
        Err(e) => {
            let pages_conn = state.db.global_landing_pages.lock().unwrap();
            let _ =
                crate::db::slugs::release_landing_page_slug(&pages_conn, &code, &target_tenant_id);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: e.to_string(),
                }),
            )
                .into_response()
        }
    }
}

// GET /api/v1/pages
pub async fn api_list_pages(
    State(state): State<AppState>,
    user: ApiUser,
    Query(query): Query<ListQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let conn = dbs.content.lock().unwrap();
    match list_landing_pages(&conn, limit, offset) {
        Ok(pages) => Json(pages).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// GET /api/v1/pages/:uuid
pub async fn api_get_page(
    State(state): State<AppState>,
    user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let conn = dbs.content.lock().unwrap();
    match get_landing_page_by_id(&conn, &uuid) {
        Ok(Some(page)) => Json(page).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "Page not found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// PUT /api/v1/pages/:uuid
pub async fn api_update_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    user: ApiUser,
    Path(uuid): Path<String>,
    Json(payload): Json<UpdatePageRequest>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let conn = dbs.content.lock().unwrap();
    match update_landing_page(
        &conn,
        &uuid,
        &payload.slug,
        &payload.title,
        &payload.html_content,
        &payload.state,
    ) {
        Ok(Some(page)) => {
            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                user.0.username(),
                "PAGE_UPDATE",
                Some("page"),
                Some(&uuid),
                Some(&ip),
                user_agent,
            );
            Json(page).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "Page not found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// DELETE /api/v1/pages/:uuid
pub async fn api_delete_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let conn = dbs.content.lock().unwrap();
    let code_opt = match get_landing_page_by_id(&conn, &uuid) {
        Ok(Some(p)) => Some(p.code),
        _ => None,
    };
    match delete_landing_page(&conn, &uuid) {
        Ok(true) => {
            if let Some(code) = code_opt {
                let pages_conn = state.db.global_landing_pages.lock().unwrap();
                let _ = pages_conn
                    .execute("DELETE FROM global_landing_pages WHERE slug = ?1;", [&code]);
            }
            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                user.0.username(),
                "PAGE_DELETION",
                Some("page"),
                Some(&uuid),
                Some(&ip),
                user_agent,
            );
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "Page not found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// --- Statistics Endpoints ---

#[derive(Serialize)]
pub struct OverallStatsResponse {
    pub total_urls: i64,
    pub total_pages: i64,
    pub total_clicks: i64,
    pub total_page_views: i64,
    pub active_links: i64,
    pub dead_links: i64,
}

// GET /api/v1/stats
pub async fn api_overall_stats(State(state): State<AppState>, user: ApiUser) -> Response {
    if let Err(err) = user.require_admin() {
        return err.into_response();
    }

    let (total_urls, active_links, dead_links) = {
        let conn = state.db.global_urls.lock().unwrap();
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM global_urls WHERE status != 'retired';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let active: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM global_urls WHERE status = 'active';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let dead: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM global_urls WHERE status = 'disabled';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (total, active, dead)
    };
    let total_pages = {
        let conn = state.db.global_landing_pages.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM global_landing_pages WHERE status != 'retired';",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };
    let total_clicks = {
        let users_conn = state.users_db.lock().unwrap();
        crate::db::users::get_platform_total_clicks(&state.db.topology, &users_conn).unwrap_or(0)
    };
    let total_page_views = 0i64;

    Json(OverallStatsResponse {
        total_urls,
        total_pages,
        total_clicks,
        total_page_views,
        active_links,
        dead_links,
    })
    .into_response()
}

#[derive(Serialize)]
pub struct DetailStatsResponse {
    pub target_id: String,
    pub clicks: Vec<(String, i64)>,
    pub top_countries: Vec<(String, i64)>,
    pub top_referrers: Vec<(String, i64)>,
    pub top_browsers: Vec<(String, i64)>,
}

// GET /api/v1/stats/url/:uuid
pub async fn api_url_stats(
    State(state): State<AppState>,
    user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };

    // Verify URL exists
    {
        let conn = dbs.content.lock().unwrap();
        if get_url_by_id(&conn, &uuid).unwrap_or(None).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "URL not found".to_string(),
                }),
            )
                .into_response();
        }
    }

    let conn = dbs.analytics.lock().unwrap();
    let clicks = get_clicks_trend(&conn, "url", &uuid, 30)
        .or_else(|_| get_clicks_trend_raw(&conn, "url", &uuid, 30))
        .unwrap_or_default();
    let top_countries = get_metric_rankings(&conn, "url", &uuid, "country", 10)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &uuid, "country", 10))
        .unwrap_or_default();
    let top_referrers = get_metric_rankings(&conn, "url", &uuid, "referrer", 10)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &uuid, "referrer", 10))
        .unwrap_or_default();
    let top_browsers = get_metric_rankings(&conn, "url", &uuid, "browser", 10)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &uuid, "browser", 10))
        .unwrap_or_default();

    Json(DetailStatsResponse {
        target_id: uuid,
        clicks,
        top_countries,
        top_referrers,
        top_browsers,
    })
    .into_response()
}

// GET /api/v1/stats/page/:uuid
pub async fn api_page_stats(
    State(state): State<AppState>,
    user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };

    // Verify Page exists
    {
        let conn = dbs.content.lock().unwrap();
        if get_landing_page_by_id(&conn, &uuid)
            .unwrap_or(None)
            .is_none()
        {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "Page not found".to_string(),
                }),
            )
                .into_response();
        }
    }

    let conn = dbs.analytics.lock().unwrap();
    let clicks = get_clicks_trend(&conn, "page", &uuid, 30)
        .or_else(|_| get_clicks_trend_raw(&conn, "page", &uuid, 30))
        .unwrap_or_default();
    let top_countries = get_metric_rankings(&conn, "page", &uuid, "country", 10)
        .or_else(|_| get_metric_rankings_raw(&conn, "page", &uuid, "country", 10))
        .unwrap_or_default();
    let top_referrers = get_metric_rankings(&conn, "page", &uuid, "referrer", 10)
        .or_else(|_| get_metric_rankings_raw(&conn, "page", &uuid, "referrer", 10))
        .unwrap_or_default();
    let top_browsers = get_metric_rankings(&conn, "page", &uuid, "browser", 10)
        .or_else(|_| get_metric_rankings_raw(&conn, "page", &uuid, "browser", 10))
        .unwrap_or_default();

    Json(DetailStatsResponse {
        target_id: uuid,
        clicks,
        top_countries,
        top_referrers,
        top_browsers,
    })
    .into_response()
}

// --- QR Stats Endpoints ---

#[derive(Serialize)]
pub struct QrStatsResponse {
    pub code: String,
    pub scan_count: i64,
    pub scans: Vec<(String, String)>,
}

// GET /api/v1/qr/:code
pub async fn api_get_qr_stats(
    State(state): State<AppState>,
    user: ApiUser,
    Path(code): Path<String>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };

    let url_opt = {
        let conn = dbs.content.lock().unwrap();
        crate::db::content::get_url_by_code(&conn, &code).unwrap_or(None)
    };

    let url = match url_opt {
        Some(u) => u,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "URL not found".to_string(),
                }),
            )
                .into_response()
        }
    };

    let (scan_count, scans) = {
        let conn = dbs.analytics.lock().unwrap();
        let count = crate::db::qr::get_qr_scan_count(&conn, &url.id).unwrap_or(0);
        let scans_list = crate::db::qr::get_qr_stats_for_url(&conn, &url.id).unwrap_or_default();
        (count, scans_list)
    };

    Json(QrStatsResponse {
        code,
        scan_count,
        scans,
    })
    .into_response()
}

// --- Audit Trail Endpoints ---

#[derive(Deserialize)]
pub struct AuditQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub actor: Option<String>,
    pub action: Option<String>,
}

// GET /api/v1/audit
pub async fn api_list_audit(
    State(state): State<AppState>,
    user: ApiUser,
    Query(query): Query<AuditQuery>,
) -> Response {
    if let Err(err) = user.require_admin() {
        return err.into_response();
    }

    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);

    let conn = state.system_db.lock().unwrap();
    match crate::db::audit_events::list_audit_events(
        &conn,
        limit,
        offset,
        query.actor.as_deref(),
        query.action.as_deref(),
    ) {
        Ok(events) => Json(events).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// --- Preview Endpoints ---

#[derive(Deserialize)]
pub struct SetPreviewRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub button_text: Option<String>,
}

// POST /api/v1/urls/:uuid/preview
pub async fn api_set_preview(
    State(state): State<AppState>,
    user: ApiUser,
    Path(uuid): Path<String>,
    Json(payload): Json<SetPreviewRequest>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };

    // Verify URL exists
    {
        let conn = dbs.content.lock().unwrap();
        if get_url_by_id(&conn, &uuid).unwrap_or(None).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "URL not found".to_string(),
                }),
            )
                .into_response();
        }
    }

    let conn = dbs.content.lock().unwrap();
    match crate::db::preview::upsert_preview(
        &conn,
        &uuid,
        payload.title.as_deref(),
        payload.description.as_deref(),
        payload.logo_url.as_deref(),
        payload.button_text.as_deref(),
    ) {
        Ok(preview) => {
            // Audit Log
            {
                let system_conn = state.system_db.lock().unwrap();
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    user.0.username(),
                    "SET_PREVIEW",
                    "url",
                    &uuid,
                    None,
                );
            }
            (StatusCode::OK, Json(preview)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// GET /api/v1/urls/:uuid/preview
pub async fn api_get_preview(
    State(state): State<AppState>,
    user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };

    // Verify URL exists
    {
        let conn = dbs.content.lock().unwrap();
        if get_url_by_id(&conn, &uuid).unwrap_or(None).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "URL not found".to_string(),
                }),
            )
                .into_response();
        }
    }

    let conn = dbs.content.lock().unwrap();
    match crate::db::preview::get_preview(&conn, &uuid) {
        Ok(Some(preview)) => Json(preview).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "Preview not configured for this URL".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// DELETE /api/v1/urls/:uuid/preview
pub async fn api_delete_preview(
    State(state): State<AppState>,
    user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };

    // Verify URL exists
    {
        let conn = dbs.content.lock().unwrap();
        if get_url_by_id(&conn, &uuid).unwrap_or(None).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "URL not found".to_string(),
                }),
            )
                .into_response();
        }
    }

    let conn = dbs.content.lock().unwrap();
    match crate::db::preview::delete_preview(&conn, &uuid) {
        Ok(true) => {
            // Audit Log
            {
                let system_conn = state.system_db.lock().unwrap();
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    user.0.username(),
                    "DELETE_PREVIEW",
                    "url",
                    &uuid,
                    None,
                );
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "Preview not configured".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// --- Password Endpoints ---

#[derive(Deserialize)]
pub struct SetPasswordRequest {
    pub password: String,
}

// POST /api/v1/urls/:uuid/password
pub async fn api_set_password(
    State(state): State<AppState>,
    user: ApiUser,
    Path(uuid): Path<String>,
    Json(payload): Json<SetPasswordRequest>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };

    // Verify URL exists
    {
        let conn = dbs.content.lock().unwrap();
        if get_url_by_id(&conn, &uuid).unwrap_or(None).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "URL not found".to_string(),
                }),
            )
                .into_response();
        }
    }

    let hash = match hash_password(&payload.password) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: format!("Password hashing error: {}", e),
                }),
            )
                .into_response()
        }
    };

    let conn = dbs.content.lock().unwrap();
    match crate::db::content::set_url_password(&conn, &uuid, &hash) {
        Ok(true) => {
            // Audit Log
            {
                let system_conn = state.system_db.lock().unwrap();
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    user.0.username(),
                    "SET_PASSWORD",
                    "url",
                    &uuid,
                    None,
                );
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({ "status": "success" })),
            )
                .into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "URL not found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// DELETE /api/v1/urls/:uuid/password
pub async fn api_remove_password(
    State(state): State<AppState>,
    user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };

    // Verify URL exists
    {
        let conn = dbs.content.lock().unwrap();
        if get_url_by_id(&conn, &uuid).unwrap_or(None).is_none() {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "URL not found".to_string(),
                }),
            )
                .into_response();
        }
    }

    let conn = dbs.content.lock().unwrap();
    match crate::db::content::remove_url_password(&conn, &uuid) {
        Ok(true) => {
            // Audit Log
            {
                let system_conn = state.system_db.lock().unwrap();
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    user.0.username(),
                    "REMOVE_PASSWORD",
                    "url",
                    &uuid,
                    None,
                );
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({ "status": "success" })),
            )
                .into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "URL not found".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct CreateQrRequest {
    pub url_id: String,
    pub style: Option<String>,
}

// POST /api/qr or /api/v1/qr
pub async fn api_create_qr(
    State(state): State<AppState>,
    user: ApiUser,
    Json(payload): Json<CreateQrRequest>,
) -> Response {
    let dbs = match get_api_tenant_dbs(&state, &user) {
        Ok(d) => d,
        Err(r) => return r,
    };

    // Verify URL exists in content.db
    {
        let conn = dbs.content.lock().unwrap();
        if get_url_by_id(&conn, &payload.url_id)
            .unwrap_or(None)
            .is_none()
        {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "URL not found".to_string(),
                }),
            )
                .into_response();
        }
    }

    let style = payload.style.unwrap_or_else(|| "default".to_string());
    let conn = dbs.content.lock().unwrap();
    match crate::db::qr::upsert_qr_code(&conn, &payload.url_id, &style) {
        Ok(_) => {
            // Write Audit Event
            {
                let system_conn = state.system_db.lock().unwrap();
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    user.0.username(),
                    "CREATE_QR",
                    "qr_code",
                    &payload.url_id,
                    Some(&format!("Style: {}", style)),
                );
            }
            (StatusCode::OK, Json(serde_json::json!({ "status": "success", "url_id": payload.url_id, "style": style }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

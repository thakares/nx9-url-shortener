use axum::{
    extract::{Path, State, Query, ConnectInfo},
    http::{StatusCode, HeaderMap},
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::db::content::{
    list_urls, get_url_by_id, create_url, update_url, delete_url,
    list_landing_pages, get_landing_page_by_id, create_landing_page, update_landing_page, delete_landing_page,
    get_url_counts, get_landing_page_count
};
use crate::db::analytics::{
    get_total_clicks, get_total_page_views, get_clicks_trend, get_clicks_trend_raw,
    get_metric_rankings, get_metric_rankings_raw
};
use crate::db::admin::write_audit_log;
use crate::utils::get_client_ip;
use crate::auth::generate_token;
use crate::auth::ApiUser;
use crate::state::AppState;

// JSON Payload Structs
#[derive(Deserialize)]
pub struct CreateUrlRequest {
    pub destination: String,
    pub code: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct UpdateUrlRequest {
    pub destination: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: String, // 'healthy', 'suspect', 'dead'
    pub tags: Option<Vec<String>>,
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
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
            return (StatusCode::BAD_REQUEST, Json(ApiError { error: "Short code must be 6 hex characters".to_string() })).into_response();
        }
    }

    let tags = payload.tags.unwrap_or_default();
    let conn = state.content_db.lock().unwrap();
    match create_url(&conn, &code, &payload.destination, payload.title.as_deref(), payload.description.as_deref(), &tags) {
        Ok(url) => {
            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(&state.admin_db.lock().unwrap(), &user.0.username, "URL_CREATION", Some("url"), Some(&url.id), Some(&ip), user_agent);
            (StatusCode::CREATED, Json(url)).into_response()
        }
        Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == rusqlite::ErrorCode::ConstraintViolation => {
            (StatusCode::CONFLICT, Json(ApiError { error: "Short code already exists".to_string() })).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response(),
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
    _user: ApiUser,
    Query(query): Query<ListQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    let conn = state.content_db.lock().unwrap();
    match list_urls(&conn, limit, offset, query.tag.as_deref()) {
        Ok(urls) => Json(urls).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

// GET /api/v1/urls/:uuid
pub async fn api_get_url(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    let conn = state.content_db.lock().unwrap();
    match get_url_by_id(&conn, &uuid) {
        Ok(Some(url)) => Json(url).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(ApiError { error: "URL not found".to_string() })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response(),
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
    let conn = state.content_db.lock().unwrap();
    match update_url(&conn, &uuid, &payload.destination, payload.title.as_deref(), payload.description.as_deref(), &payload.status, &tags) {
        Ok(Some(url)) => {
            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(&state.admin_db.lock().unwrap(), &user.0.username, "URL_UPDATE", Some("url"), Some(&uuid), Some(&ip), user_agent);
            Json(url).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(ApiError { error: "URL not found".to_string() })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response(),
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
    let conn = state.content_db.lock().unwrap();
    match delete_url(&conn, &uuid) {
        Ok(true) => {
            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(&state.admin_db.lock().unwrap(), &user.0.username, "URL_DELETION", Some("url"), Some(&uuid), Some(&ip), user_agent);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, Json(ApiError { error: "URL not found".to_string() })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response(),
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
        if code.len() != 4 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
            return (StatusCode::BAD_REQUEST, Json(ApiError { error: "Short code must be 4 hex characters".to_string() })).into_response();
        }
    }

    let conn = state.content_db.lock().unwrap();
    match create_landing_page(&conn, &code, &payload.slug, &payload.title, &payload.html_content, &payload.state) {
        Ok(page) => {
            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(&state.admin_db.lock().unwrap(), &user.0.username, "PAGE_CREATION", Some("page"), Some(&page.id), Some(&ip), user_agent);
            (StatusCode::CREATED, Json(page)).into_response()
        }
        Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == rusqlite::ErrorCode::ConstraintViolation => {
            (StatusCode::CONFLICT, Json(ApiError { error: "Short code already exists".to_string() })).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

// GET /api/v1/pages
pub async fn api_list_pages(
    State(state): State<AppState>,
    _user: ApiUser,
    Query(query): Query<ListQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);

    let conn = state.content_db.lock().unwrap();
    match list_landing_pages(&conn, limit, offset) {
        Ok(pages) => Json(pages).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response(),
    }
}

// GET /api/v1/pages/:uuid
pub async fn api_get_page(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    let conn = state.content_db.lock().unwrap();
    match get_landing_page_by_id(&conn, &uuid) {
        Ok(Some(page)) => Json(page).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(ApiError { error: "Page not found".to_string() })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response(),
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
    let conn = state.content_db.lock().unwrap();
    match update_landing_page(&conn, &uuid, &payload.slug, &payload.title, &payload.html_content, &payload.state) {
        Ok(Some(page)) => {
            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(&state.admin_db.lock().unwrap(), &user.0.username, "PAGE_UPDATE", Some("page"), Some(&uuid), Some(&ip), user_agent);
            Json(page).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(ApiError { error: "Page not found".to_string() })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response(),
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
    let conn = state.content_db.lock().unwrap();
    match delete_landing_page(&conn, &uuid) {
        Ok(true) => {
            let ip = get_client_ip(&headers, connect_info);
            let user_agent = headers.get("user-agent").and_then(|h| h.to_str().ok());
            let _ = write_audit_log(&state.admin_db.lock().unwrap(), &user.0.username, "PAGE_DELETION", Some("page"), Some(&uuid), Some(&ip), user_agent);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, Json(ApiError { error: "Page not found".to_string() })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiError { error: e.to_string() })).into_response(),
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
pub async fn api_overall_stats(
    State(state): State<AppState>,
    _user: ApiUser,
) -> Response {
    let (total_urls, active_links, dead_links) = {
        let conn = state.content_db.lock().unwrap();
        get_url_counts(&conn).unwrap_or((0, 0, 0))
    };
    let total_pages = {
        let conn = state.content_db.lock().unwrap();
        get_landing_page_count(&conn).unwrap_or(0)
    };
    let (total_clicks, total_page_views) = {
        let conn = state.analytics_db.lock().unwrap();
        (
            get_total_clicks(&conn).unwrap_or(0),
            get_total_page_views(&conn).unwrap_or(0)
        )
    };

    Json(OverallStatsResponse {
        total_urls,
        total_pages,
        total_clicks,
        total_page_views,
        active_links,
        dead_links,
    }).into_response()
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
    _user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    // Verify URL exists
    {
        let conn = state.content_db.lock().unwrap();
        if get_url_by_id(&conn, &uuid).unwrap_or(None).is_none() {
            return (StatusCode::NOT_FOUND, Json(ApiError { error: "URL not found".to_string() })).into_response();
        }
    }

    let conn = state.analytics_db.lock().unwrap();
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
    }).into_response()
}

// GET /api/v1/stats/page/:uuid
pub async fn api_page_stats(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(uuid): Path<String>,
) -> Response {
    // Verify Page exists
    {
        let conn = state.content_db.lock().unwrap();
        if get_landing_page_by_id(&conn, &uuid).unwrap_or(None).is_none() {
            return (StatusCode::NOT_FOUND, Json(ApiError { error: "Page not found".to_string() })).into_response();
        }
    }

    let conn = state.analytics_db.lock().unwrap();
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
    }).into_response()
}

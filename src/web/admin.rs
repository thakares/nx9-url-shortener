use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use chrono::Utc;
use flate2::write::GzEncoder;
use flate2::Compression;
use rusqlite::params;
use serde::Deserialize;
use std::net::SocketAddr;
use tar::Builder;

use crate::db::admin::{
    create_api_key, create_session, create_user, delete_api_key, delete_session, get_config,
    get_user_by_username, get_user_count, list_api_keys, set_config,
    write_audit_log as write_audit_log_legacy,
};

#[allow(clippy::too_many_arguments)]
fn write_audit_log(
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

use crate::auth::{
    authenticate_session, generate_csrf_token, generate_token, hash_password, verify_csrf,
    verify_password, verify_sha256,
};
use crate::charts::{generate_bar_chart, generate_line_chart};
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

const PAGE_SIZE: usize = 25;
const ANALYTICS_PAGE_SIZE: usize = 50;
const MAX_JSON_EXPORT_ROWS: usize = 50_000;

// Helper: Verify session and return user or redirect to login
async fn require_auth(state: &AppState, jar: &CookieJar) -> Result<(User, String), Redirect> {
    let conn = state.admin_db.lock().unwrap();
    match authenticate_session(&conn, jar) {
        Ok(Some((user, session_id))) => Ok((user, session_id)),
        _ => Err(Redirect::to("/admin/login")),
    }
}

// GET /admin
pub async fn admin_index(State(state): State<AppState>, jar: CookieJar) -> Response {
    match require_auth(&state, &jar).await {
        Ok(_) => Redirect::to("/admin/dashboard").into_response(),
        Err(redir) => redir.into_response(),
    }
}

// GET /admin/login
pub async fn login_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let error = params.get("error").cloned();
    let csrf_token = generate_token(16);

    let mut new_jar = jar.clone();
    new_jar = new_jar.add(
        Cookie::build(("bzod_temp_csrf", csrf_token.clone()))
            .path("/admin/login")
            .secure(state.config.cookie_secure)
            .http_only(true)
            .same_site(axum_extra::extract::cookie::SameSite::Strict)
            .build(),
    );

    let template = crate::templates::LoginTemplate { error, csrf_token };
    (new_jar, template).into_response()
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
    pub csrf_token: String,
}

// POST /admin/login
pub async fn login_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<LoginForm>,
) -> Response {
    let temp_csrf = jar
        .get("bzod_temp_csrf")
        .map(|c| c.value().to_string())
        .unwrap_or_default();
    if temp_csrf.is_empty() || temp_csrf != form.csrf_token {
        return Redirect::to("/admin/login?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let user_count = {
        let conn = state.admin_db.lock().unwrap();
        get_user_count(&conn).unwrap_or(0)
    };

    let user_opt = if user_count == 0 {
        // Bootstrap Phase using BOOTSTRAP_PASSWORD_SHA256
        if form.username == state.config.admin_username
            && verify_sha256(&form.password, &state.config.bootstrap_password_sha256)
        {
            let hash = match hash_password(&form.password) {
                Ok(h) => h,
                Err(_) => {
                    return Redirect::to("/admin/login?error=Internal hashing error")
                        .into_response()
                }
            };

            let conn = state.admin_db.lock().unwrap();
            match create_user(&conn, &form.username, &hash) {
                Ok(u) => {
                    let _ = write_audit_log(
                        &conn,
                        &state,
                        &u.username,
                        "BOOTSTRAP_USER_PROVISIONED",
                        Some("user"),
                        Some(&u.id),
                        Some(&ip),
                        headers.get("user-agent").and_then(|h| h.to_str().ok()),
                    );
                    Some(u)
                }
                Err(_) => None,
            }
        } else {
            None
        }
    } else {
        // Standard DB Verification
        let conn = state.admin_db.lock().unwrap();
        match get_user_by_username(&conn, &form.username) {
            Ok(Some(u)) => {
                if verify_password(&form.password, &u.password_hash) {
                    Some(u)
                } else {
                    None
                }
            }
            _ => None,
        }
    };

    match user_opt {
        Some(user) => {
            let session_token = generate_token(32);
            let expires = (Utc::now() + chrono::Duration::days(30)).to_rfc3339();

            {
                let conn = state.admin_db.lock().unwrap();
                let _ = create_session(&conn, &session_token, &user.id, &expires);
                let _ = write_audit_log(
                    &conn,
                    &state,
                    &user.username,
                    "USER_LOGIN",
                    Some("session"),
                    Some(&session_token),
                    Some(&ip),
                    headers.get("user-agent").and_then(|h| h.to_str().ok()),
                );
            }

            let cookie = Cookie::build(("bzod_session", session_token))
                .path("/")
                .secure(state.config.cookie_secure)
                .http_only(true)
                .same_site(axum_extra::extract::cookie::SameSite::Strict)
                .max_age(time::Duration::days(30))
                .build();

            let clear_temp = Cookie::build("bzod_temp_csrf")
                .path("/admin/login")
                .max_age(time::Duration::ZERO)
                .build();

            let mut response_jar = jar.clone();
            response_jar = response_jar.add(cookie).add(clear_temp);

            (response_jar, Redirect::to("/admin/dashboard")).into_response()
        }
        None => {
            {
                let conn = state.admin_db.lock().unwrap();
                let _ = write_audit_log(
                    &conn,
                    &state,
                    "anonymous",
                    "LOGIN_FAILED",
                    None,
                    None,
                    Some(&ip),
                    headers.get("user-agent").and_then(|h| h.to_str().ok()),
                );
            }
            Redirect::to("/admin/login?error=Invalid username or password").into_response()
        }
    }
}

// GET /admin/logout
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Ok((_, session_id)) = require_auth(&state, &jar).await {
        let conn = state.admin_db.lock().unwrap();
        let _ = delete_session(&conn, &session_id);
    }

    let cookie = Cookie::build("bzod_session")
        .path("/")
        .max_age(time::Duration::ZERO)
        .build();

    let mut response_jar = jar.clone();
    response_jar = response_jar.add(cookie);

    (response_jar, Redirect::to("/admin/login")).into_response()
}

// GET /admin/dashboard
pub async fn dashboard_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let (total_urls, active_links, dead_links) = {
        let conn = state.content_db.lock().unwrap();
        get_url_counts(&conn).unwrap_or((0, 0, 0))
    };

    let total_pages = {
        let conn = state.content_db.lock().unwrap();
        get_landing_page_count(&conn).unwrap_or(0)
    };

    let total_clicks = {
        let conn = state.analytics_db.lock().unwrap();
        get_total_clicks(&conn).unwrap_or(0)
    };

    let clicks_data = {
        let conn = state.analytics_db.lock().unwrap();
        get_clicks_trend(&conn, "url", "all", 30)
            .or_else(|_| get_clicks_trend_raw(&conn, "url", "all", 30))
            .unwrap_or_default()
    };

    let mut trend_map = std::collections::BTreeMap::new();
    for i in (0..30).rev() {
        let date_str = (Utc::now() - chrono::Duration::days(i))
            .format("%Y-%m-%d")
            .to_string();
        trend_map.insert(date_str, 0i64);
    }
    for (d, c) in clicks_data {
        trend_map.insert(d, c);
    }
    let formatted_trend: Vec<(String, i64)> = trend_map.into_iter().collect();
    let traffic_chart = generate_line_chart(&formatted_trend);

    let countries_data = {
        let conn = state.analytics_db.lock().unwrap();
        get_metric_rankings(&conn, "url", "all", "country", 5)
            .or_else(|_| get_metric_rankings_raw(&conn, "url", "all", "country", 5))
            .unwrap_or_default()
    };
    let countries_chart = generate_bar_chart(&countries_data);

    let referrers_data = {
        let conn = state.analytics_db.lock().unwrap();
        get_metric_rankings(&conn, "url", "all", "referrer", 5)
            .or_else(|_| get_metric_rankings_raw(&conn, "url", "all", "referrer", 5))
            .unwrap_or_default()
    };
    let referrers_chart = generate_bar_chart(&referrers_data);

    let browsers_data = {
        let conn = state.analytics_db.lock().unwrap();
        get_metric_rankings(&conn, "url", "all", "browser", 5)
            .or_else(|_| get_metric_rankings_raw(&conn, "url", "all", "browser", 5))
            .unwrap_or_default()
    };
    let browsers_chart = generate_bar_chart(&browsers_data);

    let template = crate::templates::DashboardTemplate {
        admin_username: user.username,
        total_urls,
        total_pages,
        total_clicks,
        active_links,
        dead_links,
        traffic_chart,
        countries_chart,
        browsers_chart,
        referrers_chart,
    };

    template.into_response()
}

// GET /admin/urls
#[derive(Deserialize)]
pub struct UrlsQuery {
    pub tag: Option<String>,
    pub error: Option<String>,
    pub page: Option<usize>,
}

pub async fn urls_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<UrlsQuery>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let (urls, total_pages, page, visible_pages) = {
        let conn = state.content_db.lock().unwrap();
        let total_records = if let Some(tag_str) = query.tag.as_deref() {
            get_url_count_by_tag(&conn, tag_str).unwrap_or(0)
        } else {
            get_url_counts(&conn).map(|(t, _, _)| t).unwrap_or(0)
        };
        let calculated_total_pages = (total_records as usize).div_ceil(PAGE_SIZE);
        let total_pages = std::cmp::max(1, calculated_total_pages);
        let requested_page = query.page.unwrap_or(1);
        let current_page = if requested_page == 0 {
            1
        } else {
            requested_page
        }
        .clamp(1, total_pages);
        let offset = (current_page - 1) * PAGE_SIZE;

        let urls = list_urls(&conn, PAGE_SIZE as i64, offset as i64, query.tag.as_deref())
            .unwrap_or_default();

        let start_page = current_page.saturating_sub(3).max(1);
        let end_page = std::cmp::min(total_pages, current_page + 3);
        let visible_pages: Vec<usize> = (start_page..=end_page).collect();

        (urls, total_pages, current_page, visible_pages)
    };

    let csrf_token = generate_csrf_token(&session_id);

    let proto = if state.config.cookie_secure {
        "https"
    } else {
        "http"
    };
    let base_url = state
        .config
        .base_url
        .clone()
        .unwrap_or_else(|| format!("{}://localhost:{}", proto, state.config.port));

    let template = crate::templates::UrlsTemplate {
        admin_username: user.username,
        urls,
        csrf_token,
        error: query.error,
        tag_filter: query.tag,
        base_url,
        current_page: page,
        total_pages,
        visible_pages,
    };

    template.into_response()
}

#[derive(Deserialize)]
pub struct CreateUrlForm {
    pub destination: String,
    pub code: String,
    pub custom_slug: String,
    pub title: String,
    pub description: String,
    pub tags: String,
    pub csrf_token: String,
    pub expires_at: String,
    pub password: String,
    pub max_access_count: String,
    pub utm_source: String,
    pub utm_medium: String,
    pub utm_campaign: String,
}

// POST /admin/urls/create
pub async fn urls_create(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<CreateUrlForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/urls?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    // Custom Slug takes priority if provided
    let mut code = form.custom_slug.trim().to_lowercase();
    if code.is_empty() {
        code = form.code.trim().to_lowercase();
        if code.is_empty() {
            code = generate_token(3);
        } else {
            if code.len() != 6 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
                return Redirect::to(
                    "/admin/urls?error=Custom code must be exactly 6 hex characters",
                )
                .into_response();
            }
        }
    } else {
        if !crate::utils::validation::validate_custom_slug(&code) {
            return Redirect::to("/admin/urls?error=Custom slug must start with ! followed by 1-24 characters of a-z, 0-9, -, _")
                .into_response();
        }
    }

    let mut dest = form.destination.trim().to_string();
    if let Ok(mut parsed) = reqwest::Url::parse(&dest) {
        let mut has_utm = false;
        {
            let mut query = parsed.query_pairs_mut();
            if !form.utm_source.trim().is_empty() {
                query.append_pair("utm_source", form.utm_source.trim());
                has_utm = true;
            }
            if !form.utm_medium.trim().is_empty() {
                query.append_pair("utm_medium", form.utm_medium.trim());
                has_utm = true;
            }
            if !form.utm_campaign.trim().is_empty() {
                query.append_pair("utm_campaign", form.utm_campaign.trim());
                has_utm = true;
            }
        }
        if has_utm {
            dest = parsed.to_string();
        }
    }

    let expires_at_opt = if form.expires_at.trim().is_empty() {
        None
    } else {
        let mut rfc = form.expires_at.trim().to_string();
        if rfc.len() == 16 {
            rfc.push_str(":00Z"); // convert HTML datetime-local to standard UTC RFC3339
        }
        Some(rfc)
    };

    let password_hash_opt = if form.password.trim().is_empty() {
        None
    } else {
        match hash_password(&form.password) {
            Ok(h) => Some(h),
            Err(_) => return Redirect::to("/admin/urls?error=Hashing error").into_response(),
        }
    };

    let max_access_count_opt = if form.max_access_count.trim().is_empty() {
        None
    } else {
        match form.max_access_count.trim().parse::<i64>() {
            Ok(c) => Some(c),
            Err(_) => {
                return Redirect::to("/admin/urls?error=Invalid max access count").into_response()
            }
        }
    };

    let tags_list: Vec<String> = form
        .tags
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    let title_opt = if form.title.trim().is_empty() {
        None
    } else {
        Some(form.title.trim())
    };
    let desc_opt = if form.description.trim().is_empty() {
        None
    } else {
        Some(form.description.trim())
    };

    let res = {
        let conn = state.content_db.lock().unwrap();
        crate::db::content::create_url_extended(
            &conn,
            &code,
            &dest,
            title_opt,
            desc_opt,
            &tags_list,
            expires_at_opt.as_deref(),
            password_hash_opt.as_deref(),
            max_access_count_opt,
        )
    };

    match res {
        Ok(url) => {
            {
                let conn = state.admin_db.lock().unwrap();
                let _ = write_audit_log(
                    &conn,
                    &state,
                    &user.username,
                    "URL_CREATION",
                    Some("url"),
                    Some(&url.id),
                    Some(&ip),
                    headers.get("user-agent").and_then(|h| h.to_str().ok()),
                );
            }
            Redirect::to("/admin/urls").into_response()
        }
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Redirect::to("/admin/urls?error=Short code/slug already exists").into_response()
        }
        Err(e) => Redirect::to(&format!("/admin/urls?error=Database error: {}", e)).into_response(),
    }
}

// POST /admin/urls/delete/:id
pub async fn urls_delete(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Path(id): Path<String>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let csrf_token = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &csrf_token) {
        return Redirect::to("/admin/urls?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let conn = state.content_db.lock().unwrap();
    match delete_url(&conn, &id) {
        Ok(_) => {
            {
                let conn_admin = state.admin_db.lock().unwrap();
                let _ = write_audit_log(
                    &conn_admin,
                    &state,
                    &user.username,
                    "URL_DELETION",
                    Some("url"),
                    Some(&id),
                    Some(&ip),
                    headers.get("user-agent").and_then(|h| h.to_str().ok()),
                );
            }
            Redirect::to("/admin/urls").into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/admin/urls?error=Failed to delete link: {}", e)).into_response()
        }
    }
}

// GET /admin/pages
#[derive(Deserialize)]
pub struct PagesQuery {
    pub error: Option<String>,
    pub page: Option<usize>,
}

pub async fn pages_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<PagesQuery>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let (pages, total_pages, page, visible_pages) = {
        let conn = state.content_db.lock().unwrap();
        let total_records = get_landing_page_count(&conn).unwrap_or(0);
        let calculated_total_pages = (total_records as usize).div_ceil(PAGE_SIZE);
        let total_pages = std::cmp::max(1, calculated_total_pages);
        let requested_page = query.page.unwrap_or(1);
        let current_page = if requested_page == 0 {
            1
        } else {
            requested_page
        }
        .clamp(1, total_pages);
        let offset = (current_page - 1) * PAGE_SIZE;

        let pages = list_landing_pages(&conn, PAGE_SIZE as i64, offset as i64).unwrap_or_default();

        let start_page = current_page.saturating_sub(3).max(1);
        let end_page = std::cmp::min(total_pages, current_page + 3);
        let visible_pages: Vec<usize> = (start_page..=end_page).collect();

        (pages, total_pages, current_page, visible_pages)
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::PagesTemplate {
        admin_username: user.username,
        pages,
        csrf_token,
        error: query.error,
        current_page: page,
        total_pages,
        visible_pages,
    };

    template.into_response()
}

#[derive(Deserialize)]
pub struct CreatePageForm {
    pub title: String,
    pub slug: String,
    pub code: String,
    pub custom_slug: String,
    pub state: String,
    pub html_content: String,
    pub csrf_token: String,
}

// POST /admin/pages/create
pub async fn pages_create(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<CreatePageForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/pages?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    // Custom Slug takes priority if provided
    let mut code = form.custom_slug.trim().to_lowercase();
    if code.is_empty() {
        code = form.code.trim().to_lowercase();
        if code.is_empty() {
            code = generate_token(2);
        } else {
            if code.len() != 4 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
                return Redirect::to(
                    "/admin/pages?error=Custom code must be exactly 4 hex characters",
                )
                .into_response();
            }
        }
    } else {
        if !crate::utils::validation::validate_custom_slug(&code) {
            return Redirect::to("/admin/pages?error=Custom slug must start with ! followed by 1-24 characters of a-z, 0-9, -, _")
                .into_response();
        }
    }

    let clean_slug = form.slug.trim().to_lowercase();
    if clean_slug.is_empty() {
        return Redirect::to("/admin/pages?error=Slug is required").into_response();
    }

    let res = {
        let conn = state.content_db.lock().unwrap();
        create_landing_page(
            &conn,
            &code,
            &clean_slug,
            &form.title,
            &form.html_content,
            &form.state,
        )
    };

    match res {
        Ok(page) => {
            {
                let conn_admin = state.admin_db.lock().unwrap();
                let _ = write_audit_log(
                    &conn_admin,
                    &state,
                    &user.username,
                    "PAGE_CREATION",
                    Some("page"),
                    Some(&page.id),
                    Some(&ip),
                    headers.get("user-agent").and_then(|h| h.to_str().ok()),
                );
            }
            Redirect::to("/admin/pages").into_response()
        }
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Redirect::to("/admin/pages?error=Short code already exists").into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/admin/pages?error=Database error: {}", e)).into_response()
        }
    }
}

// POST /admin/pages/delete/:id
pub async fn pages_delete(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Path(id): Path<String>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let csrf_token = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &csrf_token) {
        return Redirect::to("/admin/pages?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let conn = state.content_db.lock().unwrap();
    match delete_landing_page(&conn, &id) {
        Ok(_) => {
            {
                let conn_admin = state.admin_db.lock().unwrap();
                let _ = write_audit_log(
                    &conn_admin,
                    &state,
                    &user.username,
                    "PAGE_DELETION",
                    Some("page"),
                    Some(&id),
                    Some(&ip),
                    headers.get("user-agent").and_then(|h| h.to_str().ok()),
                );
            }
            Redirect::to("/admin/pages").into_response()
        }
        Err(e) => Redirect::to(&format!("/admin/pages?error=Failed to delete page: {}", e))
            .into_response(),
    }
}

// GET /admin/settings
#[derive(Deserialize)]
pub struct SettingsQuery {
    pub success: Option<String>,
    pub error: Option<String>,
}

pub async fn settings_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<SettingsQuery>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let api_keys = {
        let conn = state.admin_db.lock().unwrap();
        list_api_keys(&conn, &user.id).unwrap_or_default()
    };

    let data_retention = {
        let conn = state.admin_db.lock().unwrap();
        get_config(&conn, "retention_days")
            .unwrap_or(None)
            .unwrap_or_else(|| {
                state
                    .config
                    .data_retention_days
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "unlimited".to_string())
            })
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::SettingsTemplate {
        admin_username: user.username,
        api_keys,
        data_retention,
        csrf_token,
        success: query.success,
        error: query.error,
    };

    template.into_response()
}

#[derive(Deserialize)]
pub struct ChangePasswordForm {
    pub current_password: String,
    pub new_password: String,
    pub csrf_token: String,
}

// POST /admin/settings/password
pub async fn change_password_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<ChangePasswordForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/settings?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let conn = state.admin_db.lock().unwrap();
    if !verify_password(&form.current_password, &user.password_hash) {
        let _ = write_audit_log(
            &conn,
            &state,
            &user.username,
            "PASSWORD_CHANGE_FAIL",
            Some("user"),
            Some(&user.id),
            Some(&ip),
            headers.get("user-agent").and_then(|h| h.to_str().ok()),
        );
        return Redirect::to("/admin/settings?error=Incorrect current password").into_response();
    }

    let new_hash = match hash_password(&form.new_password) {
        Ok(h) => h,
        Err(_) => return Redirect::to("/admin/settings?error=Hashing error").into_response(),
    };

    let res = conn.execute(
        "UPDATE users SET password_hash = ?1 WHERE id = ?2;",
        params![new_hash, user.id],
    );
    match res {
        Ok(_) => {
            let _ = write_audit_log(
                &conn,
                &state,
                &user.username,
                "PASSWORD_CHANGE_SUCCESS",
                Some("user"),
                Some(&user.id),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/admin/settings?success=Password updated successfully").into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/admin/settings?error=Failed to update password: {}",
            e
        ))
        .into_response(),
    }
}

#[derive(Deserialize)]
pub struct RetentionForm {
    pub retention: String,
    pub csrf_token: String,
}

// POST /admin/settings/retention
pub async fn change_retention_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<RetentionForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/settings?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let conn = state.admin_db.lock().unwrap();
    match set_config(&conn, "retention_days", &form.retention) {
        Ok(_) => {
            let _ = write_audit_log(
                &conn,
                &state,
                &user.username,
                "RETENTION_POLICY_CHANGED",
                Some("config"),
                Some("retention_days"),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/admin/settings?success=Retention policy saved").into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/admin/settings?error=Database error: {}", e)).into_response()
        }
    }
}

// POST /admin/settings/compact
pub async fn compact_db_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Response {
    let (user, _session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let ip = get_client_ip(&headers, connect_info);

    match state.db_compact() {
        Ok(_) => {
            let conn = state.admin_db.lock().unwrap();
            let _ = write_audit_log(
                &conn,
                &state,
                &user.username,
                "DATABASE_COMPACTION",
                Some("system"),
                Some("all_dbs"),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/admin/settings?success=Database files compacted successfully")
                .into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/admin/settings?error=Failed to compact: {}", e)).into_response()
        }
    }
}

// GET /admin/settings/backup
pub async fn download_backup(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let ip = get_client_ip(&headers, connect_info);

    // Create tar.gz in memory
    let mut buffer = Vec::new();
    let res = {
        let enc = GzEncoder::new(&mut buffer, Compression::default());
        let mut tar = Builder::new(enc);

        let files = vec!["admin.db", "content.db", "analytics.db", "system.db"];
        let mut add_err = None;
        for f in files {
            let path = state.config.data_dir.join(f);
            if path.exists() {
                if let Err(e) = tar.append_path_with_name(&path, f) {
                    add_err = Some(e);
                    break;
                }
            }
        }

        match add_err {
            Some(e) => Err(e),
            None => match tar.into_inner().and_then(|encoder| encoder.finish()) {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            },
        }
    };

    match res {
        Ok(_) => {
            {
                let conn = state.admin_db.lock().unwrap();
                let _ = write_audit_log(
                    &conn,
                    &state,
                    &user.username,
                    "DATABASE_BACKUP",
                    Some("system"),
                    Some("tarball"),
                    Some(&ip),
                    headers.get("user-agent").and_then(|h| h.to_str().ok()),
                );
            }

            let date_str = Utc::now().format("%Y-%m-%d").to_string();
            let filename = format!("{}-bzod-backup.tar.gz", date_str);

            (
                StatusCode::OK,
                [
                    ("Content-Type", "application/gzip"),
                    (
                        "Content-Disposition",
                        &format!("attachment; filename=\"{}\"", filename),
                    ),
                ],
                buffer,
            )
                .into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/admin/settings?error=Backup failed: {}", e)).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct CreateApiKeyForm {
    pub key_name: String,
    pub csrf_token: String,
}

// POST /admin/settings/api-keys/create
pub async fn create_api_key_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<CreateApiKeyForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/settings?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);
    let key_secret = format!("bzo_{}", generate_token(16));

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(key_secret.as_bytes());
    let hashed_key = hex::encode(hasher.finalize());

    let conn = state.admin_db.lock().unwrap();
    match create_api_key(&conn, &user.id, &form.key_name, &hashed_key) {
        Ok(api_key) => {
            let _ = write_audit_log(
                &conn,
                &state,
                &user.username,
                "API_KEY_CREATED",
                Some("api_key"),
                Some(&api_key.id),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to(&format!(
                "/admin/settings?success=Token generated successfully. **IMPORTANT: Copy your token now, it will never be shown again!** Token value: {}",
                key_secret
            )).into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/admin/settings?error=Database error: {}", e)).into_response()
        }
    }
}

// POST /admin/settings/api-keys/revoke/:id
pub async fn revoke_api_key_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Path(id): Path<String>,
    Form(form): Form<std::collections::HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let csrf_token = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &csrf_token) {
        return Redirect::to("/admin/settings?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let conn = state.admin_db.lock().unwrap();
    match delete_api_key(&conn, &id) {
        Ok(_) => {
            let _ = write_audit_log(
                &conn,
                &state,
                &user.username,
                "API_KEY_REVOKED",
                Some("api_key"),
                Some(&id),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/admin/settings?success=API Token revoked").into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/admin/settings?error=Failed to revoke key: {}",
            e
        ))
        .into_response(),
    }
}

#[derive(Deserialize)]
pub struct BulkQrExportForm {
    pub format: String,
    pub csrf_token: String,
}

// POST /admin/settings/bulk-qr
pub async fn bulk_qr_export_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<BulkQrExportForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/settings?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let urls = {
        let conn = state.content_db.lock().unwrap();
        crate::db::content::list_urls(&conn, 500, 0, None).unwrap_or_default()
    };

    if urls.is_empty() {
        return Redirect::to("/admin/settings?error=No shortened URLs found to export")
            .into_response();
    }

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

    match crate::services::bulk::export_qr_zip(&urls, &form.format, &base_url) {
        Ok(zip_data) => {
            // Write Audit Log
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                &state,
                &user.username,
                "BULK_QR_EXPORT",
                Some("bulk"),
                Some("qr"),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );

            Response::builder()
                .header("content-type", "application/zip")
                .header(
                    "content-disposition",
                    "attachment; filename=\"qr_codes.zip\"",
                )
                .body(axum::body::Body::from(zip_data))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => {
            Redirect::to(&format!("/admin/settings?error=Export failed: {}", e)).into_response()
        }
    }
}

// GET /admin/audit
pub async fn audit_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let logs = {
        let conn = state.system_db.lock().unwrap();
        let events = crate::db::audit_events::list_audit_events(&conn, 100, 0, None, None)
            .unwrap_or_default();
        events
            .into_iter()
            .map(|e| {
                let (ip, ua) = if let Some(ref m) = e.metadata {
                    if m.starts_with("IP: ") {
                        let parts: Vec<&str> = m.split(", UA: ").collect();
                        let ip = parts[0]
                            .trim_start_matches("IP: ")
                            .trim_matches('"')
                            .trim_matches('\'')
                            .replace("Some(", "")
                            .replace(")", "");
                        let ua = if parts.len() > 1 {
                            parts[1]
                                .trim_matches('"')
                                .trim_matches('\'')
                                .replace("Some(", "")
                                .replace(")", "")
                        } else {
                            "Unknown".to_string()
                        };
                        (Some(ip), Some(ua))
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };

                crate::models::AuditLog {
                    id: e.id,
                    timestamp: e.timestamp,
                    username: e.actor,
                    action: e.action,
                    object_type: Some(e.object_type),
                    object_id: Some(e.object_id),
                    ip_address: ip,
                    user_agent: ua,
                }
            })
            .collect()
    };

    let template = crate::templates::AuditTemplate {
        admin_username: user.username,
        logs,
    };

    template.into_response()
}

// GET /admin/status
pub async fn status_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let app_status = "Healthy";

    let db_status = {
        let conn_ok = {
            let conn = state.admin_db.lock().unwrap();
            get_user_count(&conn).is_ok()
        };
        if conn_ok {
            format!(
                "Operational\n\nDatabase Files:\n{}",
                get_db_file_info(&state.config.data_dir)
            )
        } else {
            "Degraded (Database connections failed)".to_string()
        }
    };

    let queue_size = 0;
    let memory_usage = get_memory_usage();

    let uptime_duration = state.start_time.elapsed();
    let uptime = crate::utils::format_duration(uptime_duration);

    let urls = {
        let conn = state.content_db.lock().unwrap();
        crate::db::content::list_urls(&conn, 50, 0, None).unwrap_or_default()
    };

    let template = crate::templates::StatusTemplate {
        admin_username: user.username,
        app_status,
        db_status,
        queue_size,
        memory_usage,
        uptime,
        version: "0.1.0",
        git_commit: "unknown",
        urls,
    };

    template.into_response()
}

// POST /admin/settings/restore
pub async fn restore_backup_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let ip = get_client_ip(&headers, connect_info);
    let mut file_bytes = Vec::new();
    let mut confirm_text = String::new();
    let mut csrf_token = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "backup_file" {
            if let Ok(bytes) = field.bytes().await {
                file_bytes = bytes.to_vec();
            }
        } else if name == "confirm_text" {
            if let Ok(text) = field.text().await {
                confirm_text = text.trim().to_string();
            }
        } else if name == "csrf_token" {
            if let Ok(token) = field.text().await {
                csrf_token = token.trim().to_string();
            }
        }
    }

    if !verify_csrf(&session_id, &csrf_token) {
        return Redirect::to("/admin/settings?error=Invalid CSRF token").into_response();
    }

    if confirm_text != "RESTORE" {
        return Redirect::to("/admin/settings?error=Confirmation text must be exactly 'RESTORE'")
            .into_response();
    }

    if file_bytes.is_empty() {
        return Redirect::to("/admin/settings?error=No backup file uploaded").into_response();
    }

    // Save uploaded archive to a temporary file
    let temp_file_path =
        std::env::temp_dir().join(format!("bzod_restore_{}.tar.gz", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::write(&temp_file_path, &file_bytes) {
        return Redirect::to(&format!(
            "/admin/settings?error=Failed to write temp file: {}",
            e
        ))
        .into_response();
    }

    // Log RESTORE_INITIATED audit event before restore
    {
        let conn = state.admin_db.lock().unwrap();
        let _ = write_audit_log(
            &conn,
            &state,
            &user.username,
            "RESTORE_INITIATED",
            Some("system"),
            Some("tarball"),
            Some(&ip),
            headers.get("user-agent").and_then(|h| h.to_str().ok()),
        );
    }

    // Call the perform_restore engine inside closed connection blocks
    let restore_res = {
        // Temporarily suspend access to active SQLite connections
        let mut admin_conn = state.admin_db.lock().unwrap();
        let mut content_conn = state.content_db.lock().unwrap();
        let mut analytics_conn = state.analytics_db.lock().unwrap();
        let mut system_conn = state.system_db.lock().unwrap();

        // 1. Close current connections by replacing them with dummy in-memory DBs
        *admin_conn = match rusqlite::Connection::open_in_memory() {
            Ok(c) => c,
            Err(e) => {
                return Redirect::to(&format!(
                    "/admin/settings?error=Failed to open temp in-memory DB: {}",
                    e
                ))
                .into_response()
            }
        };
        *content_conn = match rusqlite::Connection::open_in_memory() {
            Ok(c) => c,
            Err(e) => {
                return Redirect::to(&format!(
                    "/admin/settings?error=Failed to open temp in-memory DB: {}",
                    e
                ))
                .into_response()
            }
        };
        *analytics_conn = match rusqlite::Connection::open_in_memory() {
            Ok(c) => c,
            Err(e) => {
                return Redirect::to(&format!(
                    "/admin/settings?error=Failed to open temp in-memory DB: {}",
                    e
                ))
                .into_response()
            }
        };
        *system_conn = match rusqlite::Connection::open_in_memory() {
            Ok(c) => c,
            Err(e) => {
                return Redirect::to(&format!(
                    "/admin/settings?error=Failed to open temp in-memory DB: {}",
                    e
                ))
                .into_response()
            }
        };

        // 2. Perform restore unpacking/validation
        let res = crate::cli::restore::perform_restore(&temp_file_path, &state.config.data_dir);

        // 3. Reinitialize database connections
        let new_admin = rusqlite::Connection::open(state.config.data_dir.join("admin.db"));
        let new_content = rusqlite::Connection::open(state.config.data_dir.join("content.db"));
        let new_analytics = rusqlite::Connection::open(state.config.data_dir.join("analytics.db"));
        let new_system = rusqlite::Connection::open(state.config.data_dir.join("system.db"));

        match (new_admin, new_content, new_analytics, new_system) {
            (Ok(adm), Ok(cnt), Ok(any), Ok(sys)) => {
                let _ = crate::db::sqlite::enable_wal(&adm, "admin");
                let _ = crate::db::sqlite::enable_wal(&cnt, "content");
                let _ = crate::db::sqlite::enable_wal(&any, "analytics");
                let _ = crate::db::sqlite::enable_wal(&sys, "system");

                let _ = crate::db::sqlite::enable_foreign_keys(&adm, "admin");
                let _ = crate::db::sqlite::enable_foreign_keys(&cnt, "content");
                let _ = crate::db::sqlite::enable_foreign_keys(&any, "analytics");
                let _ = crate::db::sqlite::enable_foreign_keys(&sys, "system");

                *admin_conn = adm;
                *content_conn = cnt;
                *analytics_conn = any;
                *system_conn = sys;
            }
            _ => {
                return Redirect::to("/admin/settings?error=Failed to reopen restored databases")
                    .into_response();
            }
        }

        res
    };

    let _ = std::fs::remove_file(&temp_file_path);

    match restore_res {
        Ok(_) => {
            // Write database restore success log to newly restored admin db
            {
                let conn = state.admin_db.lock().unwrap();
                let _ = write_audit_log(
                    &conn,
                    &state,
                    &user.username,
                    "DATABASE_RESTORE",
                    Some("system"),
                    Some("tarball"),
                    Some(&ip),
                    headers.get("user-agent").and_then(|h| h.to_str().ok()),
                );
            }
            Redirect::to("/admin/login").into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/admin/settings?error=Restore failed: {}", e)).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct AnalyticsQuery {
    pub analytics_page: Option<usize>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

fn validate_date_filters(
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

fn escape_csv_field(field: &str) -> String {
    let needs_escaping =
        field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r');
    if needs_escaping {
        let escaped = field.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        field.to_string()
    }
}

struct DbExportStream {
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

async fn perform_csv_export(
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

async fn perform_json_export(
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

// GET /admin/analytics/url/:id
pub async fn url_analytics_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let url = {
        let conn = state.content_db.lock().unwrap();
        match get_url_by_id(&conn, &id) {
            Ok(Some(u)) => u,
            Ok(None) => return (StatusCode::NOT_FOUND, "URL not found").into_response(),
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        }
    };

    let conn = state.analytics_db.lock().unwrap();

    let schema_cols = get_visits_schema_columns(&conn).unwrap_or_default();
    let has_utm_source = schema_cols.contains("utm_source");
    let has_utm_campaign = schema_cols.contains("utm_campaign");
    if has_utm_source || has_utm_campaign {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "UTM mapping verification failed",
        )
            .into_response();
    }

    let (clean_date_from, clean_date_to) =
        match validate_date_filters(query.date_from.as_deref(), query.date_to.as_deref()) {
            Ok(res) => res,
            Err(status) => return status.into_response(),
        };

    let total_clicks = get_target_visit_total_filtered(
        &conn,
        "url",
        &id,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or(0);

    let unique_visitors = get_target_unique_visitors(&conn, "url", &id).unwrap_or(0);
    let qr_scans = crate::db::qr::get_qr_scan_count(&conn, &id).unwrap_or(0);
    let direct_clicks = (total_clicks - qr_scans).max(0);

    let clicks_data = get_clicks_trend(&conn, "url", &id, 30)
        .or_else(|_| get_clicks_trend_raw(&conn, "url", &id, 30))
        .unwrap_or_default();
    let mut trend_map = std::collections::BTreeMap::new();
    for i in (0..30).rev() {
        let date_str = (Utc::now() - chrono::Duration::days(i))
            .format("%Y-%m-%d")
            .to_string();
        trend_map.insert(date_str, 0i64);
    }
    for (d, c) in clicks_data {
        trend_map.insert(d, c);
    }
    let formatted_trend: Vec<(String, i64)> = trend_map.into_iter().collect();
    let traffic_chart = generate_line_chart(&formatted_trend);

    let monthly_data = get_monthly_clicks_trend(&conn, "url", &id, 12).unwrap_or_default();
    let monthly_chart = generate_line_chart(&monthly_data);

    let countries_data = get_metric_rankings(&conn, "url", &id, "country", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &id, "country", 5))
        .unwrap_or_default();
    let countries_chart = generate_bar_chart(&countries_data);

    let referrers_data = get_metric_rankings(&conn, "url", &id, "referrer", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &id, "referrer", 5))
        .unwrap_or_default();
    let referrers_chart = generate_bar_chart(&referrers_data);

    let browsers_data = get_metric_rankings(&conn, "url", &id, "browser", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "url", &id, "browser", 5))
        .unwrap_or_default();
    let browsers_chart = generate_bar_chart(&browsers_data);

    let calculated_total_pages = (total_clicks as usize).div_ceil(ANALYTICS_PAGE_SIZE);
    let total_pages = std::cmp::max(1, calculated_total_pages);
    let requested_page = query.analytics_page.unwrap_or(1);
    let current_page = if requested_page == 0 {
        1
    } else {
        requested_page
    }
    .clamp(1, total_pages);
    let offset = (current_page - 1) * ANALYTICS_PAGE_SIZE;

    let visits_raw = get_target_visits_paginated(
        &conn,
        "url",
        &id,
        ANALYTICS_PAGE_SIZE as i64,
        offset as i64,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or_default();

    let visits: Vec<crate::templates::VisitorLogEntry> = visits_raw
        .into_iter()
        .enumerate()
        .map(|(idx, r)| {
            let (browser, _, _) = parse_ua(&r.user_agent);
            let referrer = clean_referrer(&r.referer);
            let sr = offset + idx + 1;
            crate::templates::VisitorLogEntry {
                sr,
                timestamp: r.timestamp,
                ip_address: r.ip_address,
                country: if r.country.is_empty() {
                    "Unknown".to_string()
                } else {
                    r.country
                },
                referrer,
                browser,
                user_agent: r.user_agent,
                utm_source: "-".to_string(),
                utm_campaign: "-".to_string(),
            }
        })
        .collect();

    let start_page = current_page.saturating_sub(3).max(1);
    let end_page = std::cmp::min(total_pages, current_page + 3);
    let visible_pages: Vec<usize> = (start_page..=end_page).collect();

    let page_start = if total_clicks == 0 { 0 } else { offset + 1 };
    let page_end = if total_clicks == 0 {
        0
    } else {
        std::cmp::min(total_clicks as usize, offset + ANALYTICS_PAGE_SIZE)
    };

    let template = crate::templates::UrlAnalyticsTemplate {
        admin_username: user.username,
        url,
        total_clicks,
        unique_visitors,
        qr_scans,
        direct_clicks,
        traffic_chart,
        monthly_chart,
        countries_chart,
        referrers_chart,
        browsers_chart,
        visits,
        current_page,
        total_pages,
        visible_pages,
        total_records: total_clicks,
        page_start,
        page_end,
        date_from: clean_date_from,
        date_to: clean_date_to,
    };

    template.into_response()
}

// GET /admin/analytics/page/:id
pub async fn page_analytics_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    let (user, _) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let page = {
        let conn = state.content_db.lock().unwrap();
        match get_landing_page_by_id(&conn, &id) {
            Ok(Some(p)) => p,
            Ok(None) => return (StatusCode::NOT_FOUND, "Landing page not found").into_response(),
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
        }
    };

    let conn = state.analytics_db.lock().unwrap();

    let schema_cols = get_visits_schema_columns(&conn).unwrap_or_default();
    let has_utm_source = schema_cols.contains("utm_source");
    let has_utm_campaign = schema_cols.contains("utm_campaign");
    if has_utm_source || has_utm_campaign {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "UTM mapping verification failed",
        )
            .into_response();
    }

    let (clean_date_from, clean_date_to) =
        match validate_date_filters(query.date_from.as_deref(), query.date_to.as_deref()) {
            Ok(res) => res,
            Err(status) => return status.into_response(),
        };

    let total_views = get_target_visit_total_filtered(
        &conn,
        "page",
        &id,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or(0);

    let unique_visitors = get_target_unique_visitors(&conn, "page", &id).unwrap_or(0);

    let clicks_data = get_clicks_trend(&conn, "page", &id, 30)
        .or_else(|_| get_clicks_trend_raw(&conn, "page", &id, 30))
        .unwrap_or_default();
    let mut trend_map = std::collections::BTreeMap::new();
    for i in (0..30).rev() {
        let date_str = (Utc::now() - chrono::Duration::days(i))
            .format("%Y-%m-%d")
            .to_string();
        trend_map.insert(date_str, 0i64);
    }
    for (d, c) in clicks_data {
        trend_map.insert(d, c);
    }
    let formatted_trend: Vec<(String, i64)> = trend_map.into_iter().collect();
    let traffic_chart = generate_line_chart(&formatted_trend);

    let monthly_data = get_monthly_clicks_trend(&conn, "page", &id, 12).unwrap_or_default();
    let monthly_chart = generate_line_chart(&monthly_data);

    let countries_data = get_metric_rankings(&conn, "page", &id, "country", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "page", &id, "country", 5))
        .unwrap_or_default();
    let countries_chart = generate_bar_chart(&countries_data);

    let referrers_data = get_metric_rankings(&conn, "page", &id, "referrer", 5)
        .or_else(|_| get_metric_rankings_raw(&conn, "page", &id, "referrer", 5))
        .unwrap_or_default();
    let referrers_chart = generate_bar_chart(&referrers_data);

    let calculated_total_pages = (total_views as usize).div_ceil(ANALYTICS_PAGE_SIZE);
    let total_pages = std::cmp::max(1, calculated_total_pages);
    let requested_page = query.analytics_page.unwrap_or(1);
    let current_page = if requested_page == 0 {
        1
    } else {
        requested_page
    }
    .clamp(1, total_pages);
    let offset = (current_page - 1) * ANALYTICS_PAGE_SIZE;

    let visits_raw = get_target_visits_paginated(
        &conn,
        "page",
        &id,
        ANALYTICS_PAGE_SIZE as i64,
        offset as i64,
        clean_date_from.as_deref(),
        clean_date_to.as_deref(),
    )
    .unwrap_or_default();

    let visits: Vec<crate::templates::VisitorLogEntry> = visits_raw
        .into_iter()
        .enumerate()
        .map(|(idx, r)| {
            let (browser, _, _) = parse_ua(&r.user_agent);
            let referrer = clean_referrer(&r.referer);
            let sr = offset + idx + 1;
            crate::templates::VisitorLogEntry {
                sr,
                timestamp: r.timestamp,
                ip_address: r.ip_address,
                country: if r.country.is_empty() {
                    "Unknown".to_string()
                } else {
                    r.country
                },
                referrer,
                browser,
                user_agent: r.user_agent,
                utm_source: "-".to_string(),
                utm_campaign: "-".to_string(),
            }
        })
        .collect();

    let start_page = current_page.saturating_sub(3).max(1);
    let end_page = std::cmp::min(total_pages, current_page + 3);
    let visible_pages: Vec<usize> = (start_page..=end_page).collect();

    let page_start = if total_views == 0 { 0 } else { offset + 1 };
    let page_end = if total_views == 0 {
        0
    } else {
        std::cmp::min(total_views as usize, offset + ANALYTICS_PAGE_SIZE)
    };

    let template = crate::templates::PageAnalyticsTemplate {
        admin_username: user.username,
        page,
        total_views,
        unique_visitors,
        traffic_chart,
        monthly_chart,
        countries_chart,
        referrers_chart,
        visits,
        current_page,
        total_pages,
        visible_pages,
        total_records: total_views,
        page_start,
        page_end,
        date_from: clean_date_from,
        date_to: clean_date_to,
    };

    template.into_response()
}

pub async fn url_analytics_csv_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    if require_auth(&state, &jar).await.is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    perform_csv_export(state, "url", id, query.date_from, query.date_to).await
}

pub async fn url_analytics_json_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    if require_auth(&state, &jar).await.is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    perform_json_export(state, "url", id, query.date_from, query.date_to).await
}

pub async fn page_analytics_csv_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    if require_auth(&state, &jar).await.is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    perform_csv_export(state, "page", id, query.date_from, query.date_to).await
}

pub async fn page_analytics_json_export(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    if require_auth(&state, &jar).await.is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    perform_json_export(state, "page", id, query.date_from, query.date_to).await
}

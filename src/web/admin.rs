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

use crate::db::admin::{
    create_api_key, delete_api_key, get_config, get_user_count, list_api_keys, set_config,
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
    authenticate_admin_session, authenticate_user_session, generate_csrf_token, generate_token,
    hash_password, verify_csrf, verify_password, verify_sha256,
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

// Helper: Verify admin session and return user or redirect to login
async fn require_auth(state: &AppState, jar: &CookieJar) -> Result<(User, String), Redirect> {
    let conn = state.users_db.lock().unwrap();
    match authenticate_admin_session(&conn, jar) {
        Ok(Some((user, session_id))) => Ok((user, session_id)),
        _ => Err(Redirect::to("/admin/login")),
    }
}

// Helper: Verify tenant user session and return user or redirect to login
async fn require_user_auth(
    state: &AppState,
    jar: &CookieJar,
) -> Result<(crate::models::TenantUser, String), Redirect> {
    let conn = state.users_db.lock().unwrap();
    match authenticate_user_session(&conn, jar) {
        Ok(Some((user, session_id))) => Ok((user, session_id)),
        _ => Err(Redirect::to("/login")),
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
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let error = params.get("error").cloned();
    let csrf_token = generate_token(16);
    let cookie = Cookie::build(("bzod_temp_csrf", csrf_token.clone()))
        .path("/admin/login")
        .secure(state.config.cookie_secure)
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Strict)
        .max_age(time::Duration::minutes(10))
        .build();

    let new_jar = jar.add(cookie);
    let template = crate::templates::LoginTemplate {
        error,
        csrf_token,
        action: "/admin/login".to_string(),
        title: "Admin Login".to_string(),
        subtitle: "Administrative Access".to_string(),
        button_text: "Sign In".to_string(),
    };
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

    let (user_count, admin_count, active_session_count) = {
        let conn = state.users_db.lock().unwrap();
        let u_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM users;", [], |r| r.get(0))
            .unwrap_or(0);
        let a_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE account_type = 'admin';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let now = Utc::now().to_rfc3339();
        let s_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE expires_at > ?1;",
                [now],
                |r| r.get(0),
            )
            .unwrap_or(0);
        (u_count, a_count, s_count)
    };

    let bootstrap_allowed = user_count <= 1 && admin_count == 0 && active_session_count == 0;

    let user_opt = if bootstrap_allowed
        && form.username == state.config.admin_username
        && verify_sha256(&form.password, &state.config.bootstrap_password_sha256)
    {
        // Bootstrap Phase using BOOTSTRAP_PASSWORD_SHA256
        let hash = match hash_password(&form.password) {
            Ok(h) => h,
            Err(_) => {
                return Redirect::to("/admin/login?error=Internal hashing error").into_response()
            }
        };

        let conn = state.users_db.lock().unwrap();
        match crate::db::users::create_admin_user(&conn, &form.username, &hash) {
            Ok(u) => {
                // Initialize user specific directory and DB files
                if let Err(e) = state.db.init_user_databases(u.id) {
                    tracing::error!(
                        "Failed to init user databases during admin bootstrap: {:?}",
                        e
                    );
                }

                // Write audit event in system.db
                let system_conn = state.system_db.lock().unwrap();
                let metadata = format!(
                    "IP: {:?}, UA: {:?}",
                    ip,
                    headers.get("user-agent").and_then(|h| h.to_str().ok())
                );
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    &u.username,
                    "BOOTSTRAP_USER_PROVISIONED",
                    "user",
                    &u.id.to_string(),
                    Some(&metadata),
                );

                Some(User {
                    id: u.id.to_string(),
                    username: u.username,
                    password_hash: u.password_hash,
                    created_at: u.created_at,
                })
            }
            Err(e) => {
                tracing::error!("Failed to create admin user during bootstrap: {:?}", e);
                None
            }
        }
    } else {
        // Standard DB Verification
        let conn = state.users_db.lock().unwrap();
        let user_res: Result<Option<crate::models::TenantUser>, rusqlite::Error> = conn.query_row(
            "SELECT id, username, password_hash, status, created_at, last_login, account_type, organization_id, metadata 
             FROM users WHERE username = ?1;",
            [&form.username],
            |row| {
                Ok(crate::models::TenantUser {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                    last_login: row.get(5)?,
                    account_type: row.get(6)?,
                    organization_id: row.get(7)?,
                    metadata: row.get(8)?,
                })
            }
        ).optional();

        match user_res {
            Ok(Some(u)) => {
                if u.status != "active" {
                    tracing::warn!(
                        username = form.username,
                        reason = "account_disabled",
                        "login rejected"
                    );
                    None
                } else if u.account_type != "admin" {
                    tracing::warn!(
                        username = form.username,
                        reason = "insufficient_privileges",
                        "login rejected"
                    );
                    None
                } else if verify_password(&form.password, &u.password_hash) {
                    Some(User {
                        id: u.id.to_string(),
                        username: u.username,
                        password_hash: u.password_hash,
                        created_at: u.created_at,
                    })
                } else {
                    tracing::warn!(
                        username = form.username,
                        reason = "invalid_credentials",
                        "login rejected"
                    );
                    None
                }
            }
            _ => {
                tracing::warn!(
                    username = form.username,
                    reason = "user_not_found",
                    "login rejected"
                );
                None
            }
        }
    };

    match user_opt {
        Some(user) => {
            let session_token = generate_token(32);
            let expires = (Utc::now() + chrono::Duration::days(30)).to_rfc3339();

            {
                let conn = state.users_db.lock().unwrap();
                let user_id_i64 = user.id.parse::<i64>().unwrap_or(0);
                let now = Utc::now().to_rfc3339();
                let _ = conn.execute(
                    "INSERT INTO sessions (id, user_id, expires_at, created_at) VALUES (?1, ?2, ?3, ?4);",
                    rusqlite::params![session_token, user_id_i64, expires, now],
                );

                // Write audit event in system.db
                let system_conn = state.system_db.lock().unwrap();
                let metadata = format!(
                    "IP: {:?}, UA: {:?}",
                    ip,
                    headers.get("user-agent").and_then(|h| h.to_str().ok())
                );
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    &user.username,
                    "USER_LOGIN",
                    "session",
                    &session_token,
                    Some(&metadata),
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
                let system_conn = state.system_db.lock().unwrap();
                let metadata = format!(
                    "IP: {:?}, UA: {:?}",
                    ip,
                    headers.get("user-agent").and_then(|h| h.to_str().ok())
                );
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    "anonymous",
                    "LOGIN_FAILED",
                    "login",
                    "",
                    Some(&metadata),
                );
            }
            Redirect::to("/admin/login?error=Invalid username or password").into_response()
        }
    }
}

// GET /logout
pub async fn public_logout(State(_state): State<AppState>, jar: CookieJar) -> Response {
    let cookie = Cookie::build("bzod_user_session")
        .path("/")
        .max_age(time::Duration::ZERO)
        .build();

    let mut response_jar = jar.clone();
    response_jar = response_jar.add(cookie);

    (response_jar, Redirect::to("/login")).into_response()
}

// GET /login
pub async fn public_login_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let error = params.get("error").cloned();
    let csrf_token = generate_token(16);
    let cookie = Cookie::build(("bzod_temp_csrf", csrf_token.clone()))
        .path("/login")
        .secure(state.config.cookie_secure)
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Strict)
        .max_age(time::Duration::minutes(10))
        .build();

    let new_jar = jar.add(cookie);
    let template = crate::templates::LoginTemplate {
        error,
        csrf_token,
        action: "/login".to_string(),
        title: "User Login".to_string(),
        subtitle: "Standard account access".to_string(),
        button_text: "Sign In".to_string(),
    };
    (new_jar, template).into_response()
}

// POST /login
pub async fn public_login_post(
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
        return Redirect::to("/login?error=Invalid CSRF token").into_response();
    }

    let ip = get_client_ip(&headers, connect_info);

    let user_opt: Option<crate::models::TenantUser> = {
        let conn = state.users_db.lock().unwrap();
        let user_res: Result<Option<crate::models::TenantUser>, rusqlite::Error> = conn.query_row(
            "SELECT id, username, password_hash, status, created_at, last_login, account_type, organization_id, metadata \
             FROM users WHERE username = ?1;",
            [&form.username],
            |row| {
                Ok(crate::models::TenantUser {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                    last_login: row.get(5)?,
                    account_type: row.get(6)?,
                    organization_id: row.get(7)?,
                    metadata: row.get(8)?,
                })
            }
        ).optional();

        match user_res {
            Ok(Some(u)) => {
                if u.status != "active" {
                    None
                } else if verify_password(&form.password, &u.password_hash) {
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
                let conn = state.users_db.lock().unwrap();
                let _ =
                    crate::db::users::create_user_session(&conn, &session_token, user.id, &expires);

                let system_conn = state.system_db.lock().unwrap();
                let metadata = format!(
                    "IP: {:?}, UA: {:?}",
                    ip,
                    headers.get("user-agent").and_then(|h| h.to_str().ok())
                );
                let _ = crate::db::audit_events::write_audit_event(
                    &system_conn,
                    &user.username,
                    "USER_LOGIN",
                    "session",
                    &session_token,
                    Some(&metadata),
                );
            }

            let cookie = Cookie::build(("bzod_user_session", session_token))
                .path("/")
                .secure(state.config.cookie_secure)
                .http_only(true)
                .same_site(axum_extra::extract::cookie::SameSite::Strict)
                .max_age(time::Duration::days(30))
                .build();

            let clear_temp = Cookie::build("bzod_temp_csrf")
                .path("/login")
                .max_age(time::Duration::ZERO)
                .build();

            let mut response_jar = jar.clone();
            response_jar = response_jar.add(cookie).add(clear_temp);

            (response_jar, Redirect::to("/user/dashboard")).into_response()
        }
        None => {
            let system_conn = state.system_db.lock().unwrap();
            let metadata = format!(
                "IP: {:?}, UA: {:?}",
                ip,
                headers.get("user-agent").and_then(|h| h.to_str().ok())
            );
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                "anonymous",
                "LOGIN_FAILED",
                "login",
                "",
                Some(&metadata),
            );
            Redirect::to("/login?error=Invalid username or password").into_response()
        }
    }
}

// GET /user/dashboard
pub async fn user_dashboard_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let (total_urls, active_links, dead_links) = {
        let conn = user_dbs.content.lock().unwrap();
        get_url_counts(&conn).unwrap_or((0, 0, 0))
    };

    let total_pages = {
        let conn = user_dbs.content.lock().unwrap();
        get_landing_page_count(&conn).unwrap_or(0)
    };

    let total_clicks = {
        let conn = user_dbs.analytics.lock().unwrap();
        get_total_clicks(&conn).unwrap_or(0)
    };

    let clicks_data = {
        let conn = user_dbs.analytics.lock().unwrap();
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
        let conn = user_dbs.analytics.lock().unwrap();
        get_metric_rankings(&conn, "url", "all", "country", 5)
            .or_else(|_| get_metric_rankings_raw(&conn, "url", "all", "country", 5))
            .unwrap_or_default()
    };
    let countries_chart = generate_bar_chart(&countries_data);

    let referrers_data = {
        let conn = user_dbs.analytics.lock().unwrap();
        get_metric_rankings(&conn, "url", "all", "referrer", 5)
            .or_else(|_| get_metric_rankings_raw(&conn, "url", "all", "referrer", 5))
            .unwrap_or_default()
    };
    let referrers_chart = generate_bar_chart(&referrers_data);

    let browsers_data = {
        let conn = user_dbs.analytics.lock().unwrap();
        get_metric_rankings(&conn, "url", "all", "browser", 5)
            .or_else(|_| get_metric_rankings_raw(&conn, "url", "all", "browser", 5))
            .unwrap_or_default()
    };
    let browsers_chart = generate_bar_chart(&browsers_data);

    let template = crate::templates::UserDashboardTemplate {
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

#[derive(Deserialize)]
pub struct UserUrlsQuery {
    pub tag: Option<String>,
    pub error: Option<String>,
    pub page: Option<usize>,
}

#[derive(Deserialize)]
pub struct CreateUserUrlForm {
    pub destination: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub custom_slug: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: String,
    pub csrf_token: String,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub max_access_count: String,
    #[serde(default)]
    pub utm_source: String,
    #[serde(default)]
    pub utm_medium: String,
    #[serde(default)]
    pub utm_campaign: String,
}

pub async fn user_urls_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<UserUrlsQuery>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let (urls, total_pages, page, visible_pages) = {
        let conn = user_dbs.content.lock().unwrap();
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

    let template = crate::templates::UserUrlsTemplate {
        admin_username: user.username.clone(),
        username: user.username,
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

pub async fn user_urls_create(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<CreateUserUrlForm>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/user/urls?error=Invalid CSRF token").into_response();
    }

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let ip = get_client_ip(&headers, connect_info);

    let mut code = form.custom_slug.trim().to_lowercase();
    if code.is_empty() {
        code = form.code.trim().to_lowercase();
        if code.is_empty() {
            code = generate_token(3);
        } else if code.len() != 6 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
            return Redirect::to("/user/urls?error=Custom code must be exactly 6 hex characters")
                .into_response();
        }
    } else if !crate::utils::validation::validate_custom_slug(&code) {
        return Redirect::to(
            "/user/urls?error=Custom slug must start with ! followed by 1-24 a-z, 0-9, -, _",
        )
        .into_response();
    }

    let is_slug_avail = {
        let system_conn = state.system_db.lock().unwrap();
        crate::db::users::is_slug_available(&system_conn, &code).unwrap_or(false)
    };
    if !is_slug_avail {
        return Redirect::to("/user/urls?error=Short code/slug already exists").into_response();
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
            rfc.push_str(":00Z");
        }
        Some(rfc)
    };

    let password_hash_opt = if form.password.trim().is_empty() {
        None
    } else {
        match hash_password(&form.password) {
            Ok(h) => Some(h),
            Err(_) => return Redirect::to("/user/urls?error=Hashing error").into_response(),
        }
    };

    let max_access_count_opt = if form.max_access_count.trim().is_empty() {
        None
    } else {
        match form.max_access_count.trim().parse::<i64>() {
            Ok(c) => Some(c),
            Err(_) => {
                return Redirect::to("/user/urls?error=Invalid max access count").into_response()
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
        let conn = user_dbs.content.lock().unwrap();
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
            let _ = crate::db::users::increment_quota_counter(
                &state.users_db.lock().unwrap(),
                user.id,
                "urls",
            );
            let _ = crate::db::users::register_global_slug(
                &state.system_db.lock().unwrap(),
                &code,
                user.id,
                "url",
                &url.id,
            );
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                &state,
                &user.username,
                "USER_URL_CREATION",
                Some("url"),
                Some(&url.id),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/user/urls").into_response()
        }
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Redirect::to("/user/urls?error=Short code/slug already exists").into_response()
        }
        Err(e) => Redirect::to(&format!("/user/urls?error=Database error: {}", e)).into_response(),
    }
}

pub async fn user_urls_delete(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Path(id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let csrf_token = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &csrf_token) {
        return Redirect::to("/user/urls?error=Invalid CSRF token").into_response();
    }

    let conn = user_dbs.content.lock().unwrap();
    match get_url_by_id(&conn, &id) {
        Ok(Some(url)) => {
            let _ = crate::db::users::decrement_quota_counter(
                &state.users_db.lock().unwrap(),
                user.id,
                "urls",
            );
            match delete_url(&conn, &id) {
                Ok(_) => {
                    let _ = crate::db::users::release_global_slug(
                        &state.system_db.lock().unwrap(),
                        &url.code,
                        user.id,
                    );
                    let ip = get_client_ip(&headers, connect_info);
                    let _ = write_audit_log(
                        &state.admin_db.lock().unwrap(),
                        &state,
                        &user.username,
                        "USER_URL_DELETION",
                        Some("url"),
                        Some(&id),
                        Some(&ip),
                        headers.get("user-agent").and_then(|h| h.to_str().ok()),
                    );
                    Redirect::to("/user/urls").into_response()
                }
                Err(e) => Redirect::to(&format!("/user/urls?error=Failed to delete link: {}", e))
                    .into_response(),
            }
        }
        _ => Redirect::to("/user/urls?error=Link not found").into_response(),
    }
}

#[derive(Deserialize)]
pub struct UserPagesQuery {
    pub error: Option<String>,
    pub page: Option<usize>,
}

#[derive(Deserialize)]
pub struct CreateUserPageForm {
    pub title: String,
    pub slug: String,
    pub code: String,
    pub custom_slug: String,
    pub state: String,
    pub html_content: String,
    pub csrf_token: String,
}

pub async fn user_pages_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<UserPagesQuery>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let (pages, total_pages, page, visible_pages) = {
        let conn = user_dbs.content.lock().unwrap();
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

    let template = crate::templates::UserPagesTemplate {
        admin_username: user.username.clone(),
        username: user.username,
        pages,
        csrf_token,
        error: query.error,
        current_page: page,
        total_pages,
        visible_pages,
    };

    template.into_response()
}

pub async fn user_pages_create(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<CreateUserPageForm>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/user/pages?error=Invalid CSRF token").into_response();
    }

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let mut code = form.custom_slug.trim().to_lowercase();
    if code.is_empty() {
        code = form.code.trim().to_lowercase();
        if code.is_empty() {
            code = generate_token(2);
        } else if code.len() != 4 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
            return Redirect::to("/user/pages?error=Custom code must be exactly 4 hex characters")
                .into_response();
        }
    } else if !crate::utils::validation::validate_custom_slug(&code) {
        return Redirect::to(
            "/user/pages?error=Custom slug must start with ! followed by 1-24 a-z, 0-9, -, _",
        )
        .into_response();
    }

    let clean_slug = form.slug.trim().to_lowercase();
    if clean_slug.is_empty() {
        return Redirect::to("/user/pages?error=Slug is required").into_response();
    }

    let is_slug_avail = {
        let system_conn = state.system_db.lock().unwrap();
        crate::db::users::is_slug_available(&system_conn, &code).unwrap_or(false)
    };
    if !is_slug_avail {
        return Redirect::to("/user/pages?error=Short code/slug already exists").into_response();
    }

    let res = {
        let conn = user_dbs.content.lock().unwrap();
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
            let _ = crate::db::users::increment_quota_counter(
                &state.users_db.lock().unwrap(),
                user.id,
                "landings",
            );
            let _ = crate::db::users::register_global_slug(
                &state.system_db.lock().unwrap(),
                &code,
                user.id,
                "page",
                &page.id,
            );
            let ip = get_client_ip(&headers, connect_info);
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                &state,
                &user.username,
                "USER_PAGE_CREATION",
                Some("page"),
                Some(&page.id),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/user/pages").into_response()
        }
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Redirect::to("/user/pages?error=Short code already exists").into_response()
        }
        Err(e) => Redirect::to(&format!("/user/pages?error=Database error: {}", e)).into_response(),
    }
}

pub async fn user_pages_delete(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Path(id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let csrf_token = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &csrf_token) {
        return Redirect::to("/user/pages?error=Invalid CSRF token").into_response();
    }

    let conn = user_dbs.content.lock().unwrap();
    match get_landing_page_by_id(&conn, &id) {
        Ok(Some(page)) => {
            let _ = crate::db::users::decrement_quota_counter(
                &state.users_db.lock().unwrap(),
                user.id,
                "landings",
            );
            match delete_landing_page(&conn, &id) {
                Ok(_) => {
                    let _ = crate::db::users::release_global_slug(
                        &state.system_db.lock().unwrap(),
                        &page.code,
                        user.id,
                    );
                    let ip = get_client_ip(&headers, connect_info);
                    let _ = write_audit_log(
                        &state.admin_db.lock().unwrap(),
                        &state,
                        &user.username,
                        "USER_PAGE_DELETION",
                        Some("page"),
                        Some(&id),
                        Some(&ip),
                        headers.get("user-agent").and_then(|h| h.to_str().ok()),
                    );
                    Redirect::to("/user/pages").into_response()
                }
                Err(e) => Redirect::to(&format!("/user/pages?error=Failed to delete page: {}", e))
                    .into_response(),
            }
        }
        _ => Redirect::to("/user/pages?error=Page not found").into_response(),
    }
}

#[derive(Deserialize)]
pub struct UserSettingsQuery {
    pub success: Option<String>,
    pub error: Option<String>,
}

pub async fn user_settings_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<UserSettingsQuery>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let csrf_token = generate_csrf_token(&session_id);
    let template = crate::templates::UserSettingsTemplate {
        admin_username: user.username.clone(),
        username: user.username,
        csrf_token,
        success: query.success,
        error: query.error,
    };

    template.into_response()
}

pub async fn user_change_password_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<ChangePasswordForm>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/user/settings?error=Invalid CSRF token").into_response();
    }

    if form.current_password.trim().is_empty() || form.new_password.trim().len() < 8 {
        return Redirect::to("/user/settings?error=Password must be at least 8 characters")
            .into_response();
    }

    let conn = state.users_db.lock().unwrap();
    if !verify_password(&form.current_password, &user.password_hash) {
        let _ = write_audit_log(
            &state.admin_db.lock().unwrap(),
            &state,
            &user.username,
            "USER_PASSWORD_CHANGE_FAIL",
            Some("user"),
            Some(&user.id.to_string()),
            Some(&get_client_ip(&headers, connect_info)),
            headers.get("user-agent").and_then(|h| h.to_str().ok()),
        );
        return Redirect::to("/user/settings?error=Incorrect current password").into_response();
    }

    let new_hash = match hash_password(&form.new_password) {
        Ok(h) => h,
        Err(_) => return Redirect::to("/user/settings?error=Hashing error").into_response(),
    };

    match conn.execute(
        "UPDATE users SET password_hash = ?1 WHERE id = ?2;",
        params![new_hash, user.id],
    ) {
        Ok(_) => {
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                &state,
                &user.username,
                "USER_PASSWORD_CHANGED",
                Some("user"),
                Some(&user.id.to_string()),
                Some(&get_client_ip(&headers, connect_info)),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/user/settings?success=Password updated successfully").into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/user/settings?error=Failed to update password: {}",
            e
        ))
        .into_response(),
    }
}

pub async fn user_download_backup(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
) -> Response {
    let (user, _) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let ip = get_client_ip(&headers, connect_info);
    let user_dir = state
        .config
        .data_dir
        .join("users")
        .join(user.id.to_string());

    let mut buffer = Vec::new();
    let res = {
        let enc = GzEncoder::new(&mut buffer, Compression::default());
        let mut tar = Builder::new(enc);
        let files = vec!["content.db", "analytics.db", "profile.db"];
        let mut add_err = None;
        for f in files {
            let path = user_dir.join(f);
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
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                &state,
                &user.username,
                "USER_BACKUP",
                Some("user"),
                Some(&user.id.to_string()),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            let date_str = Utc::now().format("%Y-%m-%d").to_string();
            let filename = format!("user-{}-bzod-backup.tar.gz", date_str);
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
            Redirect::to(&format!("/user/settings?error=Backup failed: {}", e)).into_response()
        }
    }
}

pub async fn user_restore_backup_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
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
        return Redirect::to("/user/settings?error=Invalid CSRF token").into_response();
    }
    if confirm_text != "RESTORE" {
        return Redirect::to("/user/settings?error=Confirmation text must be exactly 'RESTORE'")
            .into_response();
    }
    if file_bytes.is_empty() {
        return Redirect::to("/user/settings?error=No backup file uploaded").into_response();
    }

    let temp_file_path =
        std::env::temp_dir().join(format!("bzod_user_restore_{}.tar.gz", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::write(&temp_file_path, &file_bytes) {
        return Redirect::to(&format!(
            "/user/settings?error=Failed to write temp file: {}",
            e
        ))
        .into_response();
    }

    let user_dir = state
        .config
        .data_dir
        .join("users")
        .join(user.id.to_string());
    let restore_res = {
        let file = match File::open(&temp_file_path) {
            Ok(f) => f,
            Err(e) => {
                return Redirect::to(&format!(
                    "/user/settings?error=Failed to open upload: {}",
                    e
                ))
                .into_response()
            }
        };
        let tar_gz = GzDecoder::new(file);
        let mut archive = tar::Archive::new(tar_gz);
        if let Err(e) = archive.unpack(&user_dir) {
            Err(e)
        } else {
            Ok(())
        }
    };

    let _ = std::fs::remove_file(&temp_file_path);

    match restore_res {
        Ok(_) => {
            let mut pool = state.user_dbs.lock().unwrap();
            pool.remove(&user.id);
            let _ = write_audit_log(
                &state.admin_db.lock().unwrap(),
                &state,
                &user.username,
                "USER_RESTORE",
                Some("user"),
                Some(&user.id.to_string()),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/user/settings?success=User database restored successfully")
                .into_response()
        }
        Err(e) => {
            Redirect::to(&format!("/user/settings?error=Restore failed: {}", e)).into_response()
        }
    }
}

// GET /admin/logout
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Ok((_, session_id)) = require_auth(&state, &jar).await {
        let conn = state.users_db.lock().unwrap();
        let _ = conn.execute("DELETE FROM sessions WHERE id = ?1;", [&session_id]);
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

#[derive(Deserialize)]
pub struct UsersQuery {
    pub success: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateUserForm {
    pub username: String,
    pub password: String,
    pub account_type: String,
    pub metadata: String,
    pub csrf_token: String,
}

#[derive(Deserialize)]
pub struct UpdateUserStatusForm {
    pub status: String,
    pub csrf_token: String,
}

#[derive(Deserialize)]
pub struct UpdateUserTypeForm {
    pub account_type: String,
    pub csrf_token: String,
}

#[derive(Deserialize)]
pub struct ResetPasswordForm {
    pub new_password: String,
    pub csrf_token: String,
}

#[derive(Deserialize)]
pub struct DeleteUserForm {
    pub csrf_token: String,
}

pub async fn users_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<UsersQuery>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let users = {
        let conn = state.users_db.lock().unwrap();
        crate::db::users::list_users(&conn).unwrap_or_default()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::UsersTemplate {
        admin_username: user.username,
        users,
        csrf_token,
        success: query.success,
        error: query.error,
    };

    template.into_response()
}

pub async fn users_create_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<CreateUserForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/users?error=Invalid CSRF token").into_response();
    }

    let username = form.username.trim().to_lowercase();
    if username.len() < 3 {
        return Redirect::to("/admin/users?error=Username must be at least 3 characters")
            .into_response();
    }
    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Redirect::to(
            "/admin/users?error=Username must contain only alphanumeric characters, hyphens, or underscores",
        )
        .into_response();
    }

    if form.password.trim().len() < 8 {
        return Redirect::to("/admin/users?error=Password must be at least 8 characters")
            .into_response();
    }

    let account_type = if form.account_type.trim().is_empty() {
        "standard"
    } else {
        form.account_type.trim()
    };

    let metadata = if form.metadata.trim().is_empty() {
        None
    } else {
        Some(form.metadata.trim())
    };

    let hash = match hash_password(&form.password) {
        Ok(h) => h,
        Err(_) => return Redirect::to("/admin/users?error=Internal hashing error").into_response(),
    };

    let new_user = {
        let conn = state.users_db.lock().unwrap();
        match crate::db::users::create_user(&conn, &username, &hash, account_type, metadata) {
            Ok(u) => u,
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Redirect::to("/admin/users?error=Username already exists").into_response();
            }
            Err(e) => {
                return Redirect::to(&format!("/admin/users?error=Database error: {}", e))
                    .into_response();
            }
        }
    };

    if let Err(e) = state.db.init_user_databases(new_user.id) {
        return Redirect::to(&format!(
            "/admin/users?error=Failed to initialize user databases: {}",
            e
        ))
        .into_response();
    }

    {
        let conn = state.admin_db.lock().unwrap();
        let _ = write_audit_log(
            &conn,
            &state,
            &user.username,
            "USER_CREATION",
            Some("user"),
            Some(&new_user.id.to_string()),
            None,
            None,
        );
    }

    Redirect::to("/admin/users?success=User created successfully").into_response()
}

pub async fn users_update_status_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<UpdateUserStatusForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/users?error=Invalid CSRF token").into_response();
    }

    let status = form.status.trim().to_lowercase();
    if !["active", "disabled", "suspended", "pending", "deleted"].contains(&status.as_str()) {
        return Redirect::to("/admin/users?error=Invalid user status").into_response();
    }

    let conn = state.users_db.lock().unwrap();
    match crate::db::users::update_user_status(&conn, id, &status) {
        Ok(_) => {
            let conn_admin = state.admin_db.lock().unwrap();
            let _ = write_audit_log(
                &conn_admin,
                &state,
                &user.username,
                "USER_STATUS_UPDATE",
                Some("user"),
                Some(&id.to_string()),
                None,
                None,
            );
            Redirect::to("/admin/users?success=User status updated").into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/admin/users?error=Failed to update status: {}",
            e
        ))
        .into_response(),
    }
}

pub async fn users_update_type_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<UpdateUserTypeForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/users?error=Invalid CSRF token").into_response();
    }

    let account_type = form.account_type.trim().to_lowercase();
    if !["admin", "standard", "organization", "service", "system"].contains(&account_type.as_str())
    {
        return Redirect::to("/admin/users?error=Invalid account type").into_response();
    }

    let conn = state.users_db.lock().unwrap();
    match crate::db::users::update_user_account_type(&conn, id, &account_type) {
        Ok(_) => {
            let conn_admin = state.admin_db.lock().unwrap();
            let _ = write_audit_log(
                &conn_admin,
                &state,
                &user.username,
                "USER_ACCOUNT_TYPE_UPDATE",
                Some("user"),
                Some(&id.to_string()),
                None,
                None,
            );
            Redirect::to("/admin/users?success=User account type updated").into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/admin/users?error=Failed to update account type: {}",
            e
        ))
        .into_response(),
    }
}

pub async fn users_reset_password_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Path(id): Path<i64>,
    Form(form): Form<ResetPasswordForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/users?error=Invalid CSRF token").into_response();
    }

    if form.new_password.trim().len() < 8 {
        return Redirect::to("/admin/users?error=Password must be at least 8 characters")
            .into_response();
    }

    let hash = match hash_password(&form.new_password) {
        Ok(h) => h,
        Err(_) => return Redirect::to("/admin/users?error=Internal hashing error").into_response(),
    };

    let conn = state.users_db.lock().unwrap();
    match crate::db::users::reset_user_password(&conn, id, &hash) {
        Ok(_) => {
            let ip = get_client_ip(&headers, connect_info);
            let conn_admin = state.admin_db.lock().unwrap();
            let _ = write_audit_log(
                &conn_admin,
                &state,
                &user.username,
                "USER_PASSWORD_RESET",
                Some("user"),
                Some(&id.to_string()),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/admin/users?success=Password reset successfully").into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/admin/users?error=Failed to reset password: {}",
            e
        ))
        .into_response(),
    }
}

pub async fn users_delete_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Path(id): Path<i64>,
    Form(form): Form<DeleteUserForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/users?error=Invalid CSRF token").into_response();
    }

    match crate::web::multi_user::delete_user_resources(&state, id, &user.username, false) {
        Ok(_) => {
            let ip = get_client_ip(&headers, connect_info);
            let conn_admin = state.admin_db.lock().unwrap();
            let _ = write_audit_log(
                &conn_admin,
                &state,
                &user.username,
                "USER_DELETION",
                Some("user"),
                Some(&id.to_string()),
                Some(&ip),
                headers.get("user-agent").and_then(|h| h.to_str().ok()),
            );
            Redirect::to("/admin/users?success=User deleted successfully").into_response()
        }
        Err(err) => Redirect::to(&format!("/admin/users?error={}", err)).into_response(),
    }
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

// GET /user/audit
pub async fn user_audit_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let logs = {
        let conn = state.system_db.lock().unwrap();
        let events =
            crate::db::audit_events::list_audit_events(&conn, 100, 0, Some(&user.username), None)
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

    let template = crate::templates::UserAuditTemplate {
        admin_username: user.username,
        logs,
    };

    template.into_response()
}

// GET /user/status
pub async fn user_status_get(State(state): State<AppState>, jar: CookieJar) -> Response {
    let (user, _) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response(),
    };

    let app_status = "Healthy";

    let db_status = {
        let conn_ok = {
            let conn = user_dbs.content.lock().unwrap();
            get_url_counts(&conn).is_ok()
        };
        if conn_ok {
            "Operational".to_string()
        } else {
            "Degraded (Database connection failed)".to_string()
        }
    };

    let queue_size = 0;
    let memory_usage = get_memory_usage();

    let uptime_duration = state.start_time.elapsed();
    let uptime = crate::utils::format_duration(uptime_duration);

    let urls = {
        let conn = user_dbs.content.lock().unwrap();
        crate::db::content::list_urls(&conn, 50, 0, None).unwrap_or_default()
    };

    let template = crate::templates::UserStatusTemplate {
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

// ---------------------------------------------------------------------------
// GA Hardening UI Handlers and Structs
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UserDetailStats {
    pub max_urls: i64,
    pub max_landings: i64,
    pub max_api_tokens: i64,
    pub max_storage_mb: i64,
    pub current_urls: i64,
    pub current_landings: i64,
    pub current_api_tokens: i64,
    pub current_storage_mb: i64,
    pub url_pct: i64,
    pub landing_pct: i64,
    pub token_pct: i64,
    pub storage_pct: i64,
    pub total_visits: i64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GlobalSlugRow {
    pub slug: String,
    pub owner_user_id: i64,
    pub target_type: String,
    pub target_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub deleted_at: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ModerationLogEntry {
    pub id: String,
    pub timestamp: String,
    pub admin_username: String,
    pub target_user_id: i64,
    pub target_username: Option<String>,
    pub resource_type: String,
    pub resource_identifier: String,
    pub action: String,
    pub severity: String,
    pub reason: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SlugHistoryRow {
    pub id: i64,
    pub slug: String,
    pub old_owner_user_id: Option<i64>,
    pub new_owner_user_id: Option<i64>,
    pub action: String,
    pub timestamp: String,
    pub admin_username: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct JobHistoryRow {
    pub id: String,
    pub job_name: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HealthCheckRow {
    pub id: String,
    pub object_type: String,
    pub object_id: String,
    pub checked_at: String,
    pub status_code: Option<i64>,
    pub error_message: Option<String>,
    pub is_healthy: i64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BackupFileRow {
    pub filename: String,
    pub size_str: String,
    pub created_str: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BackupHistoryRow {
    pub id: String,
    pub backup_path: String,
    pub status: String,
    pub created_at: String,
    pub size_bytes: i64,
    pub error_message: Option<String>,
}

// Helpers
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn get_dir_size(dir: &std::path::Path) -> std::io::Result<u64> {
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

pub fn get_user_detail_stats(
    state: &AppState,
    user_id: i64,
) -> Result<UserDetailStats, Box<dyn std::error::Error>> {
    use rusqlite::OptionalExtension;
    let quotas = {
        let conn = state.users_db.lock().unwrap();
        conn.query_row(
            "SELECT max_urls, max_landings, max_api_tokens, max_storage_mb, current_urls, current_landings, current_api_tokens, current_storage_mb 
             FROM quotas WHERE user_id = ?1;",
            [user_id],
            |row| Ok(crate::models::UserQuotas {
                user_id,
                max_urls: row.get(0)?,
                max_landings: row.get(1)?,
                max_api_tokens: row.get(2)?,
                max_storage_mb: row.get(3)?,
                current_urls: row.get(4)?,
                current_landings: row.get(5)?,
                current_api_tokens: row.get(6)?,
                current_storage_mb: row.get(7)?,
            })
        ).optional()?
    };

    let quotas = quotas.unwrap_or(crate::models::UserQuotas {
        user_id,
        max_urls: 100,
        max_landings: 10,
        max_api_tokens: 5,
        max_storage_mb: 100,
        current_urls: 0,
        current_landings: 0,
        current_api_tokens: 0,
        current_storage_mb: 0,
    });

    let total_visits = {
        if let Ok(dbs) = state.get_user_dbs(user_id) {
            let conn = dbs.analytics.lock().unwrap();
            conn.query_row("SELECT COUNT(*) FROM visits;", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap_or(0)
        } else {
            0
        }
    };

    let url_pct = if quotas.max_urls > 0 {
        (quotas.current_urls * 100) / quotas.max_urls
    } else {
        0
    };
    let landing_pct = if quotas.max_landings > 0 {
        (quotas.current_landings * 100) / quotas.max_landings
    } else {
        0
    };
    let token_pct = if quotas.max_api_tokens > 0 {
        (quotas.current_api_tokens * 100) / quotas.max_api_tokens
    } else {
        0
    };
    let storage_pct = if quotas.max_storage_mb > 0 {
        (quotas.current_storage_mb * 100) / quotas.max_storage_mb
    } else {
        0
    };

    Ok(UserDetailStats {
        max_urls: quotas.max_urls,
        max_landings: quotas.max_landings,
        max_api_tokens: quotas.max_api_tokens,
        max_storage_mb: quotas.max_storage_mb,
        current_urls: quotas.current_urls,
        current_landings: quotas.current_landings,
        current_api_tokens: quotas.current_api_tokens,
        current_storage_mb: quotas.current_storage_mb,
        url_pct,
        landing_pct,
        token_pct,
        storage_pct,
        total_visits,
    })
}

// GET /admin/users/new
pub async fn users_new_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::UsersNewTemplate {
        admin_username: user.username,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

// GET /admin/users/:id
pub async fn user_detail_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let target_user = {
        let conn = state.users_db.lock().unwrap();
        let u_res = conn.query_row(
            "SELECT id, username, password_hash, status, created_at, last_login, account_type, organization_id, metadata 
             FROM users WHERE id = ?1;",
            [id],
            |row| Ok(crate::models::TenantUser {
                id: row.get(0)?,
                username: row.get(1)?,
                password_hash: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                last_login: row.get(5)?,
                account_type: row.get(6)?,
                organization_id: row.get(7)?,
                metadata: row.get(8)?,
            })
        );
        match u_res {
            Ok(u) => u,
            Err(_) => return Redirect::to("/admin/users?error=User not found").into_response(),
        }
    };

    let stats = match get_user_detail_stats(&state, id) {
        Ok(s) => s,
        Err(_) => return Redirect::to("/admin/users?error=Database error").into_response(),
    };

    let sessions = {
        let conn = state.users_db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, user_id, expires_at, created_at FROM sessions WHERE user_id = ?1;")
            .unwrap();
        let rows = stmt
            .query_map([id], |row| {
                Ok(crate::models::UserSession {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    expires_at: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let tokens = {
        let conn = state.users_db.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, token_hash, created_at FROM api_tokens WHERE user_id = ?1;",
            )
            .unwrap();
        let rows = stmt
            .query_map([id], |row| {
                Ok(crate::models::UserApiToken {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    token_hash: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::UserDetailTemplate {
        admin_username: user.username,
        target_user,
        stats,
        sessions,
        tokens,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

// GET /admin/users/:id/edit
pub async fn user_edit_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let target_user = {
        let conn = state.users_db.lock().unwrap();
        let u_res = conn.query_row(
            "SELECT id, username, password_hash, status, created_at, last_login, account_type, organization_id, metadata 
             FROM users WHERE id = ?1;",
            [id],
            |row| Ok(crate::models::TenantUser {
                id: row.get(0)?,
                username: row.get(1)?,
                password_hash: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                last_login: row.get(5)?,
                account_type: row.get(6)?,
                organization_id: row.get(7)?,
                metadata: row.get(8)?,
            })
        );
        match u_res {
            Ok(u) => u,
            Err(_) => return Redirect::to("/admin/users?error=User not found").into_response(),
        }
    };

    let quotas = {
        let conn = state.users_db.lock().unwrap();
        conn.query_row(
            "SELECT max_urls, max_landings, max_api_tokens, max_storage_mb, current_urls, current_landings, current_api_tokens, current_storage_mb 
             FROM quotas WHERE user_id = ?1;",
            [id],
            |row| Ok(crate::models::UserQuotas {
                user_id: id,
                max_urls: row.get(0)?,
                max_landings: row.get(1)?,
                max_api_tokens: row.get(2)?,
                max_storage_mb: row.get(3)?,
                current_urls: row.get(4)?,
                current_landings: row.get(5)?,
                current_api_tokens: row.get(6)?,
                current_storage_mb: row.get(7)?,
            })
        ).unwrap_or(crate::models::UserQuotas {
            user_id: id,
            max_urls: 100,
            max_landings: 10,
            max_api_tokens: 5,
            max_storage_mb: 100,
            current_urls: 0,
            current_landings: 0,
            current_api_tokens: 0,
            current_storage_mb: 0,
        })
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::UserEditTemplate {
        admin_username: user.username,
        target_user,
        quotas,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

#[derive(Deserialize)]
pub struct UserEditForm {
    pub account_type: String,
    pub metadata: String,
    pub max_urls: i64,
    pub max_landings: i64,
    pub max_api_tokens: i64,
    pub max_storage_mb: i64,
    pub csrf_token: String,
}

// POST /admin/users/:id/edit
pub async fn user_edit_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<UserEditForm>,
) -> Response {
    let (_user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to(&format!(
            "/admin/users/{}/edit?error=Invalid CSRF token",
            id
        ))
        .into_response();
    }

    let conn = state.users_db.lock().unwrap();
    let _ = conn.execute(
        "UPDATE users SET account_type = ?1, metadata = ?2 WHERE id = ?3;",
        rusqlite::params![form.account_type, form.metadata, id],
    );

    let _ = conn.execute(
        "INSERT INTO quotas (user_id, max_urls, max_landings, max_api_tokens, max_storage_mb) 
         VALUES (?1, ?2, ?3, ?4, ?5) 
         ON CONFLICT(user_id) DO UPDATE SET 
            max_urls = excluded.max_urls, 
            max_landings = excluded.max_landings, 
            max_api_tokens = excluded.max_api_tokens, 
            max_storage_mb = excluded.max_storage_mb;",
        rusqlite::params![
            id,
            form.max_urls,
            form.max_landings,
            form.max_api_tokens,
            form.max_storage_mb
        ],
    );

    Redirect::to(&format!(
        "/admin/users/{}?success=User updated successfully",
        id
    ))
    .into_response()
}

// GET /admin/moderation
pub async fn moderation_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let flagged_items = {
        let conn = state.system_db.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT slug, owner_user_id, target_type, target_id, created_at, updated_at, status, deleted_at 
             FROM global_slugs WHERE status = 'flagged' OR status = 'disabled' ORDER BY updated_at DESC;"
        ).unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(GlobalSlugRow {
                    slug: row.get(0)?,
                    owner_user_id: row.get(1)?,
                    target_type: row.get(2)?,
                    target_id: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    status: row.get(6)?,
                    deleted_at: row.get(7)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let logs = {
        let conn = state.system_db.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, admin_username, target_user_id, target_username, resource_type, resource_identifier, action, severity, reason 
             FROM moderation_events ORDER BY timestamp DESC LIMIT 50;"
        ).unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(ModerationLogEntry {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    admin_username: row.get(2)?,
                    target_user_id: row.get(3)?,
                    target_username: row.get(4)?,
                    resource_type: row.get(5)?,
                    resource_identifier: row.get(6)?,
                    action: row.get(7)?,
                    severity: row.get(8)?,
                    reason: row.get(9)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::ModerationTemplate {
        admin_username: user.username,
        flagged_items,
        logs,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

#[derive(Deserialize)]
pub struct AdminModerateForm {
    pub slug: String,
    pub action: String,
    pub severity: String,
    pub reason: String,
    pub csrf_token: String,
}

// POST /admin/moderation
pub async fn moderation_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<AdminModerateForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/moderation?error=Invalid CSRF token").into_response();
    }

    let action = form.action.trim().to_lowercase();
    if !["flagged", "disabled", "active", "deleted"].contains(&action.as_str()) {
        return Redirect::to("/admin/moderation?error=Invalid moderation action").into_response();
    }

    let (owner_user_id, target_type) = {
        let system_conn = state.system_db.lock().unwrap();
        let row_opt: Option<(i64, String)> = system_conn
            .query_row(
                "SELECT owner_user_id, target_type FROM global_slugs WHERE slug = ?1;",
                [&form.slug],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .unwrap_or(None);

        match row_opt {
            Some(r) => r,
            None => return Redirect::to("/admin/moderation?error=Slug not found").into_response(),
        }
    };

    let owner_username = {
        let users_conn = state.users_db.lock().unwrap();
        crate::db::users::get_user_by_id(&users_conn, owner_user_id)
            .unwrap_or(None)
            .map(|u| u.username)
    };

    {
        let system_conn = state.system_db.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        if action == "deleted" {
            let _ = system_conn.execute("DELETE FROM global_slugs WHERE slug = ?1;", [&form.slug]);
        } else {
            let _ = system_conn.execute(
                "UPDATE global_slugs SET status = ?1, updated_at = ?2 WHERE slug = ?3;",
                rusqlite::params![action, now, form.slug],
            );
        }

        let event_id = Uuid::new_v4().to_string();
        let _ = system_conn.execute(
            "INSERT INTO moderation_events (id, timestamp, admin_username, target_user_id, target_username, resource_type, resource_identifier, action, severity, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);",
            rusqlite::params![
                event_id,
                now,
                user.username,
                owner_user_id,
                owner_username,
                target_type,
                form.slug,
                action,
                form.severity,
                form.reason
            ],
        );

        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            &user.username,
            "CONTENT_MODERATION",
            "slug",
            &form.slug,
            Some(&format!("Action: {}, Reason: {}", action, form.reason)),
        );
    }

    Redirect::to(&format!(
        "/admin/moderation?success=Moderation action '{}' applied",
        action
    ))
    .into_response()
}

#[derive(Deserialize)]
pub struct SlugsQuery {
    pub search: Option<String>,
    pub owner: Option<i64>,
    pub status: Option<String>,
    pub success: Option<String>,
    pub error: Option<String>,
}

// GET /admin/slugs
pub async fn slugs_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<SlugsQuery>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let system_conn = state.system_db.lock().unwrap();

    let mut sql = "SELECT slug, owner_user_id, target_type, target_id, created_at, updated_at, status, deleted_at FROM global_slugs WHERE 1=1".to_string();
    let mut values = vec![];
    if let Some(ref s) = query.search {
        if !s.trim().is_empty() {
            sql.push_str(" AND slug LIKE ?");
            values.push(rusqlite::types::Value::Text(format!("%{}%", s.trim())));
        }
    }
    if let Some(o) = query.owner {
        sql.push_str(" AND owner_user_id = ?");
        values.push(rusqlite::types::Value::Integer(o));
    }
    if let Some(ref st) = query.status {
        if !st.trim().is_empty() {
            sql.push_str(" AND status = ?");
            values.push(rusqlite::types::Value::Text(st.trim().to_string()));
        }
    }
    sql.push_str(" ORDER BY created_at DESC LIMIT 100;");

    let slugs = {
        let mut stmt = system_conn.prepare(&sql).unwrap();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(values.iter()), |row| {
                Ok(GlobalSlugRow {
                    slug: row.get(0)?,
                    owner_user_id: row.get(1)?,
                    target_type: row.get(2)?,
                    target_id: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    status: row.get(6)?,
                    deleted_at: row.get(7)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let history = {
        let mut stmt = system_conn.prepare(
            "SELECT id, slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username 
             FROM slug_history ORDER BY timestamp DESC LIMIT 50;"
        ).unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(SlugHistoryRow {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    old_owner_user_id: row.get(2)?,
                    new_owner_user_id: row.get(3)?,
                    action: row.get(4)?,
                    timestamp: row.get(5)?,
                    admin_username: row.get(6)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::SlugsTemplate {
        admin_username: user.username,
        slugs,
        history,
        csrf_token,
        search_filter: query.search,
        owner_filter: query.owner,
        status_filter: query.status,
        success: query.success,
        error: query.error,
    };

    template.into_response()
}

#[derive(Deserialize)]
pub struct AdminTransferForm {
    pub slug: String,
    pub new_owner_user_id: i64,
    pub csrf_token: String,
}

// POST /admin/slugs/transfer
pub async fn slugs_transfer_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<AdminTransferForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/slugs?error=Invalid CSRF token").into_response();
    }

    let user_exists = {
        let conn = state.users_db.lock().unwrap();
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1);",
            [form.new_owner_user_id],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
    };

    if !user_exists {
        return Redirect::to("/admin/slugs?error=New owner user ID does not exist").into_response();
    }

    let (old_owner, target_type, mut new_target_id) =
        {
            let conn = state.system_db.lock().unwrap();
            let row_opt: Option<(i64, String, String)> = conn.query_row(
            "SELECT owner_user_id, target_type, target_id FROM global_slugs WHERE slug = ?1;",
            [&form.slug],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        ).optional().unwrap_or(None);
            match row_opt {
                Some(r) => r,
                None => return Redirect::to("/admin/slugs?error=Slug not found").into_response(),
            }
        };

    if old_owner == form.new_owner_user_id {
        return Redirect::to("/admin/slugs?error=Slug is already owned by this user")
            .into_response();
    }

    let old_dbs = match state.get_user_dbs(old_owner) {
        Ok(dbs) => dbs,
        Err(_) => {
            return Redirect::to("/admin/slugs?error=Failed to load current owner's database")
                .into_response()
        }
    };
    let new_dbs = match state.get_user_dbs(form.new_owner_user_id) {
        Ok(dbs) => dbs,
        Err(_) => {
            return Redirect::to("/admin/slugs?error=Failed to load new owner's database")
                .into_response()
        }
    };

    {
        let old_conn = old_dbs.content.lock().unwrap();
        let new_conn = new_dbs.content.lock().unwrap();

        if target_type == "url" {
            let url_opt = match crate::db::content::get_url_by_code(&old_conn, &form.slug) {
                Ok(u) => u,
                Err(e) => {
                    return Redirect::to(&format!(
                        "/admin/slugs?error=Failed to retrieve old URL: {}",
                        e
                    ))
                    .into_response()
                }
            };

            if let Some(url) = url_opt {
                // Check quota
                let users_conn = state.users_db.lock().unwrap();
                let quota_opt =
                    crate::db::users::get_user_quotas(&users_conn, form.new_owner_user_id)
                        .unwrap_or(None);
                if let Some(quota) = quota_opt {
                    if quota.current_urls >= quota.max_urls {
                        return Redirect::to(
                            "/admin/slugs?error=New owner has exceeded URL quota limit",
                        )
                        .into_response();
                    }
                }

                // Copy
                let new_url = match crate::db::content::create_url_extended(
                    &new_conn,
                    &url.code,
                    &url.destination,
                    url.title.as_deref(),
                    url.description.as_deref(),
                    &url.tags,
                    url.expires_at.as_deref(),
                    url.password_hash.as_deref(),
                    url.max_access_count,
                ) {
                    Ok(u) => u,
                    Err(e) => {
                        return Redirect::to(&format!(
                            "/admin/slugs?error=Failed to copy URL record: {}",
                            e
                        ))
                        .into_response()
                    }
                };
                new_target_id = new_url.id;

                // Delete old
                let _ = crate::db::content::delete_url(&old_conn, &url.id);
            }
        } else if target_type == "page" {
            let page_opt = match crate::db::content::get_landing_page_by_code(&old_conn, &form.slug)
            {
                Ok(p) => p,
                Err(e) => {
                    return Redirect::to(&format!(
                        "/admin/slugs?error=Failed to retrieve old page: {}",
                        e
                    ))
                    .into_response()
                }
            };

            if let Some(page) = page_opt {
                // Check quota
                let users_conn = state.users_db.lock().unwrap();
                let quota_opt =
                    crate::db::users::get_user_quotas(&users_conn, form.new_owner_user_id)
                        .unwrap_or(None);
                if let Some(quota) = quota_opt {
                    if quota.current_landings >= quota.max_landings {
                        return Redirect::to(
                            "/admin/slugs?error=New owner has exceeded landing page quota limit",
                        )
                        .into_response();
                    }
                }

                // Copy
                let new_page = match crate::db::content::create_landing_page(
                    &new_conn,
                    &page.code,
                    &page.slug,
                    &page.title,
                    &page.html_content,
                    &page.state,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        return Redirect::to(&format!(
                            "/admin/slugs?error=Failed to copy page record: {}",
                            e
                        ))
                        .into_response()
                    }
                };
                new_target_id = new_page.id;

                // Delete old
                let _ = crate::db::content::delete_landing_page(&old_conn, &page.id);
            }
        }
    }

    {
        let conn = state.system_db.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let _ = conn.execute(
            "UPDATE global_slugs SET owner_user_id = ?1, target_id = ?2, updated_at = ?3 WHERE slug = ?4;",
            rusqlite::params![form.new_owner_user_id, new_target_id, now, form.slug],
        );

        let _ = conn.execute(
            "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username) 
             VALUES (?1, ?2, ?3, 'transferred', ?4, ?5);",
            rusqlite::params![form.slug, old_owner, form.new_owner_user_id, now, user.username],
        );

        let _ = crate::db::audit_events::write_audit_event(
            &conn,
            &user.username,
            "SLUG_TRANSFER",
            "slug",
            &form.slug,
            Some(&format!(
                "Transferred from {} to {}",
                old_owner, form.new_owner_user_id
            )),
        );
    }

    Redirect::to(&format!(
        "/admin/slugs?success=Slug /{} successfully transferred to user {}",
        form.slug, form.new_owner_user_id
    ))
    .into_response()
}

#[derive(Deserialize)]
pub struct AdminSlugStatusForm {
    pub slug: String,
    pub status: String,
    pub csrf_token: String,
}

// POST /admin/slugs/status
pub async fn slugs_status_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<AdminSlugStatusForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/slugs?error=Invalid CSRF token").into_response();
    }

    let status = form.status.trim().to_lowercase();
    if !["active", "flagged", "disabled"].contains(&status.as_str()) {
        return Redirect::to("/admin/slugs?error=Invalid status").into_response();
    }

    {
        let conn = state.system_db.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let _ = conn.execute(
            "UPDATE global_slugs SET status = ?1, updated_at = ?2 WHERE slug = ?3;",
            rusqlite::params![status, now, form.slug],
        );

        let _ = crate::db::audit_events::write_audit_event(
            &conn,
            &user.username,
            "SLUG_STATUS_UPDATE",
            "slug",
            &form.slug,
            Some(&format!("Status set to {}", status)),
        );
    }

    Redirect::to(&format!(
        "/admin/slugs?success=Slug /{} status updated to {}",
        form.slug, status
    ))
    .into_response()
}

#[derive(Deserialize)]
pub struct AdminSlugDeleteForm {
    pub slug: String,
    pub csrf_token: String,
}

// POST /admin/slugs/delete
pub async fn slugs_delete_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<AdminSlugDeleteForm>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    if !verify_csrf(&session_id, &form.csrf_token) {
        return Redirect::to("/admin/slugs?error=Invalid CSRF token").into_response();
    }

    {
        let conn = state.system_db.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let old_owner_user_id: Option<i64> = conn
            .query_row(
                "SELECT owner_user_id FROM global_slugs WHERE slug = ?1;",
                [&form.slug],
                |row| row.get(0),
            )
            .optional()
            .unwrap_or(None);

        let _ = conn.execute("DELETE FROM global_slugs WHERE slug = ?1;", [&form.slug]);

        let _ = conn.execute(
            "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username) 
             VALUES (?1, ?2, NULL, 'deleted', ?3, ?4);",
            rusqlite::params![form.slug, old_owner_user_id, now, user.username],
        );

        let _ = crate::db::audit_events::write_audit_event(
            &conn,
            &user.username,
            "SLUG_RELEASE",
            "slug",
            &form.slug,
            Some("Slug released/deleted from global index"),
        );
    }

    Redirect::to(&format!(
        "/admin/slugs?success=Slug /{} released successfully",
        form.slug
    ))
    .into_response()
}

// GET /admin/sessions
pub async fn sessions_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let sessions = {
        let conn = state.users_db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, user_id, expires_at, created_at FROM sessions ORDER BY created_at DESC;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(crate::models::UserSession {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    expires_at: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::SessionsTemplate {
        admin_username: user.username,
        sessions,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

// POST /admin/sessions/revoke/:id
pub async fn sessions_revoke_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let form_csrf = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &form_csrf) {
        return Redirect::to("/admin/sessions?error=Invalid CSRF token").into_response();
    }

    {
        let conn = state.users_db.lock().unwrap();
        let _ = conn.execute("DELETE FROM sessions WHERE id = ?1;", [&id]);
    }

    {
        let conn_sys = state.system_db.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &conn_sys,
            &user.username,
            "SESSION_REVOKED",
            "session",
            &id,
            None,
        );
    }

    Redirect::to("/admin/sessions?success=Session revoked").into_response()
}

// POST /admin/sessions/revoke-all
pub async fn sessions_revoke_all_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let form_csrf = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &form_csrf) {
        return Redirect::to("/admin/sessions?error=Invalid CSRF token").into_response();
    }

    {
        let conn = state.users_db.lock().unwrap();
        let _ = conn.execute("DELETE FROM sessions;", []);
    }

    {
        let conn_sys = state.system_db.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &conn_sys,
            &user.username,
            "SESSIONS_ALL_REVOKED",
            "session",
            "all",
            None,
        );
    }

    Redirect::to("/admin/sessions?success=All active sessions revoked").into_response()
}

// GET /admin/quotas
pub async fn quotas_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let quotas = {
        let conn = state.users_db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT user_id, max_urls, max_landings, max_api_tokens, max_storage_mb, current_urls, current_landings, current_api_tokens, current_storage_mb FROM quotas ORDER BY user_id ASC;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(crate::models::UserQuotas {
                    user_id: row.get(0)?,
                    max_urls: row.get(1)?,
                    max_landings: row.get(2)?,
                    max_api_tokens: row.get(3)?,
                    max_storage_mb: row.get(4)?,
                    current_urls: row.get(5)?,
                    current_landings: row.get(6)?,
                    current_api_tokens: row.get(7)?,
                    current_storage_mb: row.get(8)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::QuotasTemplate {
        admin_username: user.username,
        quotas,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

// POST /admin/quotas
pub async fn quotas_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let form_csrf = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &form_csrf) {
        return Redirect::to("/admin/quotas?error=Invalid CSRF token").into_response();
    }

    let action = form.get("action").cloned().unwrap_or_default();
    let users_conn = state.users_db.lock().unwrap();

    if action == "reconcile_all" {
        let user_ids: Vec<i64> = {
            let mut stmt = users_conn.prepare("SELECT id FROM users;").unwrap();
            let rows = stmt.query_map([], |row| row.get(0)).unwrap();
            rows.filter_map(|r| r.ok()).collect()
        };

        for uid in user_ids {
            if let Ok(user_dbs) = state.get_user_dbs(uid) {
                let user_content_conn = user_dbs.content.lock().unwrap();
                let _ =
                    crate::db::users::reconcile_user_quotas(&users_conn, uid, &user_content_conn);
            }
        }

        let system_conn = state.system_db.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
            &user.username,
            "QUOTAS_RECONCILE_ALL",
            "quota",
            "all",
            None,
        );

        Redirect::to("/admin/quotas?success=All user quotas reconciled successfully")
            .into_response()
    } else if action == "reconcile" {
        let uid_str = form.get("user_id").cloned().unwrap_or_default();
        let uid = uid_str.parse::<i64>().unwrap_or(0);
        if let Ok(user_dbs) = state.get_user_dbs(uid) {
            let user_content_conn = user_dbs.content.lock().unwrap();
            let _ = crate::db::users::reconcile_user_quotas(&users_conn, uid, &user_content_conn);

            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &user.username,
                "QUOTAS_RECONCILE",
                "quota",
                &uid.to_string(),
                None,
            );
            Redirect::to(&format!("/admin/users/{}?success=Quotas reconciled", uid)).into_response()
        } else {
            Redirect::to("/admin/quotas?error=User databases not found").into_response()
        }
    } else {
        Redirect::to("/admin/quotas?error=Invalid action").into_response()
    }
}

// GET /admin/health
pub async fn health_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let mut db_reports = vec![];
    if let Ok(r) =
        crate::db::sqlite::collect_health_report(&state.admin_db.lock().unwrap(), "admin")
    {
        db_reports.push(r);
    }
    if let Ok(r) =
        crate::db::sqlite::collect_health_report(&state.system_db.lock().unwrap(), "system")
    {
        db_reports.push(r);
    }
    if let Ok(r) =
        crate::db::sqlite::collect_health_report(&state.users_db.lock().unwrap(), "users")
    {
        db_reports.push(r);
    }

    let system_db_path = state.config.data_dir.join("admin").join("system.db");
    let users_db_path = state.config.data_dir.join("admin").join("users.db");
    let admin_db_path = state.config.data_dir.join("admin").join("admin.db");

    let system_db_size = format_size(
        std::fs::metadata(&system_db_path)
            .map(|m| m.len())
            .unwrap_or(0),
    );
    let users_db_size = format_size(
        std::fs::metadata(&users_db_path)
            .map(|m| m.len())
            .unwrap_or(0),
    );
    let admin_db_size = format_size(
        std::fs::metadata(&admin_db_path)
            .map(|m| m.len())
            .unwrap_or(0),
    );

    let users_dir = state.config.data_dir.join("users");
    let tenants_db_size = format_size(get_dir_size(&users_dir).unwrap_or(0));
    let total_data_size = format_size(get_dir_size(&state.config.data_dir).unwrap_or(0));

    let job_history = {
        let conn = state.system_db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, job_name, status, started_at, finished_at, error_message FROM job_history ORDER BY started_at DESC LIMIT 20;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(JobHistoryRow {
                    id: row.get(0)?,
                    job_name: row.get(1)?,
                    status: row.get(2)?,
                    started_at: row.get(3)?,
                    finished_at: row.get(4)?,
                    error_message: row.get(5)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let health_checks = {
        let conn = state.system_db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, object_type, object_id, checked_at, status_code, error_message, is_healthy FROM health_checks ORDER BY checked_at DESC LIMIT 20;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(HealthCheckRow {
                    id: row.get(0)?,
                    object_type: row.get(1)?,
                    object_id: row.get(2)?,
                    checked_at: row.get(3)?,
                    status_code: row.get(4)?,
                    error_message: row.get(5)?,
                    is_healthy: row.get(6)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::HealthTemplate {
        admin_username: user.username,
        db_reports,
        total_data_size,
        system_db_size,
        users_db_size,
        admin_db_size,
        tenants_db_size,
        job_history,
        health_checks,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

// GET /admin/backups
pub async fn backups_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let mut files = vec![];
    if let Ok(dir_entries) = std::fs::read_dir(&state.config.backup_dir) {
        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
                {
                    if filename.ends_with(".tar.gz") {
                        let meta = entry.metadata().unwrap();
                        let size_str = format_size(meta.len());
                        let created_str = meta
                            .created()
                            .ok()
                            .map(|c| {
                                let datetime: chrono::DateTime<chrono::Utc> = c.into();
                                datetime.format("%Y-%m-%d %H:%M:%S").to_string()
                            })
                            .unwrap_or_else(|| "-".to_string());
                        files.push(BackupFileRow {
                            filename,
                            size_str,
                            created_str,
                        });
                    }
                }
            }
        }
    }
    files.sort_by(|a, b| b.filename.cmp(&a.filename));

    let history = {
        let conn = state.system_db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, backup_path, status, created_at, size_bytes, error_message FROM backup_history ORDER BY created_at DESC LIMIT 30;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(BackupHistoryRow {
                    id: row.get(0)?,
                    backup_path: row.get(1)?,
                    status: row.get(2)?,
                    created_at: row.get(3)?,
                    size_bytes: row.get(4)?,
                    error_message: row.get(5)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::BackupsTemplate {
        admin_username: user.username,
        files,
        history,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

// POST /admin/backups/create
pub async fn backups_create_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (_user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let form_csrf = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &form_csrf) {
        return Redirect::to("/admin/backups?error=Invalid CSRF token").into_response();
    }

    match crate::jobs::backup::perform_backup(&state.db, &state.config).await {
        Ok(path) => {
            let filename = std::path::Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("backup.tar.gz");
            Redirect::to(&format!(
                "/admin/backups?success=Backup created successfully: {}",
                filename
            ))
            .into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/admin/backups?error=Failed to generate backup: {}",
            e
        ))
        .into_response(),
    }
}

// GET /admin/backups/download/:filename
pub async fn backups_download_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(filename): Path<String>,
) -> Response {
    if require_auth(&state, &jar).await.is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let backup_path = state.config.backup_dir.join(&filename);
    if !backup_path.exists() {
        return StatusCode::NOT_FOUND.into_response();
    }

    match std::fs::read(&backup_path) {
        Ok(bytes) => {
            let body = axum::body::Body::from(bytes);
            Response::builder()
                .header("content-type", "application/octet-stream")
                .header(
                    "content-disposition",
                    format!("attachment; filename=\"{}\"", filename),
                )
                .body(body)
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

// POST /admin/backups/delete/:filename
pub async fn backups_delete_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(filename): Path<String>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let form_csrf = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &form_csrf) {
        return Redirect::to("/admin/backups?error=Invalid CSRF token").into_response();
    }

    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Redirect::to("/admin/backups?error=Invalid filename").into_response();
    }

    let backup_path = state.config.backup_dir.join(&filename);
    if !backup_path.exists() {
        return Redirect::to("/admin/backups?error=Backup file not found").into_response();
    }

    match std::fs::remove_file(&backup_path) {
        Ok(_) => {
            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &user.username,
                "BACKUP_DELETE",
                "backup",
                &filename,
                None,
            );
            Redirect::to("/admin/backups?success=Backup archive deleted").into_response()
        }
        Err(e) => Redirect::to(&format!(
            "/admin/backups?error=Failed to delete backup file: {}",
            e
        ))
        .into_response(),
    }
}

// POST /admin/backups/restore
pub async fn backups_restore_post(
    State(state): State<AppState>,
    jar: CookieJar,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let (user, session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let mut backup_bytes = vec![];
    let mut confirm_text = String::new();
    let mut csrf_token = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        if name == "backup_file" {
            if let Ok(bytes) = field.bytes().await {
                backup_bytes = bytes.to_vec();
            }
        } else if name == "confirm_text" {
            if let Ok(text) = field.text().await {
                confirm_text = text.trim().to_string();
            }
        } else if name == "csrf_token" {
            if let Ok(text) = field.text().await {
                csrf_token = text.trim().to_string();
            }
        }
    }

    if !verify_csrf(&session_id, &csrf_token) {
        return Redirect::to("/admin/backups?error=Invalid CSRF token").into_response();
    }

    if confirm_text != "RESTORE" {
        return Redirect::to(
            "/admin/backups?error=Confirmation text mismatch. Please type RESTORE.",
        )
        .into_response();
    }

    if backup_bytes.is_empty() {
        return Redirect::to("/admin/backups?error=Backup file is empty or missing.")
            .into_response();
    }

    let temp_file_path = state.config.data_dir.join("temp-restore-upload.tar.gz");
    if let Err(e) = std::fs::write(&temp_file_path, &backup_bytes) {
        return Redirect::to(&format!(
            "/admin/backups?error=Failed to save uploaded file: {}",
            e
        ))
        .into_response();
    }

    match crate::cli::restore::run(
        temp_file_path.to_string_lossy().to_string(),
        None,
        state.config.clone(),
    )
    .await
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&temp_file_path);
            let conn = state.users_db.lock().unwrap();
            let _ = conn.execute("DELETE FROM sessions;", []);

            let system_conn = state.system_db.lock().unwrap();
            let _ = crate::db::audit_events::write_audit_event(
                &system_conn,
                &user.username,
                "RESTORE_EXECUTION",
                "backup",
                "upload",
                None,
            );
            Redirect::to("/admin/login?success=Restore successful. Please log in again.")
                .into_response()
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp_file_path);
            Redirect::to(&format!("/admin/backups?error=Restore failed: {}", e)).into_response()
        }
    }
}

// GET /api-tokens
pub async fn api_tokens_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let tokens = {
        let conn = state.users_db.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, token_hash, created_at FROM api_tokens WHERE user_id = ?1;",
            )
            .unwrap();
        let rows = stmt
            .query_map([user.id], |row| {
                Ok(crate::models::UserApiToken {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    token_hash: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::ApiTokensTemplate {
        admin_username: user.username.clone(),
        username: user.username,
        tokens,
        new_token: params.get("new_token").cloned(),
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

// POST /api-tokens/create
pub async fn api_tokens_create_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let form_csrf = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &form_csrf) {
        return Redirect::to("/api-tokens?error=Invalid CSRF token").into_response();
    }

    use sha2::Digest;
    let raw_token = format!("key_{}", generate_token(32));
    let mut hasher = sha2::Sha256::new();
    hasher.update(raw_token.as_bytes());
    let hashed_token = hex::encode(hasher.finalize());

    {
        let conn = state.users_db.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let _ = conn.execute(
            "INSERT INTO api_tokens (user_id, token_hash, created_at) VALUES (?1, ?2, ?3);",
            rusqlite::params![user.id, hashed_token, now],
        );
    }

    {
        let conn_sys = state.system_db.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &conn_sys,
            &user.username,
            "API_TOKEN_CREATED",
            "api_token",
            "new",
            None,
        );
    }

    Redirect::to(&format!(
        "/api-tokens?new_token={}&success=Token generated successfully",
        raw_token
    ))
    .into_response()
}

// POST /api-tokens/revoke/:id
pub async fn api_tokens_revoke_post(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(token_id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let form_csrf = form.get("csrf_token").cloned().unwrap_or_default();
    if !verify_csrf(&session_id, &form_csrf) {
        return Redirect::to("/api-tokens?error=Invalid CSRF token").into_response();
    }

    {
        let conn = state.users_db.lock().unwrap();
        let _ = conn.execute(
            "DELETE FROM api_tokens WHERE id = ?1 AND user_id = ?2;",
            [token_id, user.id],
        );
    }

    {
        let conn_sys = state.system_db.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &conn_sys,
            &user.username,
            "API_TOKEN_REVOKED",
            "api_token",
            &token_id.to_string(),
            None,
        );
    }

    Redirect::to("/api-tokens?success=API token revoked").into_response()
}

// GET /analytics
pub async fn user_analytics_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let (user, session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return Redirect::to("/user/dashboard?error=Database error").into_response(),
    };

    let conn = user_dbs.analytics.lock().unwrap();

    let total_clicks = conn
        .query_row("SELECT COUNT(*) FROM visits;", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0);
    let unique_visitors = conn
        .query_row(
            "SELECT COUNT(DISTINCT ip_address) FROM visits;",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let direct_clicks = conn
        .query_row(
            "SELECT COUNT(*) FROM visits WHERE referer = '' OR referer = 'direct';",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let referred_clicks = total_clicks - direct_clicks;

    let referrers_chart = "<div>No referrer channels logged yet</div>".to_string();
    let browsers_chart = "<div>No browser traffic logged yet</div>".to_string();

    let visits = {
        let mut stmt = conn.prepare("SELECT id, target_type, target_id, timestamp, ip_address, user_agent, referer, accept_language, country, status_code FROM visits ORDER BY timestamp DESC LIMIT 50;").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(crate::models::VisitRecord {
                    id: row.get(0)?,
                    target_type: row.get(1)?,
                    target_id: row.get(2)?,
                    timestamp: row.get(3)?,
                    ip_address: row.get(4)?,
                    user_agent: row.get(5)?,
                    referer: row.get(6)?,
                    accept_language: row.get(7)?,
                    country: row.get(8)?,
                    status_code: row.get(9)?,
                    owner_user_id: Some(user.id),
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok()).collect()
    };

    let csrf_token = generate_csrf_token(&session_id);

    let template = crate::templates::UserAnalyticsTemplate {
        admin_username: user.username.clone(),
        username: user.username,
        total_clicks,
        unique_visitors,
        direct_clicks,
        referred_clicks,
        referrers_chart,
        browsers_chart,
        visits,
        csrf_token,
        success: params.get("success").cloned(),
        error: params.get("error").cloned(),
    };

    template.into_response()
}

pub async fn user_url_analytics_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Response {
    let (user, _session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return Redirect::to("/user/urls?error=Database error").into_response(),
    };

    let url = {
        let conn = user_dbs.content.lock().unwrap();
        match crate::db::content::get_url_by_id(&conn, &id) {
            Ok(Some(u)) => u,
            Ok(None) => return Redirect::to("/user/urls?error=Link not found").into_response(),
            Err(_) => return Redirect::to("/user/urls?error=Database error").into_response(),
        }
    };

    let visits = {
        let conn = user_dbs.analytics.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, target_type, target_id, timestamp, ip_address, user_agent, referer, accept_language, country, status_code 
                 FROM visits WHERE target_type = 'url' AND target_id = ?1 ORDER BY timestamp DESC;"
            )
            .unwrap();
        let rows = stmt
            .query_map([&url.id], |row| {
                Ok(crate::models::VisitRecord {
                    id: row.get(0)?,
                    target_type: row.get(1)?,
                    target_id: row.get(2)?,
                    timestamp: row.get(3)?,
                    ip_address: row.get(4)?,
                    user_agent: row.get(5)?,
                    referer: row.get(6)?,
                    accept_language: row.get(7)?,
                    country: row.get(8)?,
                    status_code: row.get(9)?,
                    owner_user_id: Some(user.id),
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok())
            .enumerate()
            .map(|(idx, r)| {
                let (browser, _, _) = parse_ua(&r.user_agent);
                let referrer = clean_referrer(&r.referer);
                crate::templates::VisitorLogEntry {
                    sr: idx + 1,
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
            .collect::<Vec<_>>()
    };

    let template = crate::templates::UserUrlAnalyticsTemplate {
        admin_username: user.username.clone(),
        username: user.username,
        url_code: url.code,
        destination: url.destination,
        visits,
    };

    template.into_response()
}

pub async fn user_page_analytics_get(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Response {
    let (user, _session_id) = match require_user_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    let user_dbs = match state.get_user_dbs(user.id) {
        Ok(dbs) => dbs,
        Err(_) => return Redirect::to("/user/pages?error=Database error").into_response(),
    };

    let page = {
        let conn = user_dbs.content.lock().unwrap();
        match crate::db::content::get_landing_page_by_id(&conn, &id) {
            Ok(Some(p)) => p,
            Ok(None) => {
                return Redirect::to("/user/pages?error=Landing page not found").into_response()
            }
            Err(_) => return Redirect::to("/user/pages?error=Database error").into_response(),
        }
    };

    let visits = {
        let conn = user_dbs.analytics.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, target_type, target_id, timestamp, ip_address, user_agent, referer, accept_language, country, status_code 
                 FROM visits WHERE target_type = 'page' AND target_id = ?1 ORDER BY timestamp DESC;"
            )
            .unwrap();
        let rows = stmt
            .query_map([&page.id], |row| {
                Ok(crate::models::VisitRecord {
                    id: row.get(0)?,
                    target_type: row.get(1)?,
                    target_id: row.get(2)?,
                    timestamp: row.get(3)?,
                    ip_address: row.get(4)?,
                    user_agent: row.get(5)?,
                    referer: row.get(6)?,
                    accept_language: row.get(7)?,
                    country: row.get(8)?,
                    status_code: row.get(9)?,
                    owner_user_id: Some(user.id),
                })
            })
            .unwrap();
        rows.filter_map(|r| r.ok())
            .enumerate()
            .map(|(idx, r)| {
                let (browser, _, _) = parse_ua(&r.user_agent);
                let referrer = clean_referrer(&r.referer);
                crate::templates::VisitorLogEntry {
                    sr: idx + 1,
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
            .collect::<Vec<_>>()
    };

    let template = crate::templates::UserPageAnalyticsTemplate {
        admin_username: user.username.clone(),
        username: user.username,
        page_code: page.code,
        title: page.title,
        visits,
    };

    template.into_response()
}

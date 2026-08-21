use super::*;

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

    {
        let users_conn = state.users_db.lock().unwrap();
        if !crate::db::users::check_quota_limit(&users_conn, user.id, "urls").unwrap_or(false) {
            return Redirect::to("/user/urls?error=Quota limit exceeded").into_response();
        }
    }

    let owner_tid = user
        .tenant_id
        .unwrap_or_else(crate::identity::TenantId::generate);

    {
        let reserved_conn = state.db.reserved.lock().unwrap();
        let urls_conn = state.db.global_urls.lock().unwrap();
        let pages_conn = state.db.global_landing_pages.lock().unwrap();
        if let Err(e) = crate::db::slugs::reserve_url_slug(
            &reserved_conn,
            &urls_conn,
            &pages_conn,
            &code,
            &owner_tid,
        ) {
            return Redirect::to(&format!(
                "/user/urls?error=Short code/slug unavailable: {}",
                e
            ))
            .into_response();
        }
    }

    let dest = match crate::services::urls::prepare_destination(
        &form.destination,
        crate::services::urls::UtmParams {
            source: Some(&form.utm_source),
            medium: Some(&form.utm_medium),
            campaign: Some(&form.utm_campaign),
        },
    ) {
        Ok(d) => d,
        Err(msg) => {
            let urls_conn = state.db.global_urls.lock().unwrap();
            let _ = crate::db::slugs::release_url_slug(&urls_conn, &code, &owner_tid);
            return Redirect::to(&format!("/user/urls?error={}", msg)).into_response();
        }
    };

    let expires_at_opt = crate::services::urls::parse_expires_at_input(&form.expires_at);

    let password_hash_opt = if form.password.trim().is_empty() {
        None
    } else {
        match hash_password(&form.password) {
            Ok(h) => Some(h),
            Err(_) => {
                let urls_conn = state.db.global_urls.lock().unwrap();
                let _ = crate::db::slugs::release_url_slug(&urls_conn, &code, &owner_tid);
                return Redirect::to("/user/urls?error=Hashing error").into_response();
            }
        }
    };

    let max_access_count_opt = if form.max_access_count.trim().is_empty() {
        None
    } else {
        match form.max_access_count.trim().parse::<i64>() {
            Ok(c) => Some(c),
            Err(_) => {
                let urls_conn = state.db.global_urls.lock().unwrap();
                let _ = crate::db::slugs::release_url_slug(&urls_conn, &code, &owner_tid);
                return Redirect::to("/user/urls?error=Invalid max access count").into_response();
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
            {
                let urls_conn = state.db.global_urls.lock().unwrap();
                let _ = crate::db::slugs::activate_url_slug(&urls_conn, &code, &url.id);
            }
            {
                let users_conn = state.users_db.lock().unwrap();
                let _ = crate::db::users::increment_quota_counter(&users_conn, user.id, "urls");
            }
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
        Err(e) => {
            let urls_conn = state.db.global_urls.lock().unwrap();
            let _ = crate::db::slugs::release_url_slug(&urls_conn, &code, &owner_tid);
            Redirect::to(&format!("/user/urls?error=Database error: {}", e)).into_response()
        }
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
                    {
                        let urls_conn = state.db.global_urls.lock().unwrap();
                        let _ = urls_conn
                            .execute("DELETE FROM global_urls WHERE slug = ?1;", [&url.code]);
                    }
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
        let conn = state.db.global_urls.lock().unwrap();
        let total_records: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM global_urls WHERE status != 'retired';",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
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

        let mut stmt = conn.prepare(
            "SELECT slug, owner_tenant_id, target_id, created_at, updated_at, status FROM global_urls WHERE status != 'retired' ORDER BY created_at DESC LIMIT ?1 OFFSET ?2;"
        ).unwrap();
        let rows = stmt
            .query_map(rusqlite::params![PAGE_SIZE as i64, offset as i64], |row| {
                let slug: String = row.get(0)?;
                let owner_tid_str: String = row.get(1)?;
                let target_id: String = row.get(2)?;
                let created_at: String = row.get(3)?;
                let updated_at: String = row.get(4)?;
                let status: String = row.get(5)?;
                Ok((
                    slug,
                    owner_tid_str,
                    target_id,
                    created_at,
                    updated_at,
                    status,
                ))
            })
            .unwrap();

        let mut urls = Vec::new();
        for item in rows.flatten() {
            let (slug, owner_tid_str, target_id, created_at, updated_at, status) = item;
            let mut resolved_url = None;
            if let Ok(tid) = owner_tid_str.parse::<crate::identity::TenantId>() {
                if let Ok(user_dbs) = state.open_tenant(tid, crate::state::TenantOpenMode::CoreJob)
                {
                    let u_conn = user_dbs.content.lock().unwrap();
                    if let Ok(Some(u)) = crate::db::content::get_url_by_id(&u_conn, &target_id) {
                        resolved_url = Some(u);
                    }
                }
            }
            if let Some(u) = resolved_url {
                urls.push(u);
            } else {
                urls.push(crate::models::Url {
                    id: target_id,
                    code: slug,
                    destination: String::new(),
                    title: None,
                    description: None,
                    created_at,
                    updated_at,
                    status,
                    tags: vec![],
                    expires_at: None,
                    password_hash: None,
                    max_access_count: None,
                    access_count: 0,
                    expired: false,
                    last_latency_ms: None,
                    last_status: None,
                });
            }
        }

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

#[derive(Deserialize, Default)]
#[serde(default)]
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
    _headers: HeaderMap,
    _connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(_form): Form<CreateUrlForm>,
) -> Response {
    let (_user, _session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    Redirect::to("/admin/urls?error=Admin is a platform operator and cannot create unowned application URLs; create links via a tenant user account").into_response()
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

    // Look up owner tenant in global_urls
    let owner_info = {
        let conn = state.db.global_urls.lock().unwrap();
        conn.query_row(
            "SELECT slug, owner_tenant_id FROM global_urls WHERE target_id = ?1 OR slug = ?1 LIMIT 1;",
            rusqlite::params![id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).ok()
    };

    if let Some((slug, tid_str)) = owner_info {
        if let Ok(tid) = tid_str.parse::<crate::identity::TenantId>() {
            if let Ok(user_dbs) = state.open_tenant(tid, crate::state::TenantOpenMode::CoreJob) {
                let conn = user_dbs.content.lock().unwrap();
                let _ = delete_url(&conn, &id);
            }
        }
        let conn = state.db.global_urls.lock().unwrap();
        let _ = conn.execute(
            "UPDATE global_urls SET status = 'retired', updated_at = ?1 WHERE slug = ?2;",
            rusqlite::params![chrono::Utc::now().to_rfc3339(), slug],
        );
    }

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

use super::*;

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

    {
        let users_conn = state.users_db.lock().unwrap();
        if !crate::db::users::check_quota_limit(&users_conn, user.id, "landings").unwrap_or(false) {
            return Redirect::to("/user/pages?error=Quota limit exceeded").into_response();
        }
    }

    let owner_tid = user
        .tenant_id
        .unwrap_or_else(crate::identity::TenantId::generate);

    {
        let reserved_conn = state.db.reserved.lock().unwrap();
        let urls_conn = state.db.global_urls.lock().unwrap();
        let pages_conn = state.db.global_landing_pages.lock().unwrap();
        if let Err(e) = crate::db::slugs::reserve_landing_page_slug(
            &reserved_conn,
            &urls_conn,
            &pages_conn,
            &code,
            &owner_tid,
        ) {
            return Redirect::to(&format!(
                "/user/pages?error=Short code/slug unavailable: {}",
                e
            ))
            .into_response();
        }
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
            {
                let pages_conn = state.db.global_landing_pages.lock().unwrap();
                let _ = crate::db::slugs::activate_landing_page_slug(&pages_conn, &code, &page.id);
            }
            {
                let users_conn = state.users_db.lock().unwrap();
                let _ = crate::db::users::increment_quota_counter(&users_conn, user.id, "landings");
            }
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
        Err(e) => {
            let pages_conn = state.db.global_landing_pages.lock().unwrap();
            let _ = crate::db::slugs::release_landing_page_slug(&pages_conn, &code, &owner_tid);
            Redirect::to(&format!("/user/pages?error=Database error: {}", e)).into_response()
        }
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
                    {
                        let pages_conn = state.db.global_landing_pages.lock().unwrap();
                        let _ = pages_conn.execute(
                            "DELETE FROM global_landing_pages WHERE slug = ?1;",
                            [&page.code],
                        );
                    }
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
        let conn = state.db.global_landing_pages.lock().unwrap();
        let total_records: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM global_landing_pages WHERE status != 'retired';",
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
            "SELECT slug, owner_tenant_id, target_id, created_at, updated_at, status FROM global_landing_pages WHERE status != 'retired' ORDER BY created_at DESC LIMIT ?1 OFFSET ?2;"
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

        let mut pages = Vec::new();
        for item in rows.flatten() {
            let (slug, owner_tid_str, target_id, created_at, updated_at, status) = item;
            let mut resolved_page = None;
            if let Ok(tid) = owner_tid_str.parse::<crate::identity::TenantId>() {
                if let Ok(user_dbs) = state.open_tenant(tid, crate::state::TenantOpenMode::CoreJob)
                {
                    let u_conn = user_dbs.content.lock().unwrap();
                    if let Ok(Some(p)) =
                        crate::db::content::get_landing_page_by_id(&u_conn, &target_id)
                    {
                        resolved_page = Some(p);
                    }
                }
            }
            if let Some(p) = resolved_page {
                pages.push(p);
            } else {
                pages.push(crate::models::LandingPage {
                    id: target_id,
                    code: slug.clone(),
                    slug,
                    title: String::new(),
                    html_content: String::new(),
                    state: status,
                    created_at,
                    updated_at,
                });
            }
        }

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

#[derive(Deserialize, Default)]
#[serde(default)]
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
    _headers: HeaderMap,
    _connect_info: Option<ConnectInfo<SocketAddr>>,
    Form(_form): Form<CreatePageForm>,
) -> Response {
    let (_user, _session_id) = match require_auth(&state, &jar).await {
        Ok(u) => u,
        Err(redir) => return redir.into_response(),
    };

    Redirect::to("/admin/pages?error=Admin is a platform operator and cannot create unowned application pages; create landing pages via a tenant user account").into_response()
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

    // Look up owner tenant in global_landing_pages
    let owner_info = {
        let conn = state.db.global_landing_pages.lock().unwrap();
        conn.query_row(
            "SELECT slug, owner_tenant_id FROM global_landing_pages WHERE target_id = ?1 OR slug = ?1 LIMIT 1;",
            rusqlite::params![id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).ok()
    };

    if let Some((slug, tid_str)) = owner_info {
        if let Ok(tid) = tid_str.parse::<crate::identity::TenantId>() {
            if let Ok(user_dbs) = state.open_tenant(tid, crate::state::TenantOpenMode::CoreJob) {
                let conn = user_dbs.content.lock().unwrap();
                let _ = delete_landing_page(&conn, &id);
            }
        }
        let conn = state.db.global_landing_pages.lock().unwrap();
        let _ = conn.execute(
            "UPDATE global_landing_pages SET status = 'retired', updated_at = ?1 WHERE slug = ?2;",
            rusqlite::params![chrono::Utc::now().to_rfc3339(), slug],
        );
    }

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

use super::*;

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
            "SELECT slug, owner_user_id, target_type, target_id, created_at, updated_at, status, deleted_at \
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
            "SELECT id, timestamp, admin_username, target_user_id, target_username, resource_type, resource_identifier, action, severity, reason \
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
            "SELECT id, slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username \
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
            "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username) \
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
            "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username) \
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

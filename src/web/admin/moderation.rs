use super::*;
use crate::identity::TenantId;

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

    let mut flagged_items: Vec<GlobalSlugRow> = Vec::new();

    // Query global_urls.db
    {
        let urls_conn = state.db.global_urls.lock().unwrap();
        let users_conn = state.users_db.lock().unwrap();
        let query_res = urls_conn.prepare(
            "SELECT slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at \
             FROM global_urls WHERE status = 'flagged' OR status = 'disabled' ORDER BY updated_at DESC;"
        ).and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| {
                let tid_str: String = row.get(1)?;
                let uid = if let Ok(tid) = TenantId::parse(&tid_str) {
                    crate::db::users::get_user_by_tenant_id(&users_conn, tid)
                        .ok()
                        .flatten()
                        .map(|u| u.id)
                        .unwrap_or(0)
                } else {
                    0
                };
                Ok(GlobalSlugRow {
                    slug: row.get(0)?,
                    owner_user_id: uid,
                    target_type: "url".to_string(),
                    target_id: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    status: row.get(5)?,
                    deleted_at: row.get(6)?,
                })
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
        });
        if let Ok(items) = query_res {
            flagged_items.extend(items);
        }
    }

    // Query global_landing_pages.db
    {
        let pages_conn = state.db.global_landing_pages.lock().unwrap();
        let users_conn = state.users_db.lock().unwrap();
        let query_res = pages_conn.prepare(
            "SELECT slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at \
             FROM global_landing_pages WHERE status = 'flagged' OR status = 'disabled' ORDER BY updated_at DESC;"
        ).and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| {
                let tid_str: String = row.get(1)?;
                let uid = if let Ok(tid) = TenantId::parse(&tid_str) {
                    crate::db::users::get_user_by_tenant_id(&users_conn, tid)
                        .ok()
                        .flatten()
                        .map(|u| u.id)
                        .unwrap_or(0)
                } else {
                    0
                };
                Ok(GlobalSlugRow {
                    slug: row.get(0)?,
                    owner_user_id: uid,
                    target_type: "page".to_string(),
                    target_id: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    status: row.get(5)?,
                    deleted_at: row.get(6)?,
                })
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
        });
        if let Ok(items) = query_res {
            flagged_items.extend(items);
        }
    }

    flagged_items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

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

    let slug_info = match state.lookup_slug(&form.slug) {
        Ok(Some(info)) => info,
        _ => return Redirect::to("/admin/moderation?error=Slug not found").into_response(),
    };

    let owner_tenant_id = match TenantId::parse(&slug_info.owner_tenant_id) {
        Ok(t) => t,
        Err(_) => {
            return Redirect::to("/admin/moderation?error=Invalid tenant on slug").into_response()
        }
    };

    let (owner_user_id, owner_username) = {
        let users_conn = state.users_db.lock().unwrap();
        match crate::db::users::get_user_by_tenant_id(&users_conn, owner_tenant_id) {
            Ok(Some(u)) => (u.id, Some(u.username)),
            _ => (0, None),
        }
    };

    let now = Utc::now().to_rfc3339();

    // 1. Update v0.8 slug database
    if action == "deleted" {
        if slug_info.target_type == crate::db::slugs::SlugTargetType::Url {
            let urls_conn = state.db.global_urls.lock().unwrap();
            let _ = urls_conn.execute("DELETE FROM global_urls WHERE slug = ?1;", [&form.slug]);
        } else {
            let pages_conn = state.db.global_landing_pages.lock().unwrap();
            let _ = pages_conn.execute(
                "DELETE FROM global_landing_pages WHERE slug = ?1;",
                [&form.slug],
            );
        }
    } else {
        if slug_info.target_type == crate::db::slugs::SlugTargetType::Url {
            let urls_conn = state.db.global_urls.lock().unwrap();
            let _ = urls_conn.execute(
                "UPDATE global_urls SET status = ?1, updated_at = ?2 WHERE slug = ?3;",
                rusqlite::params![action, now, form.slug],
            );
        } else {
            let pages_conn = state.db.global_landing_pages.lock().unwrap();
            let _ = pages_conn.execute(
                "UPDATE global_landing_pages SET status = ?1, updated_at = ?2 WHERE slug = ?3;",
                rusqlite::params![action, now, form.slug],
            );
        }
    }

    // 2. Sync to tenant content DB if accessible
    if let Ok(tenant_dbs) =
        state.open_tenant(owner_tenant_id, crate::db::tenant::TenantOpenMode::CoreJob)
    {
        if let Ok(conn) = tenant_dbs.content.lock() {
            if slug_info.target_type == crate::db::slugs::SlugTargetType::Url {
                let content_status = if action == "disabled" || action == "deleted" {
                    "dead"
                } else {
                    "active"
                };
                let _ = conn.execute(
                    "UPDATE urls SET status = ?1 WHERE code = ?2;",
                    rusqlite::params![content_status, form.slug],
                );
            } else {
                let content_state = if action == "disabled" || action == "deleted" {
                    "archived"
                } else {
                    "published"
                };
                let _ = conn.execute(
                    "UPDATE landing_pages SET state = ?1 WHERE code = ?2;",
                    rusqlite::params![content_state, form.slug],
                );
            }
        }
    }

    // 3. Record moderation event and audit log in system.db
    {
        let system_conn = state.system_db.lock().unwrap();
        let event_id = Uuid::new_v4().to_string();
        let _ = system_conn.execute(
            "INSERT INTO moderation_events (id, timestamp, admin_username, target_user_id, target_username, resource_type, resource_identifier, action, severity, reason) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);",
            rusqlite::params![
                event_id,
                now,
                user.username,
                owner_user_id,
                owner_username,
                slug_info.target_type.as_str(),
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

    let mut slugs: Vec<GlobalSlugRow> = Vec::new();

    // Query global_urls.db
    {
        let urls_conn = state.db.global_urls.lock().unwrap();
        let users_conn = state.users_db.lock().unwrap();
        let mut sql = "SELECT slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at FROM global_urls WHERE 1=1".to_string();
        let mut values = vec![];

        if let Some(ref s) = query.search {
            if !s.trim().is_empty() {
                sql.push_str(" AND slug LIKE ?");
                values.push(rusqlite::types::Value::Text(format!("%{}%", s.trim())));
            }
        }
        if let Some(ref st) = query.status {
            if !st.trim().is_empty() {
                sql.push_str(" AND status = ?");
                values.push(rusqlite::types::Value::Text(st.trim().to_string()));
            }
        }
        sql.push_str(" ORDER BY created_at DESC LIMIT 100;");

        let items_res = urls_conn.prepare(&sql).and_then(|mut stmt| {
            let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), |row| {
                let tid_str: String = row.get(1)?;
                let uid = if let Ok(tid) = TenantId::parse(&tid_str) {
                    crate::db::users::get_user_by_tenant_id(&users_conn, tid)
                        .ok()
                        .flatten()
                        .map(|u| u.id)
                        .unwrap_or(0)
                } else {
                    0
                };
                Ok(GlobalSlugRow {
                    slug: row.get(0)?,
                    owner_user_id: uid,
                    target_type: "url".to_string(),
                    target_id: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    status: row.get(5)?,
                    deleted_at: row.get(6)?,
                })
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
        });

        if let Ok(items) = items_res {
            for r in items {
                if let Some(o) = query.owner {
                    if r.owner_user_id != o {
                        continue;
                    }
                }
                slugs.push(r);
            }
        }
    }

    // Query global_landing_pages.db
    {
        let pages_conn = state.db.global_landing_pages.lock().unwrap();
        let users_conn = state.users_db.lock().unwrap();
        let mut sql = "SELECT slug, owner_tenant_id, target_id, created_at, updated_at, status, retired_at FROM global_landing_pages WHERE 1=1".to_string();
        let mut values = vec![];

        if let Some(ref s) = query.search {
            if !s.trim().is_empty() {
                sql.push_str(" AND slug LIKE ?");
                values.push(rusqlite::types::Value::Text(format!("%{}%", s.trim())));
            }
        }
        if let Some(ref st) = query.status {
            if !st.trim().is_empty() {
                sql.push_str(" AND status = ?");
                values.push(rusqlite::types::Value::Text(st.trim().to_string()));
            }
        }
        sql.push_str(" ORDER BY created_at DESC LIMIT 100;");

        let items_res = pages_conn.prepare(&sql).and_then(|mut stmt| {
            let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), |row| {
                let tid_str: String = row.get(1)?;
                let uid = if let Ok(tid) = TenantId::parse(&tid_str) {
                    crate::db::users::get_user_by_tenant_id(&users_conn, tid)
                        .ok()
                        .flatten()
                        .map(|u| u.id)
                        .unwrap_or(0)
                } else {
                    0
                };
                Ok(GlobalSlugRow {
                    slug: row.get(0)?,
                    owner_user_id: uid,
                    target_type: "page".to_string(),
                    target_id: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    status: row.get(5)?,
                    deleted_at: row.get(6)?,
                })
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
        });

        if let Ok(items) = items_res {
            for r in items {
                if let Some(o) = query.owner {
                    if r.owner_user_id != o {
                        continue;
                    }
                }
                slugs.push(r);
            }
        }
    }

    slugs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let history = {
        let system_conn = state.system_db.lock().unwrap();
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

    let req = crate::services::slug_transfer::SlugTransferRequest {
        slug: form.slug.clone(),
        new_owner_user_id: form.new_owner_user_id,
    };

    match crate::services::slug_transfer::transfer_slug(&state, &req, &user.username) {
        Ok(_) => Redirect::to(&format!(
            "/admin/slugs?success=Slug /{} successfully transferred to user {}",
            form.slug, form.new_owner_user_id
        ))
        .into_response(),
        Err(e) => Redirect::to(&format!("/admin/slugs?error={}", e.message())).into_response(),
    }
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

    let slug_info = match state.lookup_slug(&form.slug) {
        Ok(Some(info)) => info,
        _ => return Redirect::to("/admin/slugs?error=Slug not found").into_response(),
    };

    let now = Utc::now().to_rfc3339();
    if slug_info.target_type == crate::db::slugs::SlugTargetType::Url {
        let urls_conn = state.db.global_urls.lock().unwrap();
        let _ = urls_conn.execute(
            "UPDATE global_urls SET status = ?1, updated_at = ?2 WHERE slug = ?3;",
            rusqlite::params![status, now, form.slug],
        );
    } else {
        let pages_conn = state.db.global_landing_pages.lock().unwrap();
        let _ = pages_conn.execute(
            "UPDATE global_landing_pages SET status = ?1, updated_at = ?2 WHERE slug = ?3;",
            rusqlite::params![status, now, form.slug],
        );
    }

    {
        let system_conn = state.system_db.lock().unwrap();
        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
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

    let slug_info = match state.lookup_slug(&form.slug) {
        Ok(Some(info)) => info,
        _ => return Redirect::to("/admin/slugs?error=Slug not found").into_response(),
    };

    let old_owner_user_id = {
        let users_conn = state.users_db.lock().unwrap();
        if let Ok(tid) = TenantId::parse(&slug_info.owner_tenant_id) {
            crate::db::users::get_user_by_tenant_id(&users_conn, tid)
                .ok()
                .flatten()
                .map(|u| u.id)
        } else {
            None
        }
    };

    let now = Utc::now().to_rfc3339();

    if slug_info.target_type == crate::db::slugs::SlugTargetType::Url {
        let urls_conn = state.db.global_urls.lock().unwrap();
        let _ = urls_conn.execute("DELETE FROM global_urls WHERE slug = ?1;", [&form.slug]);
    } else {
        let pages_conn = state.db.global_landing_pages.lock().unwrap();
        let _ = pages_conn.execute(
            "DELETE FROM global_landing_pages WHERE slug = ?1;",
            [&form.slug],
        );
    }

    {
        let system_conn = state.system_db.lock().unwrap();
        let _ = system_conn.execute(
            "INSERT INTO slug_history (slug, old_owner_user_id, new_owner_user_id, action, timestamp, admin_username) \
             VALUES (?1, ?2, NULL, 'deleted', ?3, ?4);",
            rusqlite::params![form.slug, old_owner_user_id, now, user.username],
        );

        let _ = crate::db::audit_events::write_audit_event(
            &system_conn,
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

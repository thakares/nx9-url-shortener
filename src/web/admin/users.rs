use super::*;

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

    if new_user.account_type == "standard" {
        if let Err(e) = state.db.init_user_databases(new_user.id) {
            return Redirect::to(&format!(
                "/admin/users?error=Failed to initialize user databases: {}",
                e
            ))
            .into_response();
        }
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

pub fn get_user_detail_stats(
    state: &AppState,
    user_id: i64,
) -> Result<UserDetailStats, Box<dyn std::error::Error>> {
    use rusqlite::OptionalExtension;
    let quotas = {
        let conn = state.users_db.lock().unwrap();
        conn.query_row(
            "SELECT max_urls, max_landings, max_api_tokens, max_storage_mb, current_urls, current_landings, current_api_tokens, current_storage_mb \
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
        match crate::db::users::get_user_by_id(&conn, id) {
            Ok(Some(u)) => u,
            _ => return Redirect::to("/admin/users?error=User not found").into_response(),
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
        match crate::db::users::get_user_by_id(&conn, id) {
            Ok(Some(u)) => u,
            _ => return Redirect::to("/admin/users?error=User not found").into_response(),
        }
    };

    let quotas = {
        let conn = state.users_db.lock().unwrap();
        conn.query_row(
            "SELECT max_urls, max_landings, max_api_tokens, max_storage_mb, current_urls, current_landings, current_api_tokens, current_storage_mb \
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
        "INSERT INTO quotas (user_id, max_urls, max_landings, max_api_tokens, max_storage_mb) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(user_id) DO UPDATE SET \
            max_urls = excluded.max_urls, \
            max_landings = excluded.max_landings, \
            max_api_tokens = excluded.max_api_tokens, \
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

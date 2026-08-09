use super::*;

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
